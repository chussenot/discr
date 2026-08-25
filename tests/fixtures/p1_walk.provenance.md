# `p1_walk.ndjson` — provenance

275 frames. The third committed fixture, minted in Part 11 to exercise code the
other two never reach.

    printf 'j 5 04 00\nj 30 00 00\n' > tmp/walkleft.script
    ./oracle/disc-oracle --seed seeds/diff.seed \
        --script tmp/walkleft.script --frames 275 \
        --trace tests/fixtures/p1_walk.ndjson

* **Seed**: `seeds/diff.seed`, sha256 `4dee348993853659...`, the same seed as the
  other two fixtures — captured at `PC == $8198` in a live CHALLENGE round
  (`$6ab4` = 6949). Gitignored; see `seeds/MANIFEST.md`.
* **Input**: player 1 walks **left** from frame 5 to frame 30 and then stands
  still. Nothing else. Player 2 is the AI throughout, as in both other fixtures.
* **Why this window is trustworthy**: `scripts/oracle_diff.py` reports **275
  frames of tier-1 (frame-exact) agreement** with Hatari for this seed, so all
  275 frames here are inside it — the same guarantee `tile_damage.ndjson` uses
  for its 215.

## Why it exists, and what it is named for

It is named for what it *does*, not what it was aimed at. The exploration that
produced it was looking for a trace where a player **swings** — enters one of the
racket states 7..10 — because that is the only way to reach `$10fd8`'s racket
path, which no fixture has ever run. Walking player 1 into a disc's path does not
do that. It does something else worth having.

## What it exercises that the others do not

* **`$a78e`, a fourth steering hook.** The other two fixtures only ever install
  `$a71a`, `$a7d8` and `$a816`; this one installs `$a78e` too — player 1's
  *shallow* aim, the exact mirror of `$a7d8`. `disc_core::SteerHook` was missing
  the variant until this fixture found it.
* **Player 1's own anticipation cascade.** `$113e2` installs `$a71a` on frames
  272-274, and player 1 enters state 18 — its intercept — at frame 272. Only
  three frames of it, at the very end, so this fixture *proves the cascade runs*
  without exercising much of it.
* **Player 1 reaching state 11** (knocked down) at frame 159 from a strike no
  other fixture produces at that position, and **five tile changes** against
  `tile_damage`'s four.

## Expected result

**143 ticks, then a divergence owned by a bead.** 123 until player 2's strike
(`$c934`) was implemented, 142 until the tile collapse moved to the end of the
tick.

The wall is frame 144, `players[1].world_x` 122 against 125: player 2 stops
walking one step early because the cell its probe reads has just collapsed — in
**player 1's** bank, on the frame the collapse clears it. Switching that probe to
player 2's own bank looks obviously right and is measurably worse (this fixture
drops to 99 and `tile_damage` stops being clean), so player 2's probe is neither
`$7616[grid_cell(x - 24, y)]` nor the same expression over `$7596`.
`// UNKNOWN: see bd discr-b6x`.

Both other fixtures are clean, so this is the only one of the three that gates on
a prefix. That is the point of adding it: a clean gate measures nothing new.
