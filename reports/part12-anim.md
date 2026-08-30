# Part 12: the animation table format, and killing three feeds (discr-rxx.1)

Full narrative: `docs/disc-notes.md`, "The animation cell format, and killing
three feeds (Part 12, discr-rxx.1)". This report is the format spec, the
measured numbers, and the landing state.

## The task

`docs/state-schema.md` waived three fed inputs under `discr-75o`:
`player+$3a` (`anim_cursor`), `player+$1a` (`x_delta`), `player+$1c`..`+$22`
(`hit_box`) -- all three copied out of the animation engine's sequence
tables, which Part 10h decoded the shape of (six-byte cells: a four-byte
frame pointer, a two-byte hold, ending in a zero longword) but not the
content of. This bead decodes the frame pointer's own target, reconstructs
all three fields in `disc-core`, and measures the reconstruction against
every committed fixture.

## The cell format

A cell's own first four bytes are the frame pointer (established); it points
at a 20-byte "frame block" `$f1ca` copies out every frame:

```
frame block + $00  10 bytes of sprite/graphics data this crate does not carry
frame block + $0a  word    x_delta        (player+$1a)
frame block + $0c  word    hit_box[0]     (player+$1c)
frame block + $0e  word    hit_box[1]     (player+$1e)
frame block + $10  word    hit_box[2]     (player+$20)
frame block + $12  word    hit_box[3]     (player+$22)
```

**Confirmed three independent ways:**

1. Every cell of all 20 sequences `player.rs` already tracks, extracted
   directly from `discram.bin` as raw `(pointer, hold)` pairs, cross-checked
   against the `holds` arrays the crate already carries (hand-transcribed
   from disassembly in Part 10h, before this bead touched anything): 20/20
   match exactly. The cell format was derived purely from the frame block's
   own layout; this check is fully independent.
2. Walking the same reader backward from player 2's frame-0 cursor
   (`$44b6`, golden.ndjson -- inside no table Part 10h named) lands on a
   sixth, never-catalogued table at `$449e`: six cells, hold 4 each, clean
   terminator at `$44c2`. The reader was not tuned to find this.
3. The one measured value Part 10d named -- player 1's standing hit box,
   `[-3, 11, -20, 18]` -- lands at cell 0 of both idle tables under this
   offset scheme; the knocked-down first frame, `[-4, 11, -19, 16]`, lands at
   cell 0 of both struck-down tables.

## The cursor rule

`anim_cursor` (`player+$3a`) is `anim_base + 6*anim_cell`, recomputed inside
`anim_tick` on every advance -- not separate state to track.

Two timing rules, both measured against distinguishable evidence (not
inferred from one ambiguous case):

