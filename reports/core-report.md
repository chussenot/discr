# `disc-core` — acceptance report

`disc-core` re-implements the rules of Disc (Loriciel, 1990) in Rust from the
addresses in `docs/disc-notes.md`. `tracecheck` replays an Atari ST trace
against it and reports the first divergence. This is how far that agreement
actually goes, what is deliberately not modelled, and who owns each gap.

Same standard as `reports/oracle-report.md`: a number here was measured by a
command you can re-run, and a gap is named with the bead that tracks it.

Reproduce with `make core-check`, or ungated with `make tracecheck`.

## Result

    cargo run -p disc-tools --bin tracecheck -- \
        tests/fixtures/golden.ndjson --skip-waived

| | |
|---|---|
| Fixture | `tests/fixtures/golden.ndjson` — 100 frames, seeded from ST `$6ab4` = 6949, inside a 256-frame prefix where the oracle and Hatari agree byte-for-byte |
| Rows compared | 13 of the 15 `compared` rows of `docs/state-schema.md` (85 field instances); the trace has no column for `discs[n].vel_y` or `discs[n].damage` |
| **Ticks matched** | **10** |
| First divergence | frame 11, `players[0].state_index`, expected **20**, got **0** |
| Owner | **discr-75o** — nothing in `disc-core` enters or leaves a state |

Ten ticks is the number the gate is set to. It is not the interesting number,
because the thing that stops the run at frame 11 is one field that
`disc-core` never writes at all. Stepping past the two state-machine rows
with `--resync` measures the rest:

| what is resynced from the trace | ticks matched | first divergence | owner |
|---|---|---|---|
| nothing (the default) | **0** | frame 1, `players[1].world_x` 78 vs 81 | discr-b6x — p2 has no input |
| waived rows (`--skip-waived`) | **10** | frame 11, `players[0].state_index` 20 vs 0 | discr-75o |
| \+ `players[0].state_index` | **11** | frame 12, `players[0].facing` 20 vs 0 | **discr-xfw** (new) |
| \+ `players[0]` | **22** | frame 23, `discs[0].world_y` 82 vs 81 | discr-tan |
| \+ `discs[0].world_y` | **33** | frame 34, `discs[0].world_x` 45 vs 46 | discr-217 / **discr-0fm** (new) |
| \+ all of `discs` | **99** | none — the fixture ends | — |

Read the second-to-last row as the headline: **with the two player
state-machine rows and the one unowned `world_y` write supplied from the
trace, `disc-core` reproduces the disc's whole outbound flight — 33 ticks,
including the floor clamp at frame 11 and the velocity sign-flip on it —
plus player 1's walk, position and grid cell.**

Read the last row as the warning it is: it matches 99/99 only because the
grid never changes in this fixture. See "Honest limits".

## What is implemented, field by field

Every row of `docs/state-schema.md`'s compared table, against the ST site it
mirrors. "Verified" means a `tracecheck` run reached that frame with the row
compared, not resynced.

| field | ST | modelled as | verified |
|---|---|---|---|
| `frame` | `$6ab4`, `$8198 addq.w #1` | `wrapping_add(1)` first thing in `tick` | 99/99 ticks |
| `players[0].world_x` | `$6ca2`, `$f658 subq.w #3` / `$f86c addq.w #3` | `±3` gated on a whole-byte `cmp.b` of `$6c58` against the single direction bit, destination probed 24 units ahead (`$f60e`/`$f822`) through the `$7bfe` column table, `tst.w tile+$00` as the walkability gate (`$f63e`/`$f852`), clamped 8..152 | frames 1–22, including 8 frames of walking left |
| `players[0].world_y` | `$6ca6` | **nothing** — no vertical handler is modelled (`$fe6e`/`$fbaa` are `discr-75o`) | untested: constant 18 in the fixture |
| `players[0].facing` | `$6ca9`, set at `$f5e2`/`$f7f6` | 1 on entering state 1, 2 on entering state 2 | frames 1–11, then **wrong** — see discr-xfw below |
| `players[0].state_index` | `$6cae`, dispatched at `$f5d0` through `$10e2c` | **read only.** 1 and 2 walk; 5/11/14/16/17/19/20/21/23/24/27/31 are explicit opaque pass-throughs; nothing writes it | frames 1–10 |
| `players[0].grid_cell` | `$6cb0`, `$f836`/`$f838`/`$f842` | `8 + column(x) + (4 if y > 14)`, `column` from the 145-byte `$7bfe` table | frames 1–22; the four independently measured samples are unit-tested |
| `discs[n].world_x` | `disc+$00`, integrated by `+$06` at `$6e44` | `world_x += vel_x`, with a **floor at 0** that sign-flips `vel_x` | frames 1–33 |
| `discs[n].world_y` | `disc+$02` | **nothing** — `vel_y` is 0 on all 84 frames of `dumps/disc_trace` while `world_y` moves, so nothing in evidence integrates it | frames 1–22 |
| `discs[n].world_z` | `disc+$04` | `+1` per frame while active | frames 1–34 only; see discr-0fm |
| `discs[n].vel_x` | `disc+$06`, `$a722`–`$a860` | negated on the floor clamp, and **not otherwise touched** — `disc::steer` is the literal `$a722` rule but nothing calls it | frames 1–33 |
| `discs[n].vel_y` | `disc+$08` | never written | no column in this trace |
| `discs[n].dir_kind` | `disc+$0a`, `$a606 neg.w` | never written during flight; `disc::reflect` is the `neg.w` with no trigger | frames 1–33; it is constant until frame 52, so this proves little |
| `discs[n].damage` | `disc+$16` | caller-supplied; `$a9a0`'s source for it is not recovered | no column in this trace |
| `tiles[n].tile_type` | `tile+$00`, `$a354 clr.w` | cleared when HP reaches 0 | see "Honest limits" — the fixture never changes a tile |
| `tiles[n].hp` | `tile+$02`, `$a31c sub.w ($0016,a5),d6` / `$a34a clr.w d6` / `$a34c` | `saturating_sub(damage).max(0)`, transcribed literally, with the destroyed-cell guard `$a2ec`/`$a2f0` on the caller's side | same |

