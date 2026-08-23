# Disc (Loriciel, 1990, Atari ST) -- reverse-engineering notes

Confirmed memory map and code entry points. Generated lines come from
`reports/findings.md`, which `scripts/analyze.py` regenerates from real runs;
each line names the evidence behind it.

Conventions: addresses are ST physical addresses. Game state lives below
$8000, code from ~$8000 up. `$6ca0` is the player-1 entity record and `$6d20`
the player-2 one (same layout, stride $80).

## Confirmed variables and entry points

```
$6ca2  player_x  (word, high byte always 0, unsigned screen/grid coordinate)  idle 117; Right -> 152; Left -> 8; unchanged across the other axis
$6ca6  player_y  (word, high byte always 0, unsigned screen/grid coordinate)  idle 18; Up -> 25; Down -> 2; unchanged across the other axis
$6ab4  vbl_frame_counter  (word, wraps)  increments by exactly 1 per PAL VBL across 11 equal-gap samples, never reverses; note this is $6ab4, NOT the $6ab6 given in the brief, which is zero in every in-match dump
$6c58  joystick_decoded  (byte)  $01 up $02 down $04 left $08 right $80 fire, ORed; read as (a0) by the movement code; fire bit cleared on use by bclr #7 at $f606/$f81a/$fb90
$6cae  player_state  (byte)  index into the 32-entry jump table at $10e2c; 1 = walk left ($f5e2), 2 = walk right ($f7f6)
$10e2c  player_state_table  (32 longs)  state handler addresses
$6ca9  player_prev_state  (byte)  the PREVIOUS frame's state_index -- NOT facing
$6cda  anim_cursor  (long)  steps by 6 through the table at $2988
$6ce2  anim_countdown  (word)  frames left on the current anim cell
$f658/$f86c  player_walk  (code)  subq/addq #3 on $6ca2; walkable X is 8..152, probed +/-24 ahead and range-checked against 8 and $98
$f838  player_row_test  (code)  cmp.w #$000e,$6ca6 -- Y > 14 selects the far row of the floor grid
$6e3e  disc[0].world_x  (word, signed)  integrates by vel_x at $6e44 while world_z advances (47/48 frames verified); disc array is 8 x $42 bytes from $6e3e
$a722-$a860  disc_steer  (code)  nudges vel_x +/-1 toward playerX+offset, clamped [-2,+2] -- there is no angle table
$a6b2/$a6b6  disc_project  (code)  writes screen_x/screen_y from world (x,y,z) via LUTs $7abe, $7b5e, $59952, $5b252
$7616  tile_grid  (17 cells x 8 bytes)  cell = column($6ca2) + 8 + (4 if $6ca6 > 14); column from the byte table at $7bfe; verified on 4 independent samples
$6cb0  player_cell  (word)  the player's current grid cell index, 9..16 over the 4x2 floor
$7bfe  x_to_column  (145 bytes, index = world X 8..152)  4 columns of 40 X-units; 0 outside the arena
$6ca2  writers  $f65c, $f870, $f908, $f11c  43 changes while driving that axis; dominant writer just above $f65c
$6ca6  writers  $fe7a, $fbb6, $fbc6, $fe8a  34 changes while driving that axis; dominant writer just above $fe7a
```

## Record layouts

```
player entity record -- $6ca0 (player 1), $6d20 (player 2), stride $80
  +$02  world X          walkable 8..152, +/-3 per frame
  +$06  world Y (row)    > 14 = far row
  +$09  facing           1 = left, 2 = right
  +$0e  state index      into the jump table at $10e2c
  +$10  grid cell index  8 + column(X) + (4 if Y > 14)

disc record -- $6e3e, 8 records, stride $42
  +$00  world X          integrated by +$06
  +$02  world Y/height   $52 at round init
  +$04  world Z (depth)  +1 per frame while in flight
  +$06  X velocity       signed, clamped [-2,+2]
  +$0a  live flag
  +$0c  screen X         projected each frame at $a6b2
  +$0e  screen Y         projected each frame at $a6b6
  +$1a  long  -> $4a46
  +$3e  long  -> this disc's sub-record in the array at $704e (stride $1e)

tile grid cell -- $7616, 17 cells, stride 8
  +$00  word  occupancy / owner, {0,1,2}; tst.w'd as a walkability gate
  +$02  word  per-cell property, {1,4,5}
  +$04  long  zero
```

