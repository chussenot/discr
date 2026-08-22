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
  +$0e  grid cell index  8 + column(X) + (4 if Y > 14)

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
```