The three things `disc-core` does that are **modelled rather than mirrored**
are marked as such in the source and repeated here so nobody has to find them:

* the **floor at `world_x == 0` and its coupling to the velocity sign-flip**
  (`disc::step`). `$a606 neg.w` is real; what reaches it is a `bpl` on `d2`
  that is not decoded. The coupling is inferred from golden frames 10→12
  (`world_x` 1 → 0 → 2 while `vel_x` goes −2 → +2, a step of only −1 into the
  clamp). It is right for this fixture and is an inference.
* **no ceiling.** The fixture's two upper turnarounds sit at `world_x` 45 and
  113, different values, and both decay through +1 and 0 rather than clamping.
  Nothing in evidence supports a symmetric upper bound, so none is invented —
  which is exactly why frame 34 diverges.
* **`active` is seeded from `dir_kind != 0`** in `tracecheck`, because the ST
  encoding of an unused slot is unknown (discr-m4x). It happens to select the
  one live disc in this fixture.

## What is waived, and why

`docs/state-schema.md` is the authority; this is the short form. Twelve waived
rows and six excluded, against fifteen compared.

**Waived because `disc-core` cannot produce the row at all:**

* `players[1].*` — **discr-b6x**. `disc-core` takes both players' `Input` from
  its caller and the trace has no `$6c59` column, so `tracecheck` drives p2
  with nothing while the ST walks it. These five rows can never match. They
  were marked `compared` until this bead, which is why every `tracecheck` run
  stopped on frame 1 and reported a waiver as a divergence — the tool
  contradicting its own contract. `--skip-waived` resyncs them from the trace
  instead, and both modes print which one is in force.
* `discs[n].active`, `discs[n].aim` — **discr-m4x**. Modelled, not mirrored:
  `disc+$0a` is a direction/kind word, not a live flag, and there is no
  possession. What triggers a serve is not decoded, so `disc::serve` is an
  explicit call with no trigger of its own.

**Waived as ST behaviour, with the fields they would move left `compared`:**

* **discr-75o** — the semantics of states 5, 11, 14, 19, 20, 21, 23, 24, 27, 31
  and, critically, *what selects the next state*. The handler addresses are
  known; nothing else is. This is what stops the default `--skip-waived` run at
  frame 11.
* **discr-rf9** — states 16 and 17, seen only in one oracle autopilot run,
  never in Hatari. Not notes-grade.
* **discr-217** — what gates the `$a71a`/`$a722` steering block. The rule is
  transcribed exactly in `disc::steer` and **nothing calls it**, because in
  every trace we have the gate is off: in golden frames 1–11 the aim point is
  98 and the disc falls from 21 with `vel_x` pinned at −2, so the rule would
  have incremented on all eleven.
* **discr-tan** — what advances `disc+$02`. `vel_y` is 0 on all 84 frames of
  `dumps/disc_trace` while `world_y` moves, so it is not integrated by `vel_y`
  and `$a758` never fired.