## Interrupts and I/O (Phase 0, measured in a live match)

```
$70   -> $8198   level-4 VBL, once per frame; first instruction is
                 addq.w #1,$6ab4, and $819c then does subq.w #1,$6ab6
$118  -> $8370   MFP 6, IKBD ACIA.  Self-modifying vector state machine:
                 $8370 reads $FFFC02; $FF re-points $118 to $83b2 (next byte
                 -> $6c58, joystick 1), $FE re-points to $83c2 (-> $6c59,
                 joystick 0), otherwise the byte is a key code -> $6c56.
                 Packets arrive on state CHANGE only, not per frame.
$120  -> $8362   MFP 8, Timer B; the VBL handler re-points it to $8320 each
                 frame.  Both handlers write only palette + MFP registers.
$134  -> $83d2   MFP 13, Timer A, ~4.9 kHz PSG sample streamer.  Its cursor
                 is USP.  Its EXIT path at $83fe clears $6c5b and $6c5c.

Enabled: IERA/IMRA $21 (Timer A + Timer B), IERB/IMRB $40 (ACIA).
Timer C and Timer D are off; HBL is never taken; no ROM executes in a match.
Hardware touched per frame: PSG $FF8800-$FF8806, palette $FF8248-$FF825E,
screen base $FF8201/$FF8203, MFP $FFFA19/$FFFA1B/$FFFA1F/$FFFA21.  No FDC.
```

## Addresses added in Phase 6

```
$6c59  joystick_0        (byte)  mouse/joystick port 0, decoded at $83c2
$6c56  last_key          (byte)  raw key scancode, stored at $8378
$6c5b  sfx_active        (byte)  $FF while a sample plays; set st.b at $a6e4,
                                 cleared by Timer A's exit path at $8402
$6c5c  sfx_busy          (byte)  1 while a sample plays; set at $a6ca,
                                 cleared at $8406
$6aac  screen_buf_a      (long)  $00070600 / $00078300, swapped every frame
$6ab0  screen_buf_b      (long)  the other half of the pair
$6ab6  vbl_down_counter  (word)  decremented at $819c every frame
```

## Disc owner/direction field (Part 7)

`record+$0a` is not a boolean "live" flag. Observed values are `+1`, `-1` and
`-3`, and the code says why:

```
$a606  neg.w  ($000a,a5)      ; negated when the disc turns around
$a618  move.w #$0001,($000a,a5)
$a9b4  move.l d2,($0008,a1)   ; written as part of the spawn record store;
$a9aa  addq.w #$01,$6d8a      ; the same routine bumps this counter
```

So the **sign is the travel direction** (flipped by `neg.w`, not by a
comparison) and the magnitude distinguishes at least two kinds of disc. A
parked disc reads `-3` at world `(140, 53)`; when it launches it becomes `+1`
at world `(135, 0)` and `world_z` starts climbing. Up to 3 discs were seen
live at once, out of the 8 records.

Disc `world_x` spans **0..153**, i.e. the same range as the player's walkable
X (8..152) -- an earlier trace that only showed 0..48 had simply caught one
leg of a flight.

## Tile damage (Part 8, tier 1)

The cell is `{+$00 type, +$02 hit points}` -- **not** occupancy in `+$02`; that
was a Part-5 misreading, and `+$00` is the occupancy/type word the movement
code `tst.w`s as a walkability gate. This **resolves the design-intent
conflict in favour of the creator interview's HP model**, the same way the
angle-table claim was resolved against it earlier.

```
$a31c  sub.w  ($0016,a5),d6      ; HP -= the DISC record's damage field (+$16)
$a34a  clr.w  d6                 ; clamped at 0, never negative
$a34c  move.w d6,($02,a0,d5.w)   ; writer; Hatari reports PC $a350
$a354  clr.w  ($00,a0,d5.w)      ; HP == 0 also clears the TYPE word
$a360  move.b #$03,$6c5c         ; and queues the destruction sample
```

