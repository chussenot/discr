# disc-oracle -- differential validation report

`disc-oracle` runs the Disc game code under Musashi from a Hatari-captured
seed. This is what it agrees with Hatari about, where it stops agreeing, and
why.

Reproduce with `mise run oracle-check`.

## Result

| | idle | scripted joystick |
|---|---|---|
| Exact agreement | **275 frames** | **363 frames** |

3256 bytes compared per frame ($6a00-$76bf, video pointers excluded).

| | |
|---|---|
| Region compared | every address the oracle reports: `$6ab4` counter, `$6c58`/`$6c59` joysticks, both player records, all 8 disc records, the 17-cell tile grid |
| First divergence | video double-buffer pointers only (frame 274 idle / 362 with input) |
| Determinism | two runs of the same seed+script are byte-identical |
| Speed | **~2500 frames/s vs Hatari's ~40** (60x) |

The start of the run is verified, not assumed: the first comparable frame must
match across all 3264 bytes before any comparison proceeds, and both sides are
aligned on `$6ab4`, a counter each emulator computes for itself.

## The input run

The idle run proves nothing about input, so the differ also drives a joystick
programme: Right, Left, Up, Fire, each held and released. The stimulus goes
into Hatari as real XTEST key events, so *which* frame it lands on is not ours
to pick -- the differ reads back the frames on which the game itself decoded a
new `$6c58`/`$6c59`, and builds the oracle's script from those. The oracle is
given IKBD **packets**, never the decoded byte, so the whole path is under
test: Hatari's IKBD emits packets, the oracle's synthetic ACIA emits packets,
and both copies of the game decode them through the self-modifying vector
state machine at `$8370`.

Hatari decoded exactly the programme: `$08` (right), `$00`, `$04` (left),
`$00`, `$01` (up), `$00`, `$80` (fire), `$00`. The oracle reproduces all of it,
and everything downstream, for 363 frames.

Two real bugs surfaced here that the idle script could not have found:

* **The ACIA deasserted on interrupt acknowledge.** It must stay asserted while
  a byte is waiting and be cleared by the handler reading `$FFFC02`. Because
  the decoder is a two-interrupt state machine (`$FF`, then the state byte),
  clearing it on acknowledge meant the second byte was never fetched and
  `$6c58` never changed at all.
* **Packets were delivered on the frame boundary.** The real IKBD runs at
  7812.5 baud and is not synchronised to the VBL, so a byte lands *inside* the
  frame -- after the VBL handler's movement code has already sampled `$6c58`.
  Delivering at the boundary made the player start walking one frame early
  (`$6cae` = `$14` while Hatari still had `0`) even though `$6c58` itself
  matched. Staged bytes are now released partway into the frame
  (`--ikbd-delay`, default half a frame). That offset is a modelling choice,
  not a measurement, and it is the thing to re-examine first if an input-timing
  divergence ever shows up again.

## Using the oracle as a search engine (Part 7)

Every earlier phase stalled on the same thing: the player never gets a disc,
so the throw path stayed unexercised -- it blocked `tile_hit` in Part 4, the
throw handler in Part 5, and fire validation here. Blind input fuzzing is the
wrong tool for it, because catching is a positioning problem. A deterministic
emulator at 2500 fps is the right one: it can read the game's own state each
frame and steer.

`scripts/explore.py` drives `disc-oracle --autopilot`, whose control law needs
no knowledge of the disc's coordinate space -- `$6cb0` is the player's grid
cell, so "walk to cell N" is *cell too high -> Left, too low -> Right*. Only
the policy parameters are searched (8 target cells x 4 fire patterns). Whatever
it finds is emitted as a plain script, because Hatari cannot run an autopilot
and the finding has to be replayable.

The most productive policy by a wide margin was **cell 16 (far right) plus
pulsed fire**, which reached player states 2, 11, 16, 17, 19, 23 and 31 and
moved four counters. It also degenerates to "hold Right" once the player is
against the edge, which is the only reason a closed-loop result could be
replayed open-loop into Hatari at all -- that was luck, not design.

### What Hatari confirmed

Replayed as the `rightfire` programme, the two sides agree byte-for-byte for
**116 frames**, and that window already contains the interesting behaviour:

* `$6c58` = `$88` (Right + Fire) decoded by the game;
* player state **14** entered, which idle play never reaches;
* disc 0's owner field cycling `+1 -> -3 -> +1`;
* **disc 1 becoming live** (`-3`, then `+1`) -- a second disc served.

The writer PCs for the owner field were then measured directly in Hatari:
`$a606` (`neg.w`, the turn-around), `$a618` (set to `+1`) and `$a9b4`/`$a9b8`
in the spawn routine at `$a9a0`. Those are in `docs/disc-notes.md`.

### What is still oracle-only

Player states 11, 16, 17, 19, 23 and 31, the third simultaneous live disc, and
the counter movements after frame ~185 all occur **beyond** the validated
window. They are plausible and self-consistent, but until a programme is found
that reaches them inside a validated prefix they describe the oracle, not
provably the game. They are not in `disc-notes.md` for that reason.

### The boundary shortens as input gets busier

| programme | frames of exact agreement |
|---|---|
| idle | 275 |
| sweep (one press per direction) | 363 |
| rightfire (24 fire pulses) | **116** |

The input rows carry about +/-1 frame of jitter run to run: the stimulus goes
into Hatari as wall-clock XTEST, so a press occasionally lands on the
neighbouring frame and the reference itself differs slightly. `oracle_check.sh`
therefore asserts 360 and 112 rather than the exact figures, so the gate
catches a real regression without going red on jitter.

Same mechanism every time -- the video double-buffer pointers desync first, one
frame ahead of the general divergence. More input means more work per frame,
more frames overrun their budget, and the drop patterns part company sooner.
This is the clearest evidence yet that the limit is cycle accuracy and not a
modelling gap: the oracle does not get *wronger* with input, it gets less time
before an unmodellable timing coin-flip goes the other way.

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

* **275/363 frames from this one seed.** The boundary is a property of the
  seed and the input, not a constant: the same build agrees for 275 frames
  idle and 363 with input, because the frame-drop pattern differs.
* **Video is out of scope and demonstrably so.** The double-buffer pointers are
  excluded by default; `--strict` includes them and fails one frame earlier.
* **Timer B is not emulated.** Its two handlers provably write only palette and
  MFP registers. If a future divergence points at palette-adjacent state, this
  is the first assumption to re-test.
* **Frames 0-4 are not compared.** Capturing the seed costs a 1 MB `savebin`,
  so Hatari's trace begins about five frames later. The differ says so and
  aligns on `$6ab4` rather than pretending otherwise.
* **The IKBD delivery offset is a guess**, not a measurement: half a frame.
  It is enough to put packet arrival after the movement code, which is what
  matters here, but a game that samples input twice per frame would expose it.
* **Two input programmes exercised** (idle, and one joystick sequence). Fire
  is pressed but the player never has the disc, so the throw path is still
  unexercised on both sides.
