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
  `tile_damage` (215) the game's update pass runs **exactly once per sampled
  frame, always**. In this fixture it runs once on 200 frames, **twice on 37 and
  not at all on 37** — because the update lives in the main loop rather than the
  VBL, and the sampling point is the VBL. That was resolved in Part 11f and is
  not an oracle artefact: `updates` is a column now and `disc-core` replays it.
  So this fixture no longer has a "suspicious region"; what it still lacks is an
  independent Hatari comparison for **its own** input programme. Numbers measured
  against it are `disc-core` against the oracle, not against the machine.

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

**237 ticks** as of Part 11g. The frame-192 wall was `disc-core`'s own shape
twice over: one tick was one update, and a sampled frame holds 0, 1 or 2 of them
(Part 11f) — and each pass consumes its **own** joystick bytes, because `$d2cc`
rewrites `$6da1` inside the repeat loop (Part 11g). Frame 224 used `$08` then
`$00`, and driving both passes from the sampled `$00` loses the walk step the
first one made.

### The frame-224 wall, and the note that predicted its answer

Kept verbatim because it asked for exactly what Part 11g then built, and
because a question answered is worth more on the page than deleted:

> The wall now is frame 224, `players[1].world_x` — and it is attributed, not
> just located (bd discr-hif). It is **not** a missing `world_x` writer: the
> `--watch 0x6d22` list from Part 10i is fully transcribed (`$b038`, `$b24e`,
> `$c1d0`, the `$abc6` idle-path delta and `$ae84`'s state-16 sidestep are all in
> `player.rs`). It is the input channel's granularity. `$10ec6 bsr $d2cc` writes
> the AI byte and `$10ece` consumes it **once per `$96be`-`$96cc` main-loop
> pass**, while `ai_6da1` is sampled once per VBL — so a 2-update tick holds two
> consumed bytes and the trace records only the second. Frame 223 -> 224 is the
> one tick in this fixture where the two differ *and* it moves `world_x`: pass A
> consumed `$08` (state 2 steps +3 at `$b24e`, x 83 -> 86), pass B consumed `$00`
> (the walk exits to the turn — state 20 with `player+$09` still 2 at the sample,
> which is exactly what the trace shows). `tracecheck` feeds the destination
> sample to every pass, so it misses the step; feeding the *previous* sample
> instead breaks tick 218 (`04 -> 00`, x stays 80: there both passes consumed the
> new byte). No rule over the sampled bytes satisfies both — the mid-frame byte
> is the AI policy's output, and its test routines and `$cea6` sensor pass are
> undecoded. Passing 224 therefore needs either the policy or a re-minted
> fixture with a per-pass input column, which needs the gitignored seed.
>
> With `--skip-waived` (player 2's rows resynced from the trace) the run reaches
> **237** and diverges on `tiles[14].tile_type` at frame 238 — disc-core destroys
> the tile on a frame the ST does not — which is the first wall that is not
> player 2's.

The re-minted fixture is this file. `pass_joy` / `pass_ai` record both bytes
per frame, so 224 passes without a decoded AI policy: what the note called the
input channel's granularity was the whole of it.

Part 11h took it to **255**: the collapse advance at `$96b6` runs once per
*outer* main-loop iteration, and there are 237 of those in these 275 frames.
Part 11i took it to **271** by mirroring player 2's knock-down (`$ca12`, and
states 11 and 12, which were shared between the tables and are not).

Part 11j took it to **274 — all of it**, by transcribing player 1's
anticipation cascade (`$112f4`) and, more to the point, by discovering that the
disc's owner byte moves in this trace and had to be fed. This fixture is now
clean, and it is the trace that proved three separate wrong models the two
older fixtures agreed with.