Observed tier 1: `(2,4)->(2,1)`, `(1,4)->(1,1)`, `(2,5)->(2,2)` (all -3, the
damage that disc carried), then `(2,1)->(0,0)` and `(1,1)->(0,0)` on the
killing hit. A second, unidentified writer sets bit 7 of the HP word
(`(1,5)->(1,133)`) and clears it later; not explained.

```
$a2ec/$a300  destroyed_cell_guard  (code)  tst.w on the cell TYPE word; beq
                                     skips to $a3ea, so a type-0 cell never
                                     reaches the damage code at all
$a314        damage_multiplier     (code)  cmp.w #$0001,$6d9a -- when $6d9a is
                                     1 the disc's damage is subtracted a
                                     SECOND time at $a31c; $a32e tests 3 for a
                                     further path.  Semantics undecoded, see
                                     bd discr-z8m
$7616+cell*8 +$00  tile_type      (word) {0,1,2}; 0 = destroyed; walkability gate
$7616+cell*8 +$02  tile_hp        (word) -= disc[+$16] per hit, clamped at 0
$a34c              tile_damage    (code) the HP store; $a354 destroys the cell
```

## Disc steering and possession (Part 8, tier 1)

There is **no possession**. A disc is always in flight and always homing on a
target player's coordinates; the engine has serve, home and reflect, and no
held state. `disc-core` should model `Disc { aim: PlayerId, .. }`, not
`held_by: Option<PlayerId>`.

```
$6e3e+n*$42 +$06  vel_x   (word) steered +/-1 per frame toward $6ca2, clamped [-2,+2]
$6e3e+n*$42 +$08  vel_y   (word) steered the same way toward $6ca4 -- see below
$6e3e+n*$42 +$16  damage  (word) subtracted from tile HP on impact
$a71a  steer_at_p1_x  (code)  $6ca2 - $13
$a758  steer_at_p1_y  (code)  $6ca4 - $10
$a7d8  steer_at_p2_x  (code)  $6d22 - 4     ($a816 uses $6d22 - $13)
```

## Player +$04 is NOT the player's Y (resolved, Part 9)

`contract` flagged an apparent conflict: the record layout puts the player's Y
at `+$06` (`$6ca6`), but `$a758 steer_at_p1_y` homes the disc's `vel_y` on
`$6ca4` = `player+$04`. Both are right; they are different quantities.

From the Part-5 hexdump of the player record:

```
$6ca0:  0001   0075   0063   0012
        +$00   +$02   +$04   +$06
        flag   X=117  99     Y=18
```

`+$04` is a **constant 99** -- a height/altitude reference, not a coordinate
the player moves along. It never changed across the X and Y hunts. The disc's
`vel_y` homes on `$6ca4 - $10` = 83, and the observed disc `world_y` converges
81 -> 82 -> 83. That is the confirmation: the disc rises to the player's
height, while `$6ca6` (18 / 25 / 2) is the walkable row that selects the near
or far half of the floor grid.

```
$6ca4  player_height_ref  (word)  constant 99; disc vel_y homes on this - $10
$6ca6  player_y           (word)  the walkable row; > 14 = far row
```

So: player movement and the grid cell use `+$06`; disc vertical homing uses
`+$04`. A core that steers the disc at `+$06` will diverge from the trace.

## The disc flight cycle (Part 9) -- resolves the "freeze" and the "turnarounds"

Segmenting `tests/fixtures/tile_damage.ndjson` by `(dir_kind, world_z step)`
shows disc 0 running a four-phase cycle, twice over, with hard boundaries:

```
f0  ..34    dir_kind +1   z +1    wz 20 -> 54     outbound
f35 ..51    dir_kind +1   z  0    wz 54           DWELL, whole record frozen
f52 ..52    dir_kind -3   z -1    wz 53           turn
f53 ..69    dir_kind -3   z -3    wz 50 -> 2      return, three times faster
f70 ..70    dir_kind +1   z -2    wz 0            turn
f71 ..124   dir_kind +1   z +1    wz 1 -> 54      outbound again
f125..151   dir_kind +1   z  0    wz 54           dwell again
```

So:

* `world_z` runs between **0 and 54** and the sign of `dir_kind` says which way;
* the magnitude of `dir_kind` **is** the z step -- outbound `+1`, return `-3`,
  so the disc comes back three times faster than it goes out;
