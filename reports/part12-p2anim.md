# Part 12: player 2's own animation sequences, and the feed that stays (discr-rxx.2)

Full narrative: `docs/disc-notes.md`, "Player 2's own animation tables, and why
its feed stays (Part 12, discr-rxx.2)". This report is the catalogue, the
measured numbers, and the landing state.

## The task

`discr-rxx.1` (Part 12, `reports/part12-anim.md`) decoded the animation cell
format and reconstructed `anim_cursor`/`x_delta`/`hit_box` for player 1, but
left player 2's three copies fed: its own idle/walk sequences were not fully
catalogued, and `disc::THROW_STATES`' release gate reads player 2's
`anim_cursor` directly, so a wrong reconstruction there would desync the serve
and corrupt the disc simulation for both players. This bead catalogues every
sequence player 2's state handlers enter and attempts the same reconstruction
for player 2 -- succeeding for four of six fixtures, and finding a real,
precisely-located remaining gap in the other two.

## The catalogue

Six new tables, all read out of `discram.bin` at the pointer each cell's own
first four bytes hold, the same method `discr-rxx.1` established. Base
addresses and cell counts confirmed against the raw ROM bytes directly (never
via Ghidra's own disassembler/decompiler for the table DATA -- only for the
surrounding CODE that dispatches into them).