* **discr-5w5** — the collision test: `$a606`'s turn-around condition and how
  `$a31c`'s struck-cell index `d5` is computed. Both undecoded, so
  `disc::reflect` and `disc::impact` are explicit calls the disc loop never
  makes.
* **discr-z8m** — `$6d9a`. `$a314 cmp.w #$0001,$6d9a` applies the disc's `+$16`
  damage a *second* time and `$a32e` tests the same word against 3 for a
  further path. Single application is the base case, not the whole rule.
* **discr-dc0** — the second writer that sets and clears bit 7 of a tile's HP
  word. `$a34c` stores a plain value and cannot produce `(1,5) -> (1,133)`.
* **discr-st8** — round init, scoring, win. `GameState::default()` is all
  zeroes and is deliberately *not* `$aa50`'s round-init state.

**Excluded by scope, not unknown:** disc screen X/Y (`+$0c`/`+$0e`, recomputed
every frame at `$a6b2`/`$a6b6` from world coordinates — comparing them tests
the projection), disc sub-record pointers, the always-zero `tile+$04`, the
animation cursor and countdown, and sound/palette/screen-base I/O.

**Closed, and worth knowing why:** **discr-g38** (what damps `vel_y`) was
resolved by evidence rather than by a new model. The damping is the literal
at-target decay at `$a728`–`$a736`, and the oscillation the bead was filed
against cannot occur because `vel_y` is 0 throughout. A model was removed, not
added. What survived moved to discr-tan.

## The first genuinely unexplained divergence

**There is not one before frame 34, and the two the tool reaches are both
explained.** Saying so plainly is the finding; inventing an unexplained
divergence to have one would be worse than having none.

* Frame 11, `players[0].state_index` 20 vs 0 — **discr-75o**. `disc-core` has
  no state transitions at all; `state_index` is an input to `player::step`,
  never an output. The ST enters state 20 (`$1094a`, the transient at the start
  of a walk) one frame after the joystick byte goes `$00 -> $04`.
* Frame 23, `discs[0].world_y` 82 vs 81 — **discr-tan**. The ST advances
  `disc+$02` 81 → 82 → 83 and stops there; `vel_y` is 0 the whole time, so
  whatever writes it is not the integrator.

Past those, two divergences turned up during this bead that **did not have an
owner**, and both are now filed:

### discr-xfw — `player+$09` is not a 1/2 facing flag

Frame 12, `players[0].facing`, expected **20**, got **0**.

`docs/state-schema.md` and `disc-core` both read `$6ca9` as 1 = left
(`$f5e2`), 2 = right (`$f7f6`). The fixture disagrees: p1's `+$09` takes
`{0, 1, 11, 20, 23}` and p2's takes `{0, 1, 2, 15, 16, 18, 20}` — exactly the
sets their `state_index` takes, one frame behind. `facing[n] == state[n-1]`
holds on **96 of 99** p1 frame pairs; all three exceptions show `+$09` = 11
while the sampled `+$0e` was 0, i.e. a state whose handler ran but which was
not the sampled value.

So `+$09` is most likely the **previous state**, written at handler entry. The
1/2 reading is indistinguishable from it on states 1 and 2 alone, which is all
the notes had — and `disc-core` writing 1 and 2 there is right for the wrong
reason. This is the sort of thing a field table is supposed to catch, and it
only surfaced because `--skip-waived` let the run get past frame 1.

### discr-0fm — `world_z` is not always +1 per frame

Frame 34, `discs[0].world_x`, expected **45**, got **46**, and the same run
shows why. `docs/disc-notes.md` has `disc+$04` as "+1 per frame while in
flight". The fixture has three regimes for slot 0:

    frames  0-34   dir_kind +1   world_z 20 -> 54, +1 per frame     (the note)
    frames 35-51   the whole record FREEZES: x 45, z 54, vel_x 1, 17 frames
    frames 52-63   dir_kind -3   world_z 53 -> 20, -3 per frame
    frame  64      dir_kind flips -3 -> +1 at z 20; +1 per frame resumes

Slot 1 repeats it independently (appears at frame 76 with `dir_kind` −3 and
`world_z` 53 stepping −3 to 17, flips at frame 89). The flip happens at
`world_z` 20 for slot 0 and 17 for slot 1, so it is **not** a constant
threshold. That `|dir_kind|` and the `world_z` step are the same number (3) is
suggestive and is a hypothesis, not a measurement.

The frame-34 divergence itself is the upper turnaround: the ST decelerates
`vel_x` 2 → 1 → 0 → −1 → −2 while `disc-core`, which has no ceiling, keeps
integrating at +2. That belongs to **discr-217** — but note that the `$a722`
rule *as decoded* would not produce it either: the aim point is 98 and the
disc is at 45, so the rule says `vel_x += 1`, while the ST decays it. Whatever
gates `$a71a`, recovering the gate alone will not explain the turnaround.