* the **"freeze" is a dwell at the far end** (`wz` = 54), about 17 frames, with
  the entire record static including `world_x`. That is bd discr-0fm, and it is
  a phase of the cycle rather than an anomaly;
* what `disc` read as "upper turnarounds at `world_x` 45 and 113" were these
  dwells. They differ because `world_x` simply stops wherever it had got to.

`world_x` keeps integrating by `vel_x` during outbound and return, and holds
during the dwell. The one-step `vel_x` decay on the frame the dwell begins
(2 -> 1 at f34) is not explained.

Note the racked disc of the earlier notes sits at world `(140, 53)` -- the same
depth this cycle turns at. The rack is the far end of the run.

## THE SERVE, decoded (Part 9)

Found with Hatari's instruction history (`history cpu 60` + `lock history 40`
on a breakpoint at `$a9b4`), which showed the whole path in one hit.

**`$a972` is the serve routine.** It claims the sound-effect slot, points USP
at sample `$66f16` and starts Timer A, then fills the first free disc slot:

```
$a972  cmp.b #$01,$6c5c        ; sfx priority
$a97a  move.b #$01,$6c5c
$a982  lea.l  $00066f16,a0     ; the serve sample
$a988  st.b   $6c5b
$a98c  move.l a0,usp           ; Timer A's stream cursor
$a98e  move.b #$7c,$fffa1f     ; TADR
$a994  move.b #$01,$fffa19     ; TACR -- start it
$a99c  lea.l  $6e3e.w,a1       ; the disc array
$a9a0  moveq  #$07,d3          ; 8 slots
$a9a2  tst.b  ($0010,a1)       ; free?
$a9a6  bne.w  $aa46            ;   no -> next slot
$a9aa  addq.w #$01,$6d8a
$a9ae  move.l d0,(a1)          ; +$00 world_x : +$02 world_y
$a9b0  move.l d1,($0004,a1)    ; +$04 world_z : +$06 vel_x
$a9b4  move.l d2,($0008,a1)    ; +$08 vel_y   : +$0a dir_kind
```

**Its caller is `$c06e`-`$c0fa`, and the serve is PLAYER 2's action.** Every
parameter comes from player 2:

```
$c07a  move.w $6d22,d0 ; sub.w #9 ; swap ; move.w #$0051,d0
                                 -> world_x = p2.x - 9,  world_y = $51 = 81
$c088  move.w $6d26,d1 ; subq #1 ; swap ; (later) clr.w d1
                                 -> world_z = p2.y - 1,  vel_x = 0
$c090  move.w $6d8e,d2 ; swap ; clr.w d2
                                 -> vel_y = $6d8e,       dir_kind = 0
$c094  cmp.w #$0002,$6d9a        ; the damage-multiplier variable, again
$c0b0  btst.b #2,(a0) -> $c0ce   ; p2 LEFT  : subq #1,d1 (twice if d2 = -1)
$c0b8  btst.b #3,(a0) -> $c0e6   ; p2 RIGHT : addq #1,d1 (twice if d2 = -1)
$c0c0  bsr.w  $a972              ; straight
$c0c4  move.b #$11,$6d2e         ; p2 state_index := $11 = 17
```

So the throw direction is **player 2's own left/right input**: straight gives
`vel_x` 0, left -1 or -2, right +1 or +2. `world_y` = 81 is exactly the value
every served disc has been observed to carry.

`$c0c4` settles a loose end: the serve **sets player 2's `state_index` to 17**.
The n=2 correlation between the dwell exit and p2 entering state 17 was not a
correlation at all -- it is the same instruction sequence, three lines apart.

### Where the served `dir_kind` comes from

The register shuffle is easy to misread -- there are **two** swaps, and
stopping at the first one inverts the answer:

```
$c090  move.w $6d8e,d2        ; d2.low  = $6d8e            (reads -3)
$c094  cmp.w  #$0002,$6d9a
$c09a  bne.b  $c0a0
$c09c  move.w #$fffb,d2       ; $6d9a == 2 -> -5 instead
$c0a0  swap.w d2              ; d2.high = that value
$c0a2  clr.w  d2              ; d2.low  = 0
$c0a4  btst.b #0,(a0)
$c0a8  beq.b  $c0ac
$c0aa  subq.w #$05,d2         ; p2 flag bit 0 -> d2.low = -5
$c0ac  swap.w d2              ; SWAP BACK
```

