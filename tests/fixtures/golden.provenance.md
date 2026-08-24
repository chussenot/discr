# `golden.ndjson` — provenance

The tracecheck golden fixture. 100 frames, generated for bead **discr-3g6**.
Regenerate with [`scripts/regen_golden.sh`](../../scripts/regen_golden.sh).

    ./oracle/disc-oracle --seed seeds/diff.seed \
        --script tmp/leftright.script --frames 100 --trace tests/fixtures/golden.ndjson

## Where it came from

* **Seed**: `seeds/diff.seed`, sha256 `4dee348993853659...`, captured at the
  sampling point `PC == $8198` in a live CHALLENGE round (`$6ab4` = 6949).
  The seed is **gitignored** (`seeds/`, `*.seed`) — see `seeds/MANIFEST.md`.
  So is this file's sibling: `*.ndjson` is gitignored and `golden.ndjson` was
  committed with `git add -f`.
* **Input**: the `leftright` programme, `j 9 04 00 | j 27 00 00 | j 102 08 00`
  (Left frames 9–27, Right from frame 102). The script was derived by read-back
  from Hatari's own decoded `$6c58`/`$6c59`, so the input recorded in the trace
  is input Hatari really delivered, not input we asked for.

## Why this window is trustworthy

`scripts/oracle_diff.py --input leftright` reports **256 frames of tier-1
(frame-exact) agreement** with Hatari over 3256 bytes per frame. Frames 0–99
are inside that differentially validated prefix, so this trace is
known-faithful rather than merely plausible. That is what makes it a legitimate
oracle for `disc-core` and not just a regression snapshot of our own output.

## What it exercises

* Player-1 states 0, 1, 11, 20, 23.
* A Left press decoded through the real IKBD path (`$6c58` goes `$00` → `$04`
  at frame 10, back to `$00` at frame 28).
* One live disc integrating: slot 0, `dir_kind` +1, `vel_x` −2, `world_z`
  advancing +1 per frame.
* `dir_kind` emitted **unsigned** — `65533` in the JSON is `−3` on the ST.

## Record shape

One JSON object per line. Keys not listed below are ignored by tracecheck.

| key | maps to | compared? |
| --- | --- | --- |
| `frame` | trace index, not an ST value | no |
| `vbl_6ab4` | `$6ab4`, → `GameState::frame` as `u16` | yes |
| `joy_6c58` | `$6c58`, p1 input; `$80` is an **edge** (`bclr #7`) | drives the sim |
| `player[n].{x,y,facing,state,cell}` | `player+$02/$06/$09/$0e/$10` | yes |
| `disc[n].{wx,wy,wz,vx,flag}` | `disc+$00/$02/$04/$06/$0a` | yes |
| `disc[n].{sx,sy}` | `disc+$0c/$0e` | **no** — `excluded:projection` |
| `grid[n]` | `[tile+$00, tile+$02]` | yes |
| `state_sha256` | oracle's own digest | no |

The trace has **no column** for `disc+$08` (`vel_y`) or `disc+$16` (`damage`),
both of which `docs/state-schema.md` marks `compared`. tracecheck seeds them to
0 and skips them, and says so in its header line. A future regeneration that
adds those two columns needs no tracecheck change beyond two more rows in
`checks()`.

## Expected result today

`disc-core`'s `player`, `disc` and `tile` modules are stubs, so tracecheck is
*expected* to report a divergence on frame 1. That is the correct outcome, not
a failure of the fixture.

## Regenerated for Part 10

Re-emitted from the same seed and the same input programme with the oracle's
Part 10 columns added. **Every pre-existing column is byte-identical on every
frame** -- checked field by field, not assumed -- so this is a strict superset
of the file it replaces and no earlier result is invalidated.

New per-disc columns: `vy` (`disc+$08`), `dk` (`disc+$0a` read **signed**, where
`flag` is the same word unsigned and is kept for compatibility), `act`
(`disc+$10`), `own` (`disc+$11`), `hook` (`disc+$12`) and `dmg` (`disc+$16`).
New per-frame columns: `joy_6c59`, `ai_6da1`, `mode_6da0`, `bonus_6d9a`.

With `dmg` present, `tracecheck` compares all 15 of `docs/state-schema.md`'s
compared rows and says so in its header instead of naming missing columns.

## Regenerated again for Part 10b-d

Same seed, same programme, more columns, and every pre-existing column verified
byte-identical again. Added per player: `anim` (`player+$3a`, the animation
sequence cursor the serve gates on), `throw_dk` / `throw_mag` (`+$6e` / `+$70`,
the dir_kind and damage that player's throws carry), `box` (`+$1c`..`+$22`, the
hit box, copied out of the current animation cell) and `energy` (`+$76`).

## Regenerated for Part 10e-f

Two more additive columns, old ones verified byte-identical again: `banks`
(**both** 16-cell tile banks, `$7596` then `$7616`, 32 entries -- the 17-cell
`grid` above predates the discovery that a bank is 16 and is kept so earlier
fixtures still load) and per-player `reach` (`player+$12`).

## Regenerated for Part 10g

Three more additive per-player columns, old ones verified identical again:
`discs_out` (`player+$6a`, how many discs that player has in play) and
`disc_cap` (`player+$6c`, the cap state 18 refuses to throw past).
