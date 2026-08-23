# Exploration report -- possession, tiles, states, IKBD timing

Everything here is graded. **Tier 1** means the fact was observed inside a
window where the oracle and Hatari agree byte-for-byte, so it is a fact about
the game. Anything else is labelled.

## 1. The IKBD delivery offset, measured

The half-frame `--ikbd-delay` had been a guess since Part 6. Two measurements
replace it:

* **Arrival is uniform across the frame.** 24 breakpoint samples at the ACIA
  handler `$8370` gave FrameCycles from 15960 to 153184 with deciles at
  roughly even spacing -- what an asynchronous 7812.5-baud device should look
  like.
* **The game consumes `$6c58` at ~23200 cycles (scanline ~45).** Measured by
  bisecting `--ikbd-delay` against the Hatari reference rather than by
  breakpointing a proxy: exact agreement steps from 61 frames to 364 between
  23125 and 23312, and the high plateau runs to at least 159000.

| `--ikbd-delay` | frames of exact agreement |
|---|---|
| 0 - 16000 | 61 |
| 23125 | 61 |
| **23312** | **364** |
| 40000 - 159000 | 364 |

The default (80128, half a frame) sits in the middle of the plateau, so it was
right -- but it is now justified.

**The finding is what the two measurements say together.** Arrivals are
uniform and consumption is 14.5% into the frame, so roughly **one packet in
seven lands before consumption and is acted on in the same frame**. That is a
per-packet coin flip; no fixed delay reproduces it. disc-oracle trades it for
determinism deliberately, and it is one more reason input-heavy programmes
desync sooner than idle ones.

## 2. Drop-aware tier 2: implemented, and it does not work

The premise was that the desync is one dropped frame after which the whole
simulation is shifted by one. The tier was built with a falsifiable guard --
the three per-frame counters (`$6ab4` +1 at `$8198`, `$6ab6` -1 at `$819c`,
`$6c81` -1 at `$81e2`) must be off by *exactly* the shift -- and the guard
rejected the premise.

At the idle boundary (frame 275), shifting the oracle back one frame leaves
exactly **4** differing bytes, and all four are those counters, each off by
exactly one. A textbook dropped frame. For one frame. Stray-byte counts for
the following frames, at shift -1:

    0  64  59  116  32  88  35  63

The runs do not stay in shifted lockstep. Once the game's own timing counters
differ, the two runs take *different paths*, not the same path offset in time.

So tier 2 stays in the tool, prints that table on refusal, and **extends
nothing**. No tier-2 claim is made anywhere in this report. Seed relay is the
mechanism that actually reaches late events.

## 3. Tile damage -- resolved, tier 1

Grid changes were read straight out of the Hatari references, inside their
validated prefixes:

| reference | validated to | frame | change |
|---|---|---|---|
| idle | 275 | 65 | cell6 `(1,1) -> (0,0)` |
| idle | 275 | 165 | cell7 `(2,4) -> (2,1)` |
| idle | 275 | 203 | cell8 `(1,4) -> (1,1)` |
| rightfire | 116 | 65 | cell7 `(2,4) -> (2,1)` |
| rightfire | 116 | 89 | cell7 `(2,1) -> (0,0)` |

Watching `$7650` (cell 7 `+$02`) in Hatari gave writer PC **`$a350`**, i.e. the
store at `$a34c`:

```
$a31c  sub.w  ($0016,a5),d6        ; HP -= the DISC's damage field
$a344  tst.w  d6
$a346  bge.w  $a34c
$a34a  clr.w  d6                   ; clamp at 0
$a34c  move.w d6,($02,a0,d5.w)     ; store HP back   <-- the writer
$a350  bgt.w  $a3ba                ; still alive? done
$a354  clr.w  ($00,a0,d5.w)        ; dead: clear the TYPE word too
$a358  cmp.b  #$03,$6c5c           ; and queue the destruction sample
```

So the cell is `{+$00 type, +$02 hit points}`:

* damage is **not a constant** -- it is read from the *disc* record at `+$16`
  (every observed hit took 3, so that disc carried 3);
* HP is clamped at 0, never negative;
* when HP reaches 0 the **type word is cleared as well**, which is exactly the
  observed `(2,1) -> (0,0)`;
* destruction queues sound priority 3.

**This resolves the standing conflict in favour of the design notes.** The
creator interview said platforms take more hits at higher ranks; that is an HP
model and it is correct. Part 5's reading of `+$02` as occupancy was wrong --
occupancy is `+$00`, the word the movement code `tst.w`s as a walkability gate.
Recorded in `docs/disc-notes.md` next to the earlier angle-table correction.

**Not explained:** `(1,5) -> (1,133)` and `(0,0) -> (0,128)` set bit 7 of the
HP word and it is cleared again much later. `$a34c` stores a plain value and
cannot produce that, so a second writer exists. Flagged, not guessed at.

## 4. Possession -- answered, and the answer is "there is none"

The question was whether an incoming disc ever becomes player-owned, or
whether possession is only ever gained by serve. The evidence says **neither**:
there is no holding state at all.

What the disc engine actually does, from `$a71a`-`$a860`:

```
$a71a  move.w $6ca2,d5 ; sub.w #$13,d5 ; cmp.w d0,d5   ; home on player 1 X
$a758  move.w $6ca4,d5 ; sub.w #$10,d5 ; cmp.w d1,d5   ; home on player 1 Y
$a7d8  move.w $6d22,d5 ; subq   #4,d5                  ; home on player 2 X
$a816  move.w $6d22,d5 ; sub.w  #$13,d5                ; player 2, other offset
```

Each nudges a velocity by +/-1 per frame, clamped to `[-2,+2]` -- **on both
axes**: `+$06` is the X velocity and `+$08` is the Y velocity, steered against
`$6ca2` and `$6ca4` respectively. A disc is always in flight and always homing
on a *target player's* coordinates. The states seen are racked (`-3` at world
`(140,53)`), launched (`+1`, `world_z` from 0), and reflected (`neg.w` at
`$a606`).

**Consequence for `disc-core`:** a disc has an *aim target*, not an owner. The
API should be `Disc { aim: PlayerId, .. }`, not `Disc { held_by: Option<PlayerId> }`,
and there is no catch/throw transition to model -- only serve, home and
reflect. What would settle the last of it is the selector that picks between
the `$a71a` / `$a7d8` / `$a816` steering variants; that is the next read.

## 5. Promotion ledger

The promotion rule is unchanged: notes-grade requires a validated prefix.

**Promoted** (tier 1, from the idle/sweep/rightfire references within their
validated windows). Handler addresses from the `$10e2c` table:

| state | handler | promoted from |
|---|---|---|
| 5 | `$fb6e` | sweep, within 364 |
| 14 | `$106b2` | sweep frame 77; also rightfire within 116 |
| 20 | `$1094a` | sweep + rightfire |
| 21 | `$109aa` | sweep |
| 24 | `$10ac4` | sweep |
| 27 | `$10c8a` | sweep |

**Not promoted, with the blocker.** States **11, 16, 17, 19, 23, 31** were
seen only in autopilot runs, at frames 125-322, and no validated prefix
reaches them:

* the programmes that reach them are input-dense, and input density is exactly
  what shortens the window (116 frames for rightfire);
* seed relay would fix that, except the relayed seed lands in an active
  three-disc rally where drops come fast -- `rally_f100` verified at only 30
  frames;
* tier 2, which was supposed to bridge the gap, does not work (section 2).

The honest position is that these six need a *quieter* proximal seed: one
minted at a moment when few discs are live, close to the input that triggers
the state. That is a seed-selection problem, not a tooling gap.