## What `make core-check` gates on, and why

    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test                                        # 41 tests, default-members
    tracecheck golden.ndjson --skip-waived --min-agree 10

It gates on **the length of the matching prefix**, not on the run being clean.
`tracecheck` prints the divergence in full and exits 0 as long as at least 10
ticks matched; it exits 1 the moment that prefix shrinks.

This was a deliberate choice between two options, and the reasoning is the
same one `reports/oracle-report.md` already applied to the oracle's 275-frame
boundary:

* **Gating on zero divergence** would make `core-check` permanently red. A gate
  that is red by design gets ignored, and a gate that is ignored catches
  nothing — so it is strictly worse than no gate.
* **Exiting 0 unconditionally and gating only on the tool running** would catch
  a crash and nothing else. `disc-core` could stop reproducing the walk
  entirely and the gate would stay green.
* **Gating on `--min-agree 10`** catches the regression (the prefix got
  shorter) and reports the improvement (`raise --min-agree to N`), while the
  divergence itself stays on screen in full every single run. Nobody can forget
  it is there.

`--min-agree` is the same flag name and the same idiom as
`scripts/oracle_diff.py`, deliberately, so the two gates read alike. **10 is
recorded in `Makefile` as `TRACE_MIN_AGREE`** with the reason next to it. Raise
it when the prefix grows.

`make tracecheck` is the ungated view: same run, no `--min-agree`, exits 1.

Two things `core-check` deliberately does **not** do. It does not build
`disc-app` — that crate stays outside the workspace `default-members` so a
missing `libGL`/`libX11`/`libasound` can never break the gate — and it does not
touch `oracle/`, `scripts/` or `seeds/`, so it needs no C compiler, no
emulator, and no disk image. **It runs from a clean clone with only a Rust
toolchain**, because `tests/fixtures/golden.ndjson` is committed. There is no
new system dependency; `README.md` says so next to the ones `make oracle-check`
does need.

## The coordination account

Eight agents, seven worktrees plus the orchestrator on `master`, one shared
`.pact/` log.

    $ pact audit
    101 events from 8 agent(s)
      context  commit-policy=per-task  scheduler=waves-then-free-run
               topology-expectation=worktrees
      span     2026-08-23T07:12:37Z  ->  2026-08-23T07:49:26Z
      kinds    acquired 50, refused 1, released 45, watched 5
      conten   1 refusal(s), 0.0 per successful claim;
               1 path(s) refused and never acquired (1 refusal(s) abandoned)
      watch    5 active; 0 diff(s) delivered

    hold time over 45 completed hold(s): median 5m38s, p90 7m21s, max 7m21s

    most contended paths
      Cargo.lock                      3 hold(s) by 3 agent(s)
      crates/disc-tools/src/main.rs   3 hold(s) by 3 agent(s)
      .beads                          5 hold(s) by 2 agent(s)
      crates/disc-core/src/disc.rs    4 hold(s) by 2 agent(s)

| check | result |
|---|---|
| `--check topology --expect worktrees --allow-main orchestrator` | **clean** — every context-stamped event matches; 24 events excused from the main checkout by `--allow-main`, all of them the orchestrator's |
| `--check double-win` | **clean** — no two agents ever held one path at once |
| `--check stale-holds` | **clean** — no hold ran past its own TTL without a renew |
| `--check claim-lease-divergence` | **DID NOT RUN** — see below |

### The gap: `claim-lease-divergence` could not run

    claim-lease-divergence: scanned 101 event(s)
      no beads data (no assignee history in .beads/interactions.jsonl)
      — claim-lease-divergence could not run.

bd's audit sidecar was not recording for most of this run; it was enabled late,
and bd records from that point rather than retroactively. So the one check that
asks whether the agent who *claimed* a bead is the agent who *held* the lease
has no data for wave 1, wave 2, or most of wave 3.

**This is a gap, not a pass.** Three clean checks and one that could not run is
not four clean checks. Nothing here says the fleet diverged; nothing here says
it did not. The fix for next time is one environment variable —
`BD_AUDIT_ENABLED=1` in the shell every agent runs `bd` in, or
`bd config set audit.enabled true` to persist it — set at fleet spawn, not at
the end.

    $ pact audit --json | recount testify --repo .
    recount testify — /home/chussenot/Documents/discr
      source   stdin
      input    pact audit summary (no --check)
      scanned  1 session dir(s) · 10 session(s) · window ±300s each side
      findings 0 · testified 0 · untestified 0 · passed through 0
      nothing to testify: the audit found nothing. A clean audit is not a failure.