| state | sequence | base | cells | hold(s) | dispatch site(s) |
|---|---|---|---|---|---|
| 1 (walk left) | `ANIM_P2_WALK_LEFT` | `$449e` | 6 | 4,4,4,4,4,4 | `$ad12` (skip-turn), `$c284` (turn's own ending) |
| 2 (walk right) | `ANIM_P2_WALK_RIGHT` | `$434a` | 6 | 4,4,4,4,4,4 | `$ad66` (skip-turn), `$c284` (turn's own ending) |
| 20 (turn, from a left walk) | `ANIM_P2_TURN_LEFT` | `$4992` | 1 | 4 | `$acf6` (idle entry), walk-left's own exit |
| 20 (turn, from a right walk) | `ANIM_P2_TURN_RIGHT` | `$4988` | 1 | 4 | `$ad4a` (idle entry), walk-right's own exit |
| 5 (hold UP) | `ANIM_P2_UP` | `$4522` | 6 | 4,4,4,4,4,4 | `$aca6` (idle, direct dispatch) |
| 6 (hold DOWN) | `ANIM_P2_DOWN` | `$459a` | 6 | 4,4,4,4,4,4 | `$acc4` (idle, direct dispatch) |

Two more tables were found but are **not wired up**, because nothing in the
required fixture windows reaches them and wiring them without a fixture to
measure against would be exactly the guess the house rules forbid:

* **`$463e`** (4 cells, hold 6 each) and **`$465a`** (2 cells, hold 6 each) --
  the two checkpoints `state 19`'s own ongoing handler (`$c1ec`) commits on.
  What DISPATCHES player 2 into state 19 is a third branch inside the
  anticipation cascade (`$cc98`-`$ccb8`, installing a new hook value `$a88a`
  this crate's `SteerHook` enum has no variant for) that `farbank.ndjson`
  reaches at frame 48 -- past that fixture's own established boundary (frame
  34, an unrelated `tiles[7].hp` gap, `discr-dc0`), so it is undecoded and
  unmeasured here. `p2_hit_test`'s own comment ("`$cad0` is state 19's; no
  fixture reaches state 19") is now half-true: one does, just past where
  anything is required to hold.
* **`$439a`** (8 cells, hold 3 each, `[-9,11,-20,14]` .. `[-2,11,-17,15]`) --
  a "step over a hole" sequence player 2's own walk-right handler (`$b1d8`)
  can substitute for the normal walk table under a probe against the column
  tables. Never observed live (no fixture's floor has a hole under this
  player's path during a walk), so the substitution condition itself is
  undecoded.

Twenty existing tables (all of player 1's, and player 2's throw/smash/
intercept/reach/struck/idle sequences, `discr-rxx.1` and earlier work) are
unchanged.

## Real bugs found while measuring, fixed for both players

None of these are player-2-specific hacks -- all five are shared code, fixed
because the ASM shows the fix and player 2's data is what caught the gap
(the same values these functions handle for player 1 happen not to
distinguish the wrong behaviour from the right one, an ambiguity player 2's
own numbers resolve):

1. **`anticipate()`'s two dispatch arms never ticked.** `enter_anim` alone
   only moves the cursor; `$f1ca`'s copy is `anim_tick`'s job. Entering
   `STATE_INTERCEPT`/`STATE_REACH` without a follow-up tick left `hit_box`/
   `x_delta` frozen at the pre-dispatch state for one tick. Caught: `golden.
   ndjson` frame 22.
2. **`intercept()`'s ongoing handler (state 18) returned early without
   ticking on its three non-committing gates.** ST `$c1b0`/`$c1b8`/`$c1cc`
   all `bne`/`beq $ac40` -- the shared tail runs regardless of which gate
   exits. Missing this froze the intercept pose's own animation for its
   entire duration. Caught: `golden.ndjson`'s 18-frame state-18 window.
   (These two bugs together were briefly a double-tick once both were
   half-fixed -- `anticipate` does NOT reach `$ac40` itself, per its own
   `rts` at `$cc3e`; the *subsequent, same-tick* call into `intercept`'s own
   handler is what performs the copy, since `GameState::update` runs the disc
   loop before the per-player dispatch. Confirmed by reverting the
   `anticipate`-side tick once `intercept`'s fix was in.)
3. **`p2_throw_choice` returned early without ticking on two of its four
   gates.** ST `$ad82` (fire alone) and `$ad92` (already at the disc cap)
   both reach `$ac40`; only `$ad8a` (fire+down, a separate undecoded path)
   and the mid-walk-stamp check do not. Caught: `tile_damage.ndjson` frame
   60 (Part 10j's own "AI holds `$80`" frame).
4. **`turn()`'s idle-landing arm used the wrong "no second tick" shape.**
   `discr-rxx.1` modelled it on `struck_down`'s verified stale-reuse ending,
   but player 1's own `ANIM_TURN` hit box happens to equal `ANIM_P1_IDLE`
   cell 0's, so player 1's data cannot tell a stale reuse from a fresh tick
   apart. Player 2's asymmetric turn tables can: `ANIM_P2_TURN_RIGHT`'s hit
   box (`[-4,11,-20,18]`) differs from `ANIM_P2_IDLE` cell 0's (`[-3,...]`),
   and `golden.ndjson` frame 89 shows the FRESH value on the very tick
   `state_index` changes. Fixed to match the walk-landing arm's shape (both
   are one fresh dispatch now, not two asymmetric endings) -- `struck_down`/
   `struck_up`'s own endings are unaffected and still genuinely stale (their
   own differing values already proved that unambiguously).
5. **`walk()` never reloaded its own table when a long walk outlasted it.**
   `anim_tick`'s `Ended` return was ignored, so `anim_cell` ran past the
   table's own length. ST: a walk that exhausts its table wraps to its own
   cell 0, fresh (not `idle_tick`'s stale reload -- landing specifically on
   idle is `$f202`'s own generic fallback; a self-wrap is its own dispatch).
   Caught: `p1_walk.ndjson` frame 107, the first fixture with a walk long
   enough to exhaust `ANIM_P2_WALK_RIGHT`'s six cells.
6. **`disc::THROW_STATES`' release gate read `anim_cursor` at the wrong
   point in the tick** (`crates/disc-core/src/lib.rs`). ST `$c06e`
   (state 15's own handler) compares the cursor as the PREVIOUS frame's tail
   left it, before running its OWN copy for this frame; the gate in
   `GameState::update` ran AFTER `player::step` (where the tail's copy now
   genuinely happens), using this frame's fresh value instead -- firing the
   serve one frame early. Fixed by snapshotting `players[1].anim_cursor`
   before the per-player step loop and gating on that snapshot. This was
   invisible while player 2's cursor was fed (`feed_disc_inputs` itself feeds
   from the PREVIOUS frame, reproducing the same lag by a different route);
   discr-rxx.2 surfaced it the moment player 2's reconstruction ran live.
   Caught: `golden.ndjson` frame 51 (`discs[0].world_x` off by exactly the
   thrower's own step magnitude).

## Measured agreement (player 2, `anim_cursor`/`x_delta`/`hit_box`, both
players unfed)

Measured with `crates/disc-core/tests/anim_measure.rs` temporarily edited to
also stop feeding player 2 and to mark its three fields unwaived (that
version is not what ships -- see "Landing" below). `n/total` counts every
tick up to the first non-waived divergence, same convention as `discr-rxx.1`.

| fixture | reconstructed | player 2 agreement |
|---|---|---|
| `golden.ndjson` | 99/99 (whole fixture) | 3/3 fields, 100% |
| `tile_damage.ndjson` | 214/214 (whole fixture) | 3/3 fields, 100% |
| `handover.ndjson` | 177 (unrelated `discs[2].world_x` gap, off by 1) | 100% to there -- up from 53 before this bead's other fixes |
| `farbank.ndjson` | 35 (unrelated `tiles[7].hp` gap, `discr-dc0`) | 100% to there |
| `p1_walk.ndjson` | 135 of 275 | **diverges**: frame 135, `ANIM_P2_WALK_RIGHT` cell 1->2 one tick early |
| `bonus.ndjson` | 144 | **diverges**: frame 144, `ANIM_P2_WALK_RIGHT` cell 0->1 one tick early |

Both remaining divergences are the SAME root cause: a long walk-right run's
per-cell hold empties one tick sooner in this crate's `walk()` than the ST
shows. Direct disassembly of player 2's own walk-right handler (`$b1d8`
onward, not assumed from player 1's shape) explains why it is not a simple
off-by-one in `anim_tick`: the real handler runs an entire extra mechanic
between the step and the tail --

```
$b1e0  fire+down -> $af50 (separate, undecoded)
$b200  probe 24 units ahead against the column table ($7bfe/$7596)
$b246  cmpi.b #8,(a0) ; bne $b3b0   -- extra input bits -> a different path
$b24e  addq.w #3,$6d22               -- the step itself
$b252  a SECOND probe, at the new x, against the same tables
$b282  bne $b2da   -- near the right wall -> a parabola calc (bsr $aae8)
$b286  the probed cell type against $c/$10 -> $b3d4 (also undecoded)
$b296  a further bank test -> $b3d4
$b2a0  cmpi.l #$434a,$6d5a -- STILL at cell 0? -> the hole-substitution table
$b2c2  (reached once none of the above fire) -- another table/state load
```

None of this is present in player 1's own (fully modelled) walk. Golden's and
tile_damage's walk-right runs are short (2-3 ticks) and never reach a second
cell, so they never exercise whatever in this handler perturbs the timing;
`p1_walk` and `bonus` have long enough runs to hit it. Which exact branch
above is responsible, and what it changes about hold timing, is not decoded.
`// UNKNOWN: see bd discr-75o` -- filed as a follow-up, `discr-tun`.

Player 1: reran unaffected. `cargo test -p disc-core --test anim_measure`
(as shipped, player 2 fed) still reports `golden`/`tile_damage`/`p1_walk`
clean and whole, `handover`/`bonus`/`farbank` to their own established
boundaries -- identical to `discr-rxx.1`'s own numbers, confirming none of
the five shared-code fixes above regressed player 1.

## Landing: the feed stays, house rules applied literally

Two of six fixtures do not reach 100% for the three target fields, so per the
house rule ("100% agreement per fixture or the feed stays") **the feed is not
retired**. Concretely:

* `crates/disc-tools/src/main.rs`'s `feed_disc_inputs` is **unchanged**:
  still feeds `anim_cursor`/`x_delta`/`hit_box` for player 2 every tick.
* `crates/disc-core/src/player.rs`'s `step` keeps its snapshot/restore
  wrapper around player 2's three fields, exactly as `discr-rxx.1` left it --
  now with an updated doc comment naming the real remaining gap instead of
  the (now-resolved) cataloguing gap. Player 1 passes through untouched, as
  before.
* `docs/state-schema.md` is **unchanged**: player 2's three fields stay in
  the existing waived row.
* `crates/disc-core/tests/anim_measure.rs` keeps feeding player 2 (its
  original, `discr-rxx.1` design: this file can only ever measure what
  `step`'s own guard does not block, which is player 1). Its module doc
  records this bead's real player-2 numbers above and points here for the
  method; the numbers themselves came from a temporary edit (not shipped)
  that also disabled the guard, purely to observe the reconstruction in
  isolation.
* All ten required gates hold at their previously-established numbers (none
  shrunk) -- verified against the real `tracecheck`, not just this harness:

```
golden.ndjson       --skip-waived --min-agree 99   -> OK, 99 matched
tile_damage.ndjson  --skip-waived --min-agree 214  -> OK, 214 matched
golden.ndjson                     --min-agree 99   -> OK, 99 matched
tile_damage.ndjson                --min-agree 214  -> OK, 214 matched
p1_walk.ndjson                    --min-agree 274  -> OK, 274 matched
handover.ndjson                   --min-agree 21   -> PASS at 21 (unchanged)
handover.ndjson     --skip-waived --min-agree 222  -> PASS at 222 (unchanged)
bonus.ndjson        --skip-waived --min-agree 150  -> PASS at 150 (unchanged)
bonus.ndjson                      --min-agree 22   -> PASS at 22 (unchanged)
farbank.ndjson                    --min-agree 34   -> PASS at 34 (unchanged)
```

`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
all clean. `cargo clippy -p disc-app --all-targets -- -D warnings && cargo
test -p disc-app` also clean.

## Files

* `crates/disc-core/src/player.rs` -- six new `FRAMES_P2_*` tables and their
  `anims!` entries (`WALK_LEFT`/`WALK_RIGHT`/`TURN_LEFT`/`TURN_RIGHT`/`UP`/
  `DOWN`), `walk_anim`/`turn_anim`/`anim_cell_for_cursor` helpers, `walk`/
  `enter_turn`/`idle`/`turn` extended for player 2's own dispatch sites and
  fixed for the self-wrap gap, `intercept`/`anticipate`/`p2_throw_choice`
  fixed for their missing-tick gaps, two new states (`STATE_UP`/
  `STATE_DOWN`) and their `vertical` handler (entry + tick + revert-to-idle
  only -- the floor clamp and the further transition into states 24/25 are
  not modelled, matching this file's existing tier-1 precedent), `step`'s
  doc comment updated to name the real remaining gap.
* `crates/disc-core/src/lib.rs` -- `THROW_STATES`' release-gate ordering fix
  (snapshot the pre-step cursor).
* `crates/disc-core/tests/anim_measure.rs` -- `seed_anim_cell` generalised
  to `anim_cell_for_cursor` (checks every catalogued table, not just the
  seeded player's own idle one -- needed because player 2 routinely starts a
  fixture mid-walk or mid-throw); module doc records this bead's findings.
* `docs/disc-notes.md` -- Part 12 section appended.

## bd

`discr-rxx.2` stays `in_progress`, not closed: the catalogue and five real
bug fixes land, but the feed is not retired (two of six fixtures short of
100%, house rules applied literally). `discr-tun` (child of the `discr-rxx`
epic) is filed for the remaining walk-right/walk-left mechanic (the
hole-substitution and wall-approach-parabola paths inside `$b1d8`/its
left-walk mirror), with the exact frames (`p1_walk.ndjson` 107 and 135,
`bonus.ndjson` 144) and ASM addresses this report names as its starting
point.
