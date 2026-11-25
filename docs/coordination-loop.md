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

**3. Work.** The plan is first *rendered for the bead* by
`scripts/render_plan.py`, then bernstein runs the rendered copy — evidence,
then implementation, then measurement — under a soft and a hard budget. The
render step is not decoration: a plan cannot name its own bead, and
`bernstein run` does **not** apply a plan's `constraints:` (see "What the first
real run found", below), so the renderer writes the bead, the leased paths and
every house rule into the description of every step. The plan's
`completion_signals` are the actual gate commands: bernstein cannot mark a step
done on an agent's say-so, only on `tracecheck` agreeing with the Atari.
`--no-agent` skips the agent entirely when a human or a Claude Code subagent is
doing the work and only wants the loop's discipline around it.

The stage **fails closed**. If bernstein exits non-zero, or exits zero having
changed none of the leased paths, the loop stops: bead commented, nothing
committed, leases held. A gate can only judge work that exists.

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

## `bernstein doctor`, and which of its complaints matter

    mise run bernstein-doctor

It reports five issues on a fresh checkout. **One is real and is fixed here;
four are this container having one AI vendor's CLI instead of three.** The
distinction is worth writing down, because chasing the four wastes an
afternoon and installing them would be worse than wasting it.

**Fixed: `.sdd workspace` — missing or incomplete.** The check wants three
directories to exist: `.sdd`, `.sdd/backlog`, `.sdd/runtime`
(`_doctor_check_workspace` in bernstein's `status_cmd.py`; `config.yaml` alone
does not satisfy it, which is why adding only that file left the check red).
The skeleton is now committed — `.sdd/config.yaml` plus a `.gitkeep` in each
directory — so a fresh clone starts one issue down instead of being told to
run `bernstein init`, which would also scatter twenty-odd Python and frontend
plan templates through a Rust repository.

**Expected, not fixed: `Adapter: codex`, `Adapter: gemini`, and their two
`Auth:` rows.** `_doctor_check_adapters` loops over a hardcoded
`("claude", "codex", "gemini")` and `shutil.which()`es each one; the auth
check is hardcoded the same way. Neither consults `cli:` in `bernstein.yaml`,
so no configuration can turn them green — only installing two other vendors'
CLIs and authenticating them with keys this project does not have and must
not invent.

Nothing is degraded by their absence. `Ready to run` is computed as
`py_ok and (any_adapter and any_key)` — **any one** authenticated adapter
satisfies it, and claude's OAuth session does. The loop is pinned to `claude`
in `bernstein.yaml` so dispatch never depends on the other two.

Two advisories also ride along and are knowingly ignored: the last-green
canary noting that the installed claude CLI is ahead of the release its
conformance run has verified (bernstein's own projection, not something this
repository controls), and a newer pyright being available (a Python CI tool
this Rust project never invokes).

## Are the plan's gates real? (verified, because it is the load-bearing claim)

The loop's whole design rests on `plans/discr-loop.yaml`'s `completion_signals`
being *executed*, not decorative. Checked against bernstein **4.0.0** — the
version `mise.toml` pins — by running its own validator and reading its source:

- **`test_passes` runs a command and passes on exit 0**
  (`core/quality/janitor.py`), and the plan accepts `command:`, `value:`,
  `path:` or `contains:` as its value. So an **arbitrary project gate is
  registrable per task**, and `mise run core-check` in the measure stage
  genuinely runs.
- **`bernstein plan validate plans/discr-loop.yaml`** accepts the committed plan
  (3 stages, 5 tasks) and **rejects a typo**: with `test_passes` mis-spelled
  `tests_pass` it exits 1 with `expected a supported completion signal, got
  'tests_pass'`. `scripts/loop.sh` runs it before every `bernstein run` and
  stops on failure.

### Why bernstein is pinned: the same check gave the opposite answer on 3.18.2

This section originally warned that **an unrecognised `type:` is not an error**.
That was true of bernstein **3.18.2**, read out of its `plan_loader.py`: the
loader logged a warning and *skipped* the signal, so a typo silently deleted a
gate and the step passed on the agent's say-so.

Between two container restarts `latest` resolved to **4.0.0**, and the
behaviour reversed: signals now go through a strict `parse_completion_signal`
and an invalid one fails validation and loading. The earlier warning became
wrong without anyone changing a line of this repository — which is the case for
pinning: a claim about a tool's behaviour is a claim about a version. rust is
**not** pinned the same way and moved from 1.98 to 1.100 across the same
restart; the gate suite was re-run on 1.100 and every number is unchanged, but
that is a measurement to repeat on a toolchain bump, not a guarantee.

To bump bernstein: change the pin, run `bernstein plan validate` on the real
plan **and** on a copy with a deliberately wrong signal type, and re-read
`core/planning/signal_contract.py` before trusting the new answer.