`recount` cross-checks audit findings against the harness session transcripts.
With no findings in the summary there is nothing for it to testify to — which
is the honest output, and is also why it cannot fill the
`claim-lease-divergence` hole: it corroborates findings, it does not
manufacture the beads data that check needs.

### The one real finding: silent contention on `Cargo.lock`

The single refusal in the whole run, in full:

    07:29:13  tracecheck  acquired  Cargo.lock
              "discr-3g6: adding clap+serde_json to disc-tools (lockfile churn only)"
    07:31:53  app         REFUSED   Cargo.lock
              held by tracecheck ... 42m21s left on their hold, taken 159s ago
    07:33:08  app         msg -> orchestrator  (escalated inside its merge request)
    07:34:05  tracecheck  released  Cargo.lock          <- 2m12s after the refusal
    07:35:26  orchestrator acquired Cargo.lock, released it 5s later
              "regenerating the lockfile for disc-app after merging agent/app"

`app`'s judgement was right. It did not `--steal` a live hold, it did not
block, it dropped a *derived* file from its commit, reverted its worktree copy,
and told the orchestrator exactly what to run after the merge (`cargo build -p
disc-app`, then commit the lockfile). The orchestrator did precisely that in
`265b4a2`. Nothing was lost.

What went wrong is that **`app` never learned it could have proceeded.**
`tracecheck` released the path 2 minutes 12 seconds after the refusal, with no
message and — because nobody watched the path — no diff delivery either. All
five `pact watch` subscriptions in this run were on `docs/state-schema.md`; not
one was on a source or build file. So the release was a silent event, and 57
seconds *before* it happened `app` had already written off the lockfile.

Two mechanisms existed and neither was used:

* `pact lease acquire Cargo.lock --wait 5m` would have returned at 07:34:05,
  inside the same turn, with the lease in hand.
* `pact watch add Cargo.lock` would have delivered `tracecheck`'s diff on
  release, automatically.

The reason both were skipped is visible in the refusal text: it reports
**`42m21s left on their hold`**, and `app`'s own message says it declined to
"block 42 minutes on a derived file". But `42m21s` is the TTL remaining — an
upper bound, and against a measured p90 hold of 7m21s in this very run, a wildly
pessimistic one. The actual wait was **2m12s**. An agent that reads the
remaining TTL as an estimate of the wait will always decline to wait, which is
the behaviour to design out: `--wait` costs nothing when the holder is nearly
done, and the refusal message is the one place an agent will read a number.

Everything else in the run is clean. Every merge went through the orchestrator
because no worktree can check out `master` while the main checkout holds it;
five agents said so independently and none tried to force it.

## Honest limits

* **The gate is a 10-tick prefix on one fixture.** `--min-agree 10` is what
  `make core-check` enforces. Everything past frame 10 in the table above needs
  `--resync`, which is a measuring tool, not a claim: a resynced row is one
  `disc-core` was *given*, not one it produced.
* **The tile module is entirely untested by trace comparison.** All 17 cells
  are constant across all 100 frames of the golden fixture, so the 34 tile rows
  match trivially and prove nothing about `tile::damage`. What covers it is
  `disc-core`'s own unit tests against the three tier-1 transitions in
  `docs/disc-notes.md` — and `disc::step` never calls `tile::damage` at all,
  because `d5` is undecoded (discr-5w5). **A trace in which a disc destroys a
  cell is the highest-value fixture this project does not have.**
* **`players[0].world_y` is equally untested.** It is 18 for the whole fixture,
  and `disc-core` models no vertical movement.
* **The serve path is unexercised.** Slot 1 goes live at frame 76 on the ST;
  `disc-core` never activates a slot, because `disc::serve` has no trigger
  (discr-m4x). Under `--resync discs` that is invisible, which is exactly why
  the 99/99 row above is a warning and not a result.
* **`--skip-waived` is not the default, on purpose.** It resyncs state from the
  reference, which is the standard way to make a bad model look good. The
  default still compares everything and still stops on frame 1; you have to ask
  for the measurement, and the header says which mode produced the number.
* **One fixture, 100 frames, one seed.** `tracecheck` takes any trace; only one
  has been generated. `scripts/regen_golden.sh` needs `seeds/`, which is
  gitignored and absent from a clean clone by design.
* **The prefix numbers are not additive.** "33 ticks matched" with three rows
  resynced does not mean 33 ticks of `disc-core` being right; it means 33 ticks
  of everything *else* being right.
