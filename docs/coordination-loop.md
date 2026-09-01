# The discr coordination loop

Ten waves of agents built this repository. The loop below is what those waves
converged on, written down and made executable, so the eleventh wave does not
have to rediscover it.

    mise run loop-plan                       # what would happen: no writes
    mise run loop                            # one bead, start to proposed MR
    sh scripts/loop.sh --bead discr-tun \
        --files "crates/disc-core/src/player.rs"

Three components, each with one job:

| | |
|---|---|
| **beads** (`bd`) | what to work on, and the durable record of what was learned. The backlog is `bd ready`; a finding that is not in a bead comment is a finding the next agent will pay to rediscover. |
| **pact** | who is touching which file, and what they said about it. Leases are advisory and respected anyway; `.pact/events.jsonl` is the history. |
| **bernstein** | the deterministic coordination layer: per-task worktrees, an audit chain, and no model inside the loop itself, so a run replays byte-identically. Installed via mise (`pipx:bernstein`). |

The gates decide everything. Not the agent's confidence, not the review — the
eleven `tracecheck` runs against emulator traces of the real Atari.

## The six stages, and why each one exists

Every stage below is in the script because skipping it cost a run.

**0. Preflight.** The container is ephemeral, so `scripts/setup_container.sh`
rebuilds Hatari, Ghidra, dolt, bd and pact first. Then the loop settles the
beads database *before* reading work from it: a dolt conflict left by an
earlier sync blocks every later `bd` write with an error that names neither
the loop nor the bead, and the first time that happened it looked like four
different bugs.

It also refuses to start unless `PACT_AGENT` is set and `BEADS_ACTOR` matches
it. pact never guesses an identity; bd falls through to the checkout's git
identity, which is how a fifteen-agent fleet can log sixteen distinct actors
in `.pact/events.jsonl` and exactly one in its task history.

**1. Orient — before touching a file, not before writing one.** `pact msg
inbox`, then `pact lease ls`, then `bd ready`. A peer planning against the
same path renegotiates now, cheaply, instead of at merge time when both plans
are sunk cost. With no `--bead`, the loop takes the highest-priority ready
non-epic bead.

**2. Claim.** The bead first (`bd update --assignee`, so `bd ready` stops
offering it to peers), then **one** lease covering every path the work will
write. Several paths in a single `acquire` are taken all-or-nothing, so the
loop never ends up holding half of what it needs while a peer holds the rest.
On contention it waits *inside* the command (`--wait 30m`): ending a turn to
wait is the same as exiting, and an agent that exits never resumes — measured
on one fleet as seven agents parked, four of which never came back, one of
them holding four finished and tested fixes.

Note `--assignee` rather than `--claim`: on bd 1.2.2 `--claim` writes no
interaction row, so `pact audit --check claim-lease-divergence` has nothing to
read afterwards.

**3. Work.** bernstein runs `plans/discr-loop.yaml` — evidence, then
implementation, then measurement — with `--approval pr`, so nothing merges
itself. The plan's constraints are this repository's house rules, and its
`completion_signals` are the actual gate commands: bernstein cannot mark a
step done on an agent's say-so, only on `tracecheck` agreeing with the Atari.
`--no-agent` skips this stage entirely when a human or a Claude Code subagent
is doing the work and only wants the loop's discipline around it.

**4. Verify.** `mise run core-check`. If the gates fail, the loop stops with
the bead commented, **nothing committed and the leases still held** — the
state a peer can read correctly. Gate numbers may grow and never shrink; a
clean run can only get shorter, and the number says by how much.

**5. Land.** Commit with explicit pathspecs (a bare `git commit` commits the
whole index, which in a shared checkout once swept a peer's staged deletion
into an unrelated commit) and `--trailer Pact-Agent=$PACT_AGENT`, because
every agent commits under the same git identity and without the trailer
`git log` cannot say which of them made a change. Then close or comment the
bead, export `.beads/issues.jsonl` — the remote container's git proxy accepts
pushes to branch heads only, so `bd dolt push` 403s and the JSONL export is
how bead changes leave the box — and only *then* release the leases. A lease
released while the work is uncommitted breaks the one binding
`pact audit --check commit-correlation` exists to prove.

**6. Propose the MR.** Automatically, always: the loop writes the MR body from
its own record — the bead, its description, the commits since `master`, and
the gate result — and opens the pull request with `gh` when a `gh` CLI is
present. In the remote container there is none by design, so the MR is
*proposed*: the body is written to `.sdd/mr/<branch>.md` and the head, base
and title are printed for the orchestrator to open through the GitHub MCP.

## What the loop deliberately does not do

It does not merge. It does not decide that a wall is unimportant. It does not
close a bead whose acceptance criteria are unmet — an honest partial with its
evidence outlives a closed bead that guessed, and this repository has a
`retract:` commit type precisely because three plausible models survived
eleven parts before a measurement killed them.