* **A natural mid-sequence advance is one tick behind its own cursor.** The
  copy and the cursor advance are the same tail invocation, but the copy
  runs first, using the cell about to be superseded (`golden.ndjson` frame
  7: cursor already at cell 5, `hit_box` still cell 4's).
* **A generic ending (nowhere more specific to hand off to) reuses that same
  copy; a fresh dispatch into a NEW sequence gets a second, immediate tick.**
  Struck-down ending into idle (frame 71): cursor already reads idle's base,
  `hit_box` stays stale for one more tick. The turn transient ending into a
  walk (frame 14): `hit_box` is ALREADY the walk's fresh cell-0 data on the
  same tick. The hold count carries the same asymmetry: a fallback that does
  NOT re-tick still consumes one unit of the fresh hold immediately (turn's
  landing-on-idle arm only); every other ending shows the full hold.

## Player 1: measured agreement

`crates/disc-core/tests/anim_measure.rs` -- a standalone harness copying (not
calling into) `disc-tools`'s seed/feed/passes logic, written before `main.rs`
was free to edit -- drives all six fixtures with `anim_cursor`/`x_delta`/
`hit_box` UNFED for player 1:

| fixture | reconstructed | player 1 agreement |
|---|---|---|
| `golden.ndjson` | 99/99 (whole fixture) | 3/3 fields, 100% |
| `tile_damage.ndjson` | 214/214 (whole fixture) | 3/3 fields, 100% |
| `p1_walk.ndjson` | 274/274 (whole fixture) | 3/3 fields, 100% |
| `handover.ndjson` | 53 (unrelated `discs[1].world_x` gap) | 100% to there |
| `bonus.ndjson` | 64 (unrelated `discs[1].world_x` gap) | 100% to there |
| `farbank.ndjson` | 35 (unrelated `tiles[7].hp` gap, discr-dc0) | 100% to there |

The three fixtures with an established "nothing waived, nothing resynced"
boundary reproduce that WHOLE boundary with player 1's three fields
reconstructed -- zero mismatches, any field, any tick. The other three are
gated by fields this bead does not touch.

**Then confirmed against the real `tracecheck`, after the feed retirement
landed** -- all ten required gate invocations, same numbers as before this
bead, none shrunk:

```
golden.ndjson       --skip-waived --min-agree 99   -> OK, 99 matched
tile_damage.ndjson  --skip-waived --min-agree 214  -> OK, 214 matched
golden.ndjson                     --min-agree 99   -> OK, 99 matched
tile_damage.ndjson                --min-agree 214  -> OK, 214 matched
p1_walk.ndjson                    --min-agree 274  -> OK, 274 matched
handover.ndjson                   --min-agree 21   -> PASS at 21 (players[1].state_index, discr-b6x)
handover.ndjson     --skip-waived --min-agree 222  -> PASS at 222 (discs[0].active)
bonus.ndjson        --skip-waived --min-agree 150  -> PASS at 150 (tiles[7].hp, discr-dc0)
bonus.ndjson                      --min-agree 22   -> PASS at 22 (players[1].state_index, discr-b6x)
farbank.ndjson                    --min-agree 34   -> PASS at 34 (tiles[7].hp, discr-dc0)
```

`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
all clean.

## Player 2: stays fed (not this bead's gap)

Two independent reasons, not a waiver of convenience:

* Its own sequences are not fully catalogued -- the `$449e` table above
  surfaces within the first few ticks of `golden.ndjson` alone.
* `disc::THROW_STATES`' release gate reads player 2's `anim_cursor` directly;
  a wrong reconstructed value there desyncs the serve and corrupts the disc
  simulation for BOTH players -- measured directly while building this
  (reconstructing player 2 unconditionally regressed `discs[0].world_x`
  inside golden's own clean 99-tick run).

`crate::player::step` is now a thin wrapper: snapshot player 2's three
fields, run the real step, restore the snapshot for player 2 only. Player 1
passes through untouched. This is an enforced split, not a hope.

## Landing: the feed retired, for player 1

Two coordinated edits, both landed in this bead (no handoff needed --
`main.rs`/`docs/state-schema.md` were free by the time the measurement
above was ready):

* `crates/disc-tools/src/main.rs`: `feed_disc_inputs` no longer feeds
  `anim_cursor`/`x_delta`/`hit_box` for player 0; `checks()` gained three new
  rows (`players[0].anim_cursor`, `players[0].x_delta`,
  `players[0].hit_box[0..4]`), pushed for player 0 only -- a fed field can
  never also be compared, which is why player 2's copies have no row
  (matching `throw_dir_kind`/`throw_damage`/`reach`, still fed for both
  players, also with no row). `resync()` grew matching arms for
  `--skip-waived`/`--resync`. `SCHEMA_COMPARED` 22 -> 25, `SCHEMA_WAIVED` 14
  -> 11. A new `seed_from` helper fixes the one seeding gap this exposed
  (`anim_cell`/`anim_hold` at frame 0 -- see `docs/disc-notes.md`) at the one
  call site that seeds a replay's start; feeding/resyncing/comparison call
  sites are untouched.
* `docs/state-schema.md`: the three generic `players[n].*` waived rows are
  gone; three new `players[0].*` rows in Compared; player 2's copies folded
  into the existing `players[1].*` blanket row (8 fields now, was 5) -- same
  shape `disc_cap` used when it lost its own standalone waiver.

## Files

* `crates/disc-core/src/player.rs` -- `Frame`/`Anim.frames`, 20 `const
  FRAMES_*` tables (ST address ranges cited per table), `enter_anim`/
  `anim_tick` wired to populate `x_delta`/`hit_box`/`anim_cursor`,
  `enter_anim_fallback`, `idle_tick`, the turn/struck-down/struck-up/run_out/
  walk fallback fixes, the `step`/`step_inner` snapshot-restore split.
* `crates/disc-core/src/types.rs` -- doc comments only (fields already
  existed; no shape change).
* `crates/disc-core/tests/anim_measure.rs` -- new, the standalone
  measurement harness.
* `crates/disc-tools/src/main.rs` -- `feed_disc_inputs`, `checks()`,
  `resync()`, `seed_from`, `SCHEMA_COMPARED`/`SCHEMA_WAIVED`, the header
  text, three tests updated to use `seed_from`.
* `docs/state-schema.md` -- three rows moved, `players[1].*` extended, two
  summary counts updated, notes added.
* `docs/disc-notes.md` -- Part 12 section appended.

## bd

`discr-rxx.1` closed: 100% agreement on all six fixtures for the fields this
bead owns, feeds retired for player 1, all ten gates hold at their
established numbers, nothing shrunk.
