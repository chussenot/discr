# disc-oracle -- differential validation report

`disc-oracle` runs the Disc game code under Musashi from a Hatari-captured
seed. This is what it agrees with Hatari about, where it stops agreeing, and
why.

Reproduce with `make oracle-check`.

## Result

| | |
|---|---|
| Exact agreement | **275 consecutive frames**, 3256 bytes/frame ($6a00-$76bf) |
| Region compared | every address the oracle reports: `$6ab4` counter, `$6c58`/`$6c59` joysticks, both player records, all 8 disc records, the 17-cell tile grid |
| First divergence | frame 274, the two video double-buffer pointers only |
| Determinism | two runs of the same seed+script are byte-identical |
| Speed | **~2500 frames/s vs Hatari's ~40** (60x) |

The start of the run is verified, not assumed: the first comparable frame must
match across all 3264 bytes before any comparison proceeds, and both sides are
aligned on `$6ab4`, a counter each emulator computes for itself.

## Where it stops, and why that is not a bug to fix

At frame 274 the only disagreement is `$6aac` and `$6ab0` — the screen
double-buffer pointers, `$00070600` and `$00078300`. They swap every frame
*except* on a frame the game drops: the Hatari reference shows 462 swaps in 487
frames, so about 5% of frames are dropped. Whether a frame drops depends on
whether the main loop finished inside its cycle budget, which is a cycle-timing
question. Musashi is instruction-accurate, not cycle-accurate, so the two sides
eventually drop a different frame. One frame later (275) the whole simulation
has shifted by one frame and the divergence becomes general.

This is the risk the plan named up front, and it landed exactly where predicted
— in video state, outside the oracle's contract, not in game state.

**It is not a mistuned constant.** Sweeping the per-frame cycle budget:

| `--frame-cycles` | frames of exact agreement |
|---|---|
| 158500 | 36 |
| 159750 | **275** |
| 160256 (PAL: 512 x 313) | **275** |
| 160760 | **275** |
| 161500 | 129 |
| 163000 | 129 |

A wide plateau at 275 centred on the true PAL value. The budget is right; the
residual is instruction-level cycle timing inside the frame. Tuning further
would be overfitting to one seed, so the measured 275 is wired in as
`--min-agree`: the check passes at or above it and fails below, which makes the
boundary a regression gate instead of a permanent red.

## What the differ found that reading code had missed

Phase 0 concluded that both MFP timers were free of RAM writes below `$8000`
and could be skipped. The differ disagreed within two bytes, and it was right:
Timer A's **exit** path clears `$6c5b`/`$6c5c`, the sound-effect busy latch.
`reports/oracle-scope.md` carries the correction. Timer A is now emulated from
TACR/TADR with the ST's real clock ratio.

It also forced the seed to carry the MFP register shadow. A sound effect can
already be mid-stream at the seed instant, with its cursor in USP; without
TACR/TADR the oracle starts with the timer stopped and the latch never clears.

## Honest limits

* **275 frames from this seed.** A different seed will have a different drop
  pattern and therefore a different boundary.
* **Video is out of scope and demonstrably so.** The double-buffer pointers are
  excluded by default; `--strict` includes them and fails one frame earlier.
* **Timer B is not emulated.** Its two handlers provably write only palette and
  MFP registers. If a future divergence points at palette-adjacent state, this
  is the first assumption to re-test.
* **Frames 0-4 are not compared.** Capturing the seed costs a 1 MB `savebin`,
  so Hatari's trace begins about five frames later. The differ says so and
  aligns on `$6ab4` rather than pretending otherwise.
* **One input script exercised end to end** (idle). The IKBD path is
  implemented and packet-accurate by construction, but the differential suite
  does not yet drive a scripted joystick run.
