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
$6ca9  player_facing  (byte)  1 = left, 2 = right; set at $f5e2/$f7f6
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
```
