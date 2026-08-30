# Part 12 — the ten tier-1 player states, decoded

`bd discr-75o`'s brief: ten player states (5, 11, 14, 19, 20, 21, 23, 24, 27,
31) whose handler ADDRESSES were tier-1 known (`docs/disc-notes.md`, Part 8)
but whose behaviour was not — `disc-core` treated all ten as opaque
pass-through. Four had already been modelled by other work before this part
started (11, 20, 23 in Part 10b/10d/11i; 27 in Part 10i); this part decoded
the remaining six (5, 14, 19, 21, 24, 31), implemented the two that turned
out to be fully determined by fields `disc-core` already carries (19, 21),
and found + fixed a real bug in code that shipped before this part
(`intercept()`'s state-18 gate values were player 2's, hardcoded, for both
players).

Every number below was produced by a command in this file.

    mkdir -p /tmp/ghidra-states && cp -r tmp/ghidra_proj /tmp/ghidra-states/proj
    GHIDRA_HOME=tmp/ghidra_12.1.3_PUBLIC GHIDRA_PROJ=/tmp/ghidra-states/proj \
      scripts/ghidra/q.sh dis 108f4 80 dis 109aa 60      # states 19, 21
    GHIDRA_HOME=tmp/ghidra_12.1.3_PUBLIC GHIDRA_PROJ=/tmp/ghidra-states/proj \
      scripts/ghidra/q.sh dis fb6e 60 dis 106b2 60        # states 5, 14
    GHIDRA_HOME=tmp/ghidra_12.1.3_PUBLIC GHIDRA_PROJ=/tmp/ghidra-states/proj \
      scripts/ghidra/q.sh dis 10ac4 60 dis 10dda 70       # states 24, 31
    GHIDRA_HOME=tmp/ghidra_12.1.3_PUBLIC GHIDRA_PROJ=/tmp/ghidra-states/proj \
      scripts/ghidra/q.sh dis 1089e 20 dis 10a72 30       # state 18 (the intercept fix), state 23 cross-check
    python3 scripts/collect.py --scenario /tmp/state5_hunt.yaml --mode training
      # watches $6cae over a 90-frame Up hold: 3 hits, exactly the predicted
      # PCs -- see "Live cross-validation" below
    cargo test -p disc-core
    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --skip-waived --min-agree 99
    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/tile_damage.ndjson --skip-waived --min-agree 214
    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/tile_damage.ndjson --min-agree 214
    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson --min-agree 274