After the second swap `d2.low` is the `$6d8e` value again and `d2.high` is 0
or -5, so `move.l d2,($0008,a1)` lands:

```
+$08 vel_y    = d2.high = 0, or -5 when player 2's flag bit 0 is set
+$0a dir_kind = d2.low  = $6d8e, or -5 when $6d9a == 2
```

which is exactly what the traces show -- `vel_y` 0 and `dir_kind` -3 on every
served disc.

**So `dir_kind = -3` is not a constant in the code: it is the contents of
`$6d8e`**, which reads -3 for the whole of the idle reference and never moves
(bd discr-qqt). Given the magnitude is the per-frame `world_z` step, `$6d8e` plausibly
sets how fast a served disc travels -- a rank or difficulty knob.

One more detail from the direction branches: `$c0d0`/`$c0e8` compare `d2` with
`#$ffff` and double the `vel_x` adjustment when it matches, so a `dir_kind` of
-1 is served with twice the sideways speed of a -3.

Note `$6d9a` appears here too, tested against 2, having already appeared in the
tile-damage path tested against 1 and 3 (bd discr-z8m). Whatever it is, it
modulates both damage and serves.

## The dwell exit IS a serve (Part 9) -- two unknowns collapse into one

Watching `$6e48` (disc 0's `dir_kind`) in Hatari over 180 in-match frames gives
three writers, **twice each**, and they map one-for-one onto the three turn
events of the flight cycle:

| PC | instruction | fires | which turn |
|---|---|---|---|
| `$a9b8` | after `$a9b4 move.l d2,($0008,a1)` in the spawn routine `$a9a0` | 2 | the **dwell exit** |
| `$a61e` | after `$a618 move.w #$0001,($000a,a5)` | 2 | the near-bound turn (`wz` 0) |
| `$a60a` | after `$a606 neg.w ($000a,a5)` | 2 | the `world_x` floor flip |

The dwell exit is not a sign flip -- `dir_kind` goes `+1` to `-3`, and `neg.w`
of `+1` is `-1`, so it never could be. It is the **spawn routine rewriting the
whole record**, which also explains the rest of that frame: `world_x` jumps
45 -> 48 with `vel_x` dropping to 0, which is a rewrite, not an integration.

**So "what ends the dwell" and "what triggers a serve" are the same question.**
The disc is not turning round at the far end; it is being served again from
there, which is consistent with the rack sitting at world `(140, 53)` -- the
far end of the run. bd discr-fnl folds into bd discr-m4x: find `$a9a0`'s
caller and both are answered.

The n=2 correlation with player 2 entering `state_index` 17 now reads as the
opponent *playing* the disc, which is what a serve is.

**Careful with the entry point.** A breakpoint on `$a9a0` did **not** fire in a
180-frame window where `$a9b8` wrote `dir_kind` twice, and `find l $0-$18000
$a9a0` finds no absolute reference. `$a9a0` is only the loop *setup*
(`moveq #$07,d3`); `$aa4a dbf d3,$a9a2` loops back to **`$a9a2`**, which is the
body -- it tests `($0010,a1)` and skips busy slots, so it is scanning the 8
records for a free one. The real entry is above `$a9a0` and is not yet
identified, so "the spawn routine `$a9a0`" in the entries above should be read
as "the slot-fill loop whose body is `$a9a2`". Finding its caller is bd
discr-m4x.

## The tile type word: gate polarity confirmed, second writer found (Part 9)

The walkability gate polarity is settled by `$f634`-`$f64e`:

```
$f63e  tst.w ($00,a1,d0.w)   ; the cell's TYPE word
$f642  bne.b $f648           ; type != 0 -> d0 = $ff
$f644  moveq #$00,d0         ; type == 0 -> d0 = 0
$f64a  bne.w $f650           ; d0 != 0 skips the next instruction
$f64e  st.b  d2              ; ...so type == 0 SETS the blocked flag
```

So **type 0 blocks and non-zero is walkable** -- read it as tile presence, not
occupancy. A hole, not a person standing there.

But something else clears it. In the idle Hatari reference, inside the
275-frame validated window:

| frame | cell | change | |
|---|---|---|---|
| 65 | 6 | `(1,1) -> (0,0)` | destroyed by `$a354` |
| **114** | **14** | **`(1,1) -> (0,1)`** | **type cleared, hp still 1 -- NOT `$a354`** |
| 165 | 7 | `(2,4) -> (2,1)` | damaged -3 |
| 203 | 8 | `(1,4) -> (1,1)` | damaged -3 |
| 273 | 7 | `(2,1) -> (0,0)` | destroyed by `$a354` |

`$a354` only clears the type *after* hp reaches 0, so frame 114 has a second
writer punching a hole with hp intact (bd discr-b4q).

Finding these at all required widening the memdump window: at
`nMemdumpLines = 200` it stopped at `$767f` and cells **13-16 were never
compared** -- the far row, where the player stands (idle cell 15). The differ
skipped the absent bytes silently. It now asserts coverage.

## Two corrections from trace comparison (Part 9)

Both were found by replaying an oracle trace through the Rust core and asking
where it disagreed. Neither was visible from the disassembly alone.

### `$6ca9` is the previous state, not facing

The earlier note read "1 = left, 2 = right; set at `$f5e2`/`$f7f6`". That was an
over-reading: those two handlers are states 1 and 2, so "facing" and "the state
we were just in" take the same values and cannot be told apart from them alone.

In `tests/fixtures/golden.ndjson`, `+$09` takes **exactly the same value set as
`state_index`** -- player 1 {0,1,11,20,23}, player 2 {0,1,2,15,16,18,20} -- and
equals the previous frame's state on **96 of 99** frame pairs:

```
frame  11(0,20)  12(20,20)  13(20,20)  14(20,1)  15(1,1)      (+$09, state_index)
```

At frame 11 the state becomes 20 while `+$09` is still 0; at frame 12 `+$09`
becomes 20. It lags by one frame. Read it as `player_prev_state`.

### `world_z` advances by `dir_kind`, not by a constant +1

The earlier note said "+1 per frame while in flight". That is the `dir_kind = +1`
case. Over the fixture:

| `dir_kind` | `world_z` step | frames |
|---|---|---|
| +1 | +1 | 68 |
| +1 | 0 | 19 (a full-record freeze) |
| -3 | -3 | 11 |
| -3 | -1 | 1 |

So the step is the **`dir_kind` value itself**, which sharpens what that field
is: its sign is the travel direction *and* its magnitude is the per-frame z
step. The freeze frames and the single -1 are not explained -- see bd discr-0fm.

## The steering rule, literally (Part 9)

`$a722`-`$a758`, three cases and no others. `d5` is the aim point, `d0` the
disc's coordinate, `($0006,a5)` the velocity:

```
$a722  cmp.w d0,d5
$a724  bgt -> $a74c   aim > pos:  if vel < +2 then vel += 1     (clamp +2)
$a726  blt -> $a73c   aim < pos:  if vel > -2 then vel -= 1     (clamp -2)
       else  $a728    aim == pos: vel decays TOWARD ZERO by 1
                        $a72c bmi -> $a736 addq  (vel < 0: += 1)
                        $a72e beq -> done        (vel == 0: nothing)
                        $a730      subq          (vel > 0: -= 1)
```

The at-target decay is the whole of the damping. There is **no** gap limiting
and no proportional term: the velocity is a bounded integer nudged one step
per frame, and it unwinds only once the disc is level with the aim point.

## RETRACTED: "the steering block is gated off" -- it aims at PLAYER 2 (Part 9)

An earlier revision of this file claimed the `$a71a` steering block never
fires, on the evidence that with the aim at `$6ca2 - $13` = 98 the rule would
increment `vel_x` on eleven consecutive frames where the ST holds it at -2.
**That was the wrong conclusion from a correct observation.** The block runs;
it is simply not aimed at player 1.

Re-tested against `tests/fixtures/tile_damage.ndjson` with the `$a816` aim,
`$6d22 - $13` (player TWO's X):

```
frames 12..34:  23 of 23 velocity transitions predicted exactly
```

and frame 34 is the at-target decay itself -- p2 X 63, aim 44, disc at 44,
`vel_x` 2 -> 1 -- which is the `$a728` case, not an anomaly. The "unexplained
one-step decay at the dwell" recorded above is explained: it is the steering
rule reaching its target.

`$a7d8` (`$6d22 - 4`) fits a different stretch of the same run, f99..f124,
exactly. So **both player-2 aim variants are live within one round** and the
open question is which is selected when -- not whether the block runs.

Where it does NOT fit:

* **f1..f11**, the descent to the near bound: aim 44 sits above the disc, so
  the rule says raise `vel_x`, and the ST holds it at -2 until the bound flips
  it. The bound governs, not the steering.
* **the dwell** (f35 on): the whole record is frozen, so nothing updates.

The lesson is worth keeping: testing one aim variant and concluding "the block
is gated off" was a claim about the code drawn from a single hypothesis about
its input. bd discr-217 is retargeted from "what gates it" to "which aim
variant is selected when".

## vel_y is inert in all the evidence we have (Part 9)

Do not model vertical motion as `world_y += vel_y`. In `dumps/disc_trace`
(84 frames) **`vel_y` (+$08) is 0 on every single frame**, while `world_y`
(+$02) changes on 3 frame pairs only, 81 -> 82 -> 83. So:

* `world_x` is integrated by `vel_x` -- verified 47/48 in flight;
* `world_y` is **not** integrated by `vel_y`, and whatever advances it is
  unknown (bd discr-tan);
* the `$a758` vertical steering block never fired for that disc, so its gate
  is unknown too.

A core that integrates `world_y` by `vel_y` will overshoot the aim point and
oscillate. That is a symptom of modelling a rule the evidence does not show,
not a missing damping term.

## Player states validated tier 1 (Part 8)

Handlers from the `$10e2c` jump table; each seen inside a window where the
oracle and Hatari agree byte-for-byte.

```
$6cae = 1   walk left       $f5e2      $6cae = 2   walk right      $f7f6
$6cae = 5   $fb6e (tests fire, btst #7,(a0) at $fb74)
$6cae = 14  $106b2   entered under Right+Fire; idle play never reaches it
$6cae = 20  $1094a   transient entered when walking starts
$6cae = 21  $109aa   $6cae = 24  $10ac4   $6cae = 27  $10c8a
```

```
$6cae = 11  $10554   $6cae = 19  $108f4
$6cae = 23  $10a72   $6cae = 31  $10dda
```

States 16 and 17 are still NOT recorded: they have only ever been seen in an
oracle autopilot run, never in Hatari. See reports/exploration-report.md.

**Grading rule (corrected).** Earlier notes said notes-grade required a
differ-validated window. That conflated two questions. A differ window tests
whether *the oracle* is faithful; it says nothing extra about the game. A state
seen in a **Hatari** memdump trace is the game doing it, and Hatari is the
reference. So: observed in Hatari -> notes-grade. Observed only in the oracle
-> needs a validated window before it can be promoted.

## Ghidra bookmark set

```
$f5d0   player state dispatch (jump table at $10e2c)
$f5e2   state 1 -- walk left      $f658  subq.w #3,$6ca2
$f7f6   state 2 -- walk right     $f86c  addq.w #3,$6ca2
$fe7a   dominant writer of $6ca6 (player Y)
$a4ea   disc update loop entry (lea $6e3e,a5)
$a6b2   disc perspective projection -> screen X/Y
$a722   disc X-velocity steering (clamped [-2,+2])
$aa50   round initialiser: 8 disc records + their sub-records
$8198   VBL handler (the game's per-frame entry point)
$8370   IKBD ACIA handler / joystick decode
$83d2   Timer A PSG streamer; $83fe is its end-of-stream path
$a6c2   disc engine sound trigger (sets $6c5b/$6c5c, arms Timer A)
$a606   disc turn-around: neg.w on the owner/direction field
$a9a0   disc spawn/serve (stores the record, bumps $6d8a)
$a34c   tile damage store; $a354 destroys the cell
$a71a   disc steering, homes on a player's coordinates
```
