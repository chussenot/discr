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
* **How far this window is trustworthy: NOT 275, and the claim here was wrong.**
  `scripts/oracle_diff.py` reports 275 frames of tier-1 agreement with Hatari for
  this seed **under the idle programme**, which is what `tile_damage.ndjson`
  relies on. That measurement does not transfer to a different input: nothing has
  ever compared the `walkleft` programme against Hatari, because no Hatari
  reference for it exists.
  What is measurable without one is this. Across `golden` (100 frames) and
  `tile_damage` (215), the ST's disc loop writes `disc+$00` **exactly once per
  frame, always**. In this fixture it does the same for 191 frames and then
  starts running **twice on alternate frames** from 192 onward — 37 such frames
  in 275. So frames 0–191 behave like both validated fixtures and frames 192+ do
  something neither has ever done, and until a Hatari run of this programme
  exists **192 onward is not evidence about the game**.
  See bd discr-ovl.7.

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

**191 ticks, then a divergence owned by a bead.** The history of that number is
the fixture's whole value: 123 until player 2's strike (`$c934`) was implemented,
142 until the tile collapse moved to the end of the tick, 143 until the walk
probe stopped gating the step — **which it never did on the ST**, and which both
other fixtures had agreed with for eleven parts because in neither does a walking
player ever probe a destroyed cell.

The wall is frame 192, `discs[0].world_x` 27 against 29 — and it is **not** a
disc rule. Frame 192 is exactly the first frame on which the ST's disc loop runs
twice, and `disc-core` steps once per tick by construction. So the gate at 191 is
the last frame inside the region that behaves like the two validated fixtures,
which is a better reason to stop there than "the next rule is missing".