The `$10e2c` table itself was read directly (32 raw longwords, not
individually xref'd) to confirm every one of the ten addresses against Part
8's tier-1 list before decoding a single handler; all ten matched.

## Per-state summary

Full transcriptions, the reasoning behind each read, and the status table are
in `docs/disc-notes.md` under "The ten tier-1 states, decoded (Part 12)" —
this report gives the one-line semantics and the evidence grade for each.

| state | address | one-line semantics | decoded | implemented | cross-validated |
|---|---|---|---|---|---|
| 5 | `$fb6e` | Up from idle: rise (`world_y += 1`/frame), clamp at 25 -> state 24; fire (+ a disc, or +Down) bails to the undecoded `$f306`; Left/Right bails to the undecoded `$aae8` parabola calc | yes | no (needs `$f306`/`$aae8`) | yes -- live, entry PC |
| 11 | `$10554` | knocked down, sinks one row per animation cell, floor at `world_y` 2 | yes (Part 10b) | yes (Part 10b) | yes (Part 11i, `p1_walk` frame 256) |
| 14 | `$106b2` | Right+Fire windup; completes to `world_x += 2` + a sound cue + state 31 (with the `$1334a` hook installed), or idle if Right released / at `WALK_X_MAX` | yes | no (state 31's semantics are out of scope for a field-mutation model) | no |
| 19 | `$108f4` | player 1's third catch/commit state (alongside 18, 27); gates on `anim_cursor` + fire + !Down + a disc available; commits `world_x += 6` -> state 16 | yes | **yes** | no (nothing drives player 1 into it yet — same wall the existing docs already name for its player-2 mirror) |
| 20 | `$1094a` | the turn transient, redirects through `pending_state` | yes (Part 10b) | yes (Part 10b) | yes (`golden.ndjson` f10-14/f28-32) |
| 21 | `$109aa` | unconditional `world_x -= 3`/frame, no gate, no clamp; sequence end -> idle directly (not via `pending_state`) | yes | **yes** | no (never reached in any fixture or by the Hatari probe) |
| 23 | `$10a72` | out of energy; terminal, tests the terminator before copying | yes (Part 10b/10d) | yes (Part 10b/10d) | yes (`golden.ndjson` frame 97) |
| 24 | `$10ac4` | the hover atop state 5's rise; completes to an *unclamped* `world_y += 1` + the same sound cue + state 31, or idle if Up released | yes | no (same state-31 wall as 14) | yes -- live, transition PC |
| 27 | `$10c8a` | reaching for a disc without moving, runs its sequence out via `run_out` | yes (Part 10i) | yes (Part 10i) | yes (`tile_damage.ndjson` frame 111) |
| 31 | `$10dda` | reached from 14 or 24; unconditionally sets player 2's `$6d2d` and player 1's `$6cac` every call -- an immediate round reset | yes | no (no round-reset event exists to hang this on) | **yes — live, and the finding itself: the reset was not visible from disassembly alone** |

Bold cells are this part's own work; the rest is cited for completeness so
the table covers all ten states the bead names.

## The `intercept()` fix (retract-grade)

Decoding state 19 meant reading state 18's real player-1 handler for
comparison (`$1089e`, four bytes past the state-17 stub at `$1089a`, which is
why nothing had gone looking for it). `intercept()`, which `disc-core` already
shipped for state 18, used `INTERCEPT_RELEASE_A`/`B` (`$4624`/`$4634`) for
*both* players — but those are player 2's checkpoints, read off `$c19c`/`$c1a8`.
Player 1's own pair, read off `$108a4`/`$108b0`, is `$2c10`/`$2c20` — a
different sequence table entirely, since each player's animation data lives at
its own addresses.

**Currently inert, not currently wrong-in-practice**: the only fixture that
puts player 1 in state 18 is `p1_walk.ndjson`, whose window ends three frames
after entry (272-274) with `discs_out == disc_cap` throughout — the
disc-availability gate alone blocks commit regardless of which checkpoint
constants are used, and the recorded `anim_cursor` (`11262` = `$2bfe`) never
reaches either checkpoint pair within the window. All five gates below still
pass at their required counts with the fix applied — see the scoreboard.

`intercept()` now takes `who: PlayerId` and reads the correct pair through a
new `intercept_release(who)`; `INTERCEPT_RELEASE_A`/`B` keep their names and
values (player 2's), documented as such.

## Live cross-validation: `dumps/state5_hunt`

Existing fixtures never touch any of the four undecoded states (5, 14, 24,
31) or the two newly-implemented ones (19, 21), so a Hatari scenario was used
instead of a `--dump` diff. Scenario (`mode: training`, watching `$6cae` over
a 90-frame Up hold):

```yaml
name: state5_hunt
mode: training
settle: 60
steps:
  - {wait: 100}
  - {dump: a}
  - {watch: "$6cae", width: "b"}
  - {hold: Up, frames: 90}
  - {wait: 10}
  - {unwatch: true}
  - {dump: b}
```

Result:

```
[watch] $6cae: 3 hit(s) from PC $f23e, $fbda, $10b50
```

Three predicted transitions, three hits, in order:

* `$f23e` — the `bra $f1c4` right after `$f238`'s `move.b #$5,$6cae`: idle
  entering state 5 on Up (`$f222` gate).
* `$fbda` — the `bra $f1c4` right after `$fbd4`'s `move.b #$18,$6cae`: state
  5's rise hitting its clamp and handing off to state 24.
* `$10b50` — the hook-install instruction right after `$10b4a`'s `move.b
  #$1f,$6cae`: state 24's hover completing and handing off to state 31.

(Hatari's change-watch reports the PC reached *after* the write, one
instruction past each `move.b` — consistent across all three hits, and
consistent with how the same tool was used in Part 4's `watch_player_xy`.)

A dump taken 10 frames later (`dumps/state5_hunt/b.bin`, VBL 17922 against
the baseline dump's VBL 16684) shows the whole match state reset: player 1's
`$6cae` = 0, `$6ca6` (world_y) = 18 — the starting value, not the clamped 25
— `$6cac` = 0, and player 2's `$6d2d` = 0. All four were non-zero or moved at
some point during the 90-frame hold (per the disassembly) and are back to
their pre-jump values by the second dump. **Entering state 31 forces an
immediate round reset** — this is the one finding in this part that the
disassembly alone would not have shown with confidence: `st.b $6d2d` /
`st.b $6cac` read, in isolation, like a stun/lockout flag for an aerial
attack; watching the match state actually unwind within ten frames is what
confirms it is a reset instead.

## Gates

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```
48 `disc-core` tests pass (45 before this part + 3 new:
`state19_commits_on_its_release_frame_with_fire_and_a_disc`,
`state19_is_a_pass_through_off_its_release_frame`,
`state21_slides_unconditionally_and_ends_at_idle`), plus 11 `tracecheck` unit
tests.

```
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --skip-waived --min-agree 99
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/tile_damage.ndjson --skip-waived --min-agree 214
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/tile_damage.ndjson --min-agree 214
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson --min-agree 274
```
All five: `OK`, matched at exactly the required tick count — unchanged from
before this part, since none of the states this part touches (18's fix, 19,
21) is reached within any fixture's currently-recorded window. The two new
implementations (19, 21) and the `intercept()` fix are proven by unit test
and, for the four states left opaque, by the live Hatari probe above — not by
a shift in these numbers.

## What is still open

* `$f306` — the fire+Down / fire+available-disc branch state 5 shares with
  idle's own `$f21e bmi`. An aerial throw commit, undecoded. Filed under the
  same wall `docs/disc-notes.md` already names for idle (`bd discr-b6x`).
* `$aae8` — the column/parabola calculation state 5's Left/Right branch runs
  while rising. A new wall this part surfaced; not chased.
* `$1334a` — the function-pointer hook both state 14 and state 24 install
  into `player+$2e` (`$6cce`) before handing off to state 31. Not disassembled
  past its first few instructions (a double-buffer/sprite-table swap, of a
  shape unrelated to player state — likely a mis-resolved address rather than
  the hook's real body; worth a second look with a proper function boundary).
* What selects state 19 over 18 or 27 (`anticipate()`'s X ladder only ever
  picks the latter two) — matches the existing honest note on state 19's
  player-2 mirror in `p2_hit_test`.
* What enters states 21 and 22 at all — no xref, no fixture, no Hatari hit.
* State 22 (`$10a0e`), state 31's terminal-return neighbours, and every other
  index outside this bead's ten remain unattested, same as before this part.

All are `// UNKNOWN: see bd discr-75o` in `player.rs`, except the two ST
subroutines (`$f306`, `$aae8`) which are cross-referenced to `bd discr-b6x`
where the existing docs already point there from idle's own code.