## What the first real run found (and the dry run could not)

Everything above was "verified" by `plan validate` and `--dry-run`, and the
first three real `mise run loop` attempts all failed in stage 3 — **having
dispatched no agent and spent nothing**. Six independent defects, each
invisible to the checks that had passed (the first three from run one, the
fourth from run two, the fifth and sixth from run three):

1. **`--approval` is not a `bernstein run` option on 4.0.0.** It exists only on
   the top-level `bernstein -g` form. The loop passed it, `run` rejected it, and
   the loop treated the non-zero exit as non-fatal. The obvious fallback —
   `approval: pr` in `bernstein.yaml` — **does not work either**: the second
   real run printed `Ignoring unknown top-level key 'approval'` (and the same for
   `worktrees`). Both keys were dead config, and the comments beside them
   described a merge gate that never existed. They are removed, with a note not
   to re-add them. The "did you mean" spellings (`approvals`, `worktree_setup`)
   are different features, not renames. **There is no bernstein-side merge gate
   in this loop.** What protects master is outside bernstein: the loop commits
   only the leased paths, only after the gates pass, onto a feature branch, and
   a human merges the PR.
2. **`bernstein run` does not apply a plan's `constraints:`.** Its source names
   `("max_agents", "repos", "constraints")` as settings it does not apply, and
   `--from-plan` refuses a plan that sets one. Our house rules were in exactly
   that key, so they would never have reached an agent — the same class of
   failure as the 3.18.2 silent skip, and the plan header claimed the opposite.
   (`plan validate` accepts the key; only the run path refuses it.) Fix: the
   renderer removes the key and prepends the rules to every step description.
3. **A plan cannot name its own bead.** The steps said "read the bead" and
   nothing said which. The renderer fills it in, together with the leased paths.

4. **`bernstein run` detaches unless given `--wait`.** Found by the *second*
   real run, which got past all of the above: it posted five tasks, started a
   background task server and orchestrator, printed "Run continues in the
   background", and returned exit 0 after zero seconds with nothing done. The
   loop's new guard correctly stopped ("no work product") — but the orchestrator
   was still alive behind it, free to start spending against the budget while
   the loop reported that it had stopped and left the leases held. It was shut
   down by hand (`bernstein stop`: 0 tasks done, 0 branches merged, nothing
   spent). `--wait 3600` blocks until the run is terminal and exits with its
   outcome, so the loop's stages are now actually sequential.

5. **The spawned orchestrator re-reads `bernstein.yaml`, which has no `goal:` —
   and 4.0.0 requires one.** Found by the *third* real run, which blocked
   correctly under `--wait` and then failed with "orchestrator pid=… observed
   gone", 0 of 5 tasks started. In plan-file mode `bernstein run` bootstraps
   from the plan's *name* and hands the orchestrator it spawns no seed path; that
   process falls back to `./bernstein.yaml` and dies with `Seed file must contain
   a non-empty 'goal' string` — and the watchdog records
   `invalid-seed-configuration` and deliberately does not restart it. The cause
   was in `.sdd/runtime/orchestrator-debug.log`, not in anything `--wait`
   printed. `--seed` cannot fix this (plan mode ignores it). What every bernstein
   subprocess does resolve first is the `BERNSTEIN_SEED_PATH` environment variable
   (`core/config/seed.py: resolve_seed_path`). So the renderer now also writes a
   per-run seed — the committed `bernstein.yaml` plus `goal: <bead>: <title>` —
   and the loop exports it. The committed file stays goal-free on purpose: the
   tracker owns the goal, and a static one would compete with it. Verified with
   the real `parse_seed` and `resolve_seed_path`; the committed file alone still
   fails, so the variable is what fixes it. It also pins
   `internal_llm_provider: none`, so bernstein does not default to a provider
   whose key this project does not have and must not invent.
6. **A failed run leaves its task server and watchdog running.** Both outlived
   the loop that started them. The loop now runs `bernstein stop` on failure and
   prints the orchestrator's own `ERROR` lines, so "stopped" is true and the next
   diagnosis does not start from "run failed".

And one defect in the loop itself, which is the one that matters most: **a
failed bernstein run was non-fatal**, so the gates ran on an unchanged tree,
passed, and the loop carried on to land — it died only because a leased path
happened to not exist yet. Had one existed it would have committed nothing and
commented "gates green" on the bead. The loop now stops if bernstein fails, and
again if no leased path changed, because green gates on untouched code prove
nothing.

**Why the dry run missed all of it:** `--dry-run` called `bernstein --plan-only
-g` — a different command with a different flag set from the real `bernstein
run PLAN`. It now runs `bernstein run <rendered plan> <the same flags>
--dry-run`, so a dry run exercises what a real run will. It still cannot catch
a defect that only exists once agents are running.
