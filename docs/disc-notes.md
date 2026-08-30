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

**RETRACTED by Part 10c -- this had the branch backwards.** The full sequence is

```
$c0e6  addq.w #1,d1
$c0e8  cmp.w  #-1,d2
$c0ec  beq.b  $c0f0        ; d2 == -1 SKIPS the second addq
$c0ee  addq.w #1,d1
$c0f0  bsr    $a972
```

so the `beq` is a skip, not a doubling: **a `dir_kind` of -1 gets the single
sideways step and every other kind gets two.** The traces settle it -- golden
frame 52 serves a `dir_kind` -3 disc with the joystick's right bit set and
`vel_x` reads +2, which the "doubled only when -1" reading cannot produce.

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

## RETRACTED by Part 10: "vel_y is inert in all the evidence we have" (Part 9)

**This section is wrong and is kept for the record.** `$a556 add.w ($08,a5),d1`
integrates `world_y` by `vel_y` unconditionally, and `$a640` decays `vel_y`
toward zero *after* the integration -- so a one-frame impulse is invisible at
the sampling point. See "RETRACTED: vel_y is inert" under the `$a4ea` section
above. What follows is the original text.

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

## How Part 10 was read: Ghidra headless over a RAM image

Everything in the Part 10 sections below came out of Ghidra 12.1.3 running
headless over `seeds/rally_f100.seed` — the 1 MB RAM image of a live match —
loaded as a raw `68000:BE:32` binary at address 0 and seeded with the entry
points this file already knew. `scripts/ghidra/` has the harness; three
commands do the work:

```
scripts/ghidra/import.sh              # one-off: import + analyse (about 90 s)
scripts/ghidra/q.sh xref 6c59         # every reference to an address
scripts/ghidra/q.sh scan a7d8         # every instruction with that operand
scripts/ghidra/q.sh dis a4ea 60       # 60 instructions from there
```

Seeding 70 entry points was enough for auto-analysis to reach the whole game
loop by following calls. `xref` and `scan` differ in an important way: `xref`
uses Ghidra's reference model, `scan` walks every disassembled instruction and
matches operands literally, so it finds `move.l #$a7d8,(a5+$12)` — an address
used as *data* — which the reference model files elsewhere.

**Two rules were kept.** A static read is a hypothesis until a trace shows it,
so each finding below is tagged either **[code+trace]** (the oracle run named
in the section confirms it) or **[code only]** (the instruction is there, no
trace reaches it yet). And retractions are written down rather than edited out.

The oracle grew columns for the fields this phase found — `vy`, `dk` (the
signed `+$0a`), `act`, `own`, `hook`, `dmg` per disc, and `joy_6c59`,
`ai_6da1`, `mode_6da0`, `bonus_6d9a` per frame. The reference run cited below
is 240 frames from `seeds/rally_f100.seed`, whose first 116 frames are inside
the window where that seed was verified against Hatari.

## The control-mode dispatcher: where player 2's input comes from (Part 10)

`$10eac` — immediately after the state jump table — chooses who drives whom.
**[code+trace]**

```
$10eac  tst.b $6da0            ; != 0 -> ONE PLAYER
$10eb0  beq.b $10ed4
$10eb2  tst.b $6cab ; bpl      ; player 1 active?  (bit 7)
$10eb8  lea $6c58,a0 ; bsr $f104      ; p1 = human on joystick 0
$10ec0  tst.b $6d2b ; bpl      ; player 2 active?
$10ec6  bsr $d2cc                     ; RUN THE AI
$10eca  lea $6da1,a0 ; bsr $abb2      ; p2 = the byte the AI just wrote
$10ed2  rts

$10ed4  tst.b $6c97 ; bne $10ef8      ; two players, joysticks swapped?
$10eda  lea $6c58,a0 ; bsr $f104      ; p1 joystick 0
$10ee8  lea $6c59,a0 ; bsr $abb2      ; p2 joystick 1
$10ef8  lea $6c59,a0 ; bsr $f104      ; swapped: p1 joystick 1
$10f0c  lea $6c58,a0 ; bsr $abb2      ;          p2 joystick 0
```

So:

* **`$6da0`** selects one-player mode. It is `$ff` in `rally_f100.seed`.
* **`$6c97`** swaps the two joysticks in two-player mode.
* **`$6cab` (p1+$0b) and `$6d2b` (p2+$0b)** gate control per player, tested
  `tst.b` + `bpl` — **bit 7 set means "may act"**. They are counters, not
  flags: `$10aba addq.b #3,$6cab` (state 23's handler) and `$12d36 subq.b
  #1,$6cab`, mirrored for p2 at `$c3b2`/`$14078`. Read them as a recovery
  timer whose sign bit is the lockout.
* **`$f104` is player 1's control routine and `$abb2` is player 2's.** They are
  different code, not one routine parameterised by player.
* **`$6da1` is the byte the AI synthesises, and it is fed to `$abb2` at exactly
  the position a human joystick byte occupies.** So the AI has no privileged
  channel: it presses the same five bits a person does.

The 240-frame reference run confirms it end to end: `mode_6da0` is 255 on every
frame, **`joy_6c59` is 0 on every frame** — player 2's real joystick byte is
never written in one-player mode — and `ai_6da1` takes ten distinct values,
all of them joystick bit patterns:

```
0    2 (down)   4 (left)   5 (up|left)   6 (down|left)   8 (right)
129 (fire|up)   133 (fire|up|left)   136 (fire|right)   137 (fire|up|right)
```

That answers the architecture half of bd discr-b6x: **whatever writes `$6da1`
IS the opponent, and it is `$d2cc`.**

## The AI is a priority rule table with a random gate (Part 10)

`$d2cc`, in full. **[code+trace]**

```
$d2cc  lea $6da1,a0 ; clr.b (a0)      ; start from "no buttons"
$d2d2  bsr $cea6                      ; sensor pass (not yet decoded)
$d2d6  movea.l $6da2,a6               ; a6 = the rule table
$d2da  tst.w (a6) ; beq $d324         ; word 0 terminates
$d2de  clr.w d6 ; clr.w d2
$d2e2  move.b (a6)+,d6                ; priority
$d2e4  move.b (a6)+,d2                ; random threshold
$d2e6  movea.l (a6)+,a2               ; TEST   routine
$d2e8  movea.l (a6)+,a3               ; ACTION routine
$d2ea  movea.l (a6)+,a4               ; BEHAVIOUR identity / continuation
$d2ec  cmpa.l $6da6,a4  ; beq skip    ; already doing this -> skip
$d2f2  cmp.w  $6daa,d6  ; ble skip    ; priority <= current -> skip
$d2fa  move.b $6c5d,d0
$d2fe  add.b  $6ab5,d0                ; += low byte of the frame counter
$d302  move.b d0,$6c5d                ; the PRNG is one accumulating byte
$d306  cmp.w  d2,d0     ; bgt skip    ; random > threshold -> skip
$d30a  jsr (a2)                       ; TEST; d0 == 0 means "fires"
$d30c  tst.w d0 ; bne skip
$d310  lea $6dac,a1 ; move.l a1,$6dfc
$d318  jsr (a3)                       ; ACTION -- writes bits into $6da1
$d31a  move.l a4,$6da6                ; remember the behaviour
$d31e  move.w d6,$6daa                ; and its priority
       bra $d2da                      ; next entry
$d324  tst.l $6da6 ; beq rts
$d32a  movea.l $6dfc,a1 ; movea.l $6da6,a2 ; jsr (a2)   ; else CONTINUE
$d334  rts
```

Each table entry is **14 bytes**: `byte priority, byte threshold, long test,
long action, long identity`. A word of 0 ends the table. `$6da2` holds the
table base — nothing in the disassembled image writes it, so the initialiser
that does has not been found; in `rally_f100.seed` it is **`$efa8`**, and the
table there has **20 entries**:

```
 #  entry     prio thresh  test     action   identity
 0  $efa8      50   255    $e0d8    $e214    $e290
 1  $efb6      30   255    $e158    $e214    $e290
 2  $efc4      20   200    $d4ea    $d6a2    $e222
 3  $efd2      20   150    $d554    $d672    $e222
 4  $efe0      10   100    $d5fe    $d672    $e222
 5  $efee      12   230    $d6b4    $d6da    $e244
 6  $effc      10    90    $dd68    $deea    $e274
 7  $f00a      10    90    $dd68    $df58    $e274
 8  $f018      10    90    $de8e    $deea    $e274
 9  $f026      10    90    $de8e    $df58    $e274
10  $f034      10    90    $de12    $deea    $e274
11  $f042      10    90    $de12    $df58    $e274
12  $f050      10    90    $ddd4    $deea    $e274
13  $f05e      10    90    $ddd4    $df58    $e274
14  $f06c      10    90    $ddc4    $deea    $e274
15  $f07a      10    90    $ddc4    $df58    $e274
16  $f088      10    90    $da84    $deea    $e274
17  $f096      10   100    $da84    $df58    $e274
18  $f0a4       9    60    $da04    $df1c    $e274
19  $f0b2       8    50    $dff6    $e04a    $e290
20  $f0c0      terminator
```

Structural readings that follow directly:

* **Priority is a latch, not a sort key.** `$6daa` holds the priority of the
  behaviour currently running and a rule only fires if it *beats* that number.
  So a committed behaviour cannot be interrupted by an equal or lower one, and
  the four routines that `clr.l $6da6` / `clr.w $6daa` (`$e236`, `$e266`,
  `$e282`, `$e29e` — the identity routines themselves) are how a behaviour
  releases the latch when it is finished.
* **The threshold is a per-rule reaction probability out of 255.** Entries 0
  and 1 (priorities 50 and 30) are 255 — unconditional, so they are the
  reflexes. The eight pairs at priority 10 are 90/255 ≈ 35%, and the two
  lowest are 60 and 50. That is where "the AI gets tougher" most plausibly
  lives, and it is a table in RAM, so a per-rank table is exactly the shape the
  code would take. **Not yet demonstrated: only one table has been seen.**
* **The PRNG is `$6c5d += $6ab5` — the accumulating low byte of the frame
  counter.** It is shared: `$d07a`, `$da0e` and `$df8c` advance the same byte.
  So the AI's randomness is a deterministic function of frame history, which is
  why the oracle reproduces it at all.
* **Rules 6..17 are eight tests each paired with two alternative actions**
  (`$deea` and `$df58`), same priority and same threshold, consecutive in the
  table. The first to pass its random roll wins, so the pair is a coin flip
  between two responses to the same situation.

What is **not** decoded yet: the 11 distinct test routines, the 7 action
routines, `$cea6`'s sensor pass, and what `$6dac`/`$6dfc` carry. bd discr-b6x
stays open for those, retargeted from "find the AI" to "name each rule".

## The disc update loop `$a4ea`, in full (Part 10)

This is the routine `disc-core`'s `disc::step` mirrors, and reading it resolves
five open beads at once. **[code+trace]**

```
$a4ea  lea $6e3e,a5 ; moveq #7,d3          ; 8 records, stride $42
$a4f0  tst.b ($10,a5) ; beq next           ; +$10 == 0  -> slot is FREE
       ... copies the sprite record through +$1a / +$3e ...
$a534  tst.b ($10,a5) ; bpl next           ; +$10 bit 7 clear -> do not simulate
$a53c  move.w (a5),d0                      ; d0 = world_x  (+$00)
$a53e  move.w ($02,a5),d1                  ; d1 = world_y  (+$02)
$a542  move.w ($04,a5),d2                  ; d2 = world_z  (+$04)
$a546  tst.l ($12,a5) ; beq $a552
$a54c  movea.l ($12,a5),a0 ; jsr (a0)      ; the per-disc HOOK
$a552  add.w ($06,a5),d0                   ; world_x += vel_x
$a556  add.w ($08,a5),d1                   ; world_y += vel_y
$a55a  add.w ($0a,a5),d2                   ; world_z += dir_kind
$a55e  tst.b ($11,a5)                      ; the OWNER byte
       ... per-owner housekeeping on $6d8a/$6d0a ...
$a58e  cmp.w #$9b,d0 ; ble $a5a6           ; x > 155
$a594    clr.l ($12,a5) ; neg.w ($06,a5) ; d0 = $9b ; d4 = d2 ; a1 = $4f00
$a5a6  tst.w d0 ; bpl $a5ba                ; x < 0
$a5aa    clr.l ($12,a5) ; neg.w ($06,a5) ; d0 = 0   ; d4 = d2 ; a1 = $4f00
$a5ba  cmp.w #$4f,d2 ; ble $a5fe           ; z > 79  -- the FAR wall
$a5c0    clr.l ($12,a5) ; neg.w ($0a,a5) ; d2 = $4f ; d4 = d2 ; a1 = $4eb6
$a5d0    tst.b ($11,a5)
$a5d6      owner != 0: dir_kind := -1 ; bsr $9f5e      ; far tile grid
$a5e2      owner == 0: if $6ca0.b != 1 -> st ($11,a5)
                       $6d8a-- $6d8c-- $6d0c++ $6d0a++
$a5fe  tst.w d2 ; bpl $a640                ; z < 0   -- the NEAR wall
$a602    clr.l ($12,a5) ; neg.w ($0a,a5) ; d2 = 0   ; d4 = d2 ; a1 = $4f4a
$a612    tst.b ($11,a5)
$a618      owner == 0: dir_kind := +1 ; bsr $a24c      ; near tile grid
$a624      owner != 0: if $6ca0.b != 1 -> clr.b ($11,a5)
                       $6d0a-- $6d0c-- $6d8c++ $6d8a++
$a640  tst.w ($08,a5)                      ; VEL_Y DECAYS TOWARD ZERO
$a644  beq $a652
$a646  bmi $a64e -> addq.w #1,($08,a5)
$a648            -> subq.w #1,($08,a5)
$a652  bsr $10fd8                          ; player 1's hit test
$a656  bsr $c826                           ; player 2's hit test
$a65a  bsr $9b3e
$a65e  move.w d0,(a5) ; move.w d1,($02,a5) ; move.w d2,($04,a5)   ; write back
$a668  ... perspective projection -> +$0c / +$0e ...
$a6ba  tst.w d4 ; bmi done                 ; d4 >= 0 -> a bound was hit
$a6be  bsr $aae8 ; ... play the sample at a1
```

### `disc+$10` is the active byte and `disc+$11` the owner — un-waive both

`+$10` is tested twice: `beq` for "free" and `bpl` for "simulate", so it is a
byte whose **bit 7 means live**. It reads `$ff` on both live slots and `0` on
all six free ones across the whole reference run, and `$a9a2 tst.b ($10,a1)` in
the slot-fill loop is the same test. `+$11` reads `0` on both live slots and is
the byte `$a55e`, `$a5d0` and `$a612` branch on.

So `discs[n].active` and `discs[n].aim` are **mirrored ST fields, not models**,
and the "the ST encoding of an unused slot is unknown" note in
`docs/state-schema.md` is wrong. That half of bd discr-m4x is closed.

The owner polarity is *not* settled. At the far wall an owner of 0 becomes `$ff`
and simultaneously `$6d8a--`, `$6d8c--`, `$6d0c++`, `$6d0a++`; at the near wall
`$ff` becomes 0 with the four counters moving the other way. Which of the two
values names which player is a guess until a trace shows a disc changing hands,
and no trace does yet — 240 frames of `own` are all 0.

### The bounds: `world_x` in 0..155, `world_z` in 0..79 — [code only]

Four bounds, each with the same three-part response: **clear the hook, negate
the velocity, clamp the coordinate, and set `d4` so a sound plays.** So the
"floor at `world_x == 0` that sign-flips `vel_x`" that `disc-core` inferred from
golden frames 10-12 is the real rule, and the **ceiling it deliberately did not
invent is at `$9b` = 155** (bd discr-1q7). The z bounds are 0 and `$4f` = 79.

`world_z` was observed running 0..54, not 0..79, so the upper z bound is a code
read with no trace behind it. `world_x` reaches 151 in the seed, never 155.

### RETRACTED: "vel_y is inert -- do not model `world_y += vel_y`"

The Part 9 section above says a core that integrates `world_y` by `vel_y` is
"modelling a rule the evidence does not show". **`$a556 add.w ($08,a5),d1` is
that rule, and it is unconditional.** The observation behind the retracted claim
was correct — `vel_y` is 0 at every sampling point — and the inference from it
was wrong, for a reason the code makes obvious:

**`vel_y` is decayed toward zero by 1 at `$a640`, *after* the integration and
before the sample.** A single-frame impulse of +1 therefore moves `world_y` by 1
and is back to 0 by the time the VBL handler is reached. It is structurally
invisible at the sampling point.

The reference run shows it directly. At frames 113-115 the hook `$a816` is
installed on disc 0 and `world_y` goes 81 -> 82 -> 83 while `vy` samples 0 on
every one of those frames:

```
frame  hook   wy  vy
  113  a816   81   0
  114  a816   82   0
  115  a816   83   0
  116  a816   83   0
```

So bd discr-tan's answer is `$a556`, `world_y += vel_y`, with the caveat that
**what sets `vel_y` is the hook, and the hooks are not fully decoded** — the
81->82->83 pattern is consistent with the hook writing +1 on three frames, and
that specific claim is still an inference.

### bd discr-217 answered: the steering "gate" is the hook pointer `disc+$12`

`disc+$12` is a longword function pointer, called at `$a54c` before the
integration and **cleared by every one of the four bounds** and by `$a276` in
the tile-damage path. Three routines are ever installed in it, and `scan` finds
every site:

| installed | by | inside |
|---|---|---|
| `$a71a` (aims via `$6ca2`, player 1's X) | `$113e2` | `$10fd8`, **player 1's hit test** |
| `$a7d8` (aims via `$6d22 - 4`) | `$cb70`, `$cbae` | `$c826`, **player 2's hit test** |
| `$a816` (aims via `$6d22 - $13`) | `$cc1e` | `$c826`, player 2's hit test |

That is the whole selector. The steering block is not gated by a flag: **it runs
exactly while a hook is installed, a player's hit test installs it, and any wall
clears it.** Which of `$a7d8` and `$a816` p2's test installs depends on where in
`$c826`'s cascade of range checks the disc was caught — `$cb70` is the first
`vel_x`-side branch, `$cc1e` a later one.

The reference run shows `hook` taking `0`, `$a7d8` and `$a816` on disc 0 within
120 frames, which is why the Part 9 retraction found `$a816` fitting one stretch
and `$a7d8` another: **both are live in one round because each is installed by a
different catch.**

### bd discr-5w5 answered: the collision test is `world_z` crossing a wall

There is no geometric disc-vs-tile test. The disc reaches a wall — `z < 0` or
`z > 79` — and the wall routine decides which cell it struck from the disc's
`(world_x, world_y)`:

```
$a24c  lea $7bfe,a0
$a250  move.b ($04,a0,d0.w),d5      ; d5 = colTable[$7c02 + world_x]
$a254  cmp.w #$46,d1 ; ble $a25c
$a25a  addq.b #4,d5                 ; world_y > 70 -> the far row
$a25c  ext.w d5 ; lsl.w #3,d5       ; x8 -- the tile stride
$a260  lea $7616,a0                 ; THE NEAR GRID
$a264  move.w ($02,a0,d5),d6        ; the cell's HP word
```

So `d5` is `colTable[$7c02 + world_x]`, plus 4 when `world_y > $46`, times 8.
The same `$7bfe` table the player's `grid_cell` uses, read at offset +4 rather
than +0 — and the row threshold is `$46` = 70, not the player code's 14,
because a disc's `world_y` runs around 81 while a player's runs 18/25.

**There are two tile grids.** `$9f5e` is `$a24c` instruction-for-instruction
with two substitutions: `lea $7596,a0` instead of `$7616`, and `$6d1c` instead
of `$6d9a`. So `$7596` is the far wall's grid and `$7616` the near one, and each
player has their own bonus-code word. `disc-core` and the differ have only ever
looked at `$7616`. **[code only]** — no trace has covered `$7596`.

## `$6d9a` is the active bonus code, NOT a difficulty rank (Part 10)

The standing hypothesis was that `$6d9a` tested against 1, 2 and 3 was the
interview's three ranks. **It is refuted, and by one instruction:**

```
$8240  tst.w $6d9c ; beq $8250
$8246  subq.w #1,$6d9c            ; a countdown, every VBL
$824a  bne $8250
$824c  clr.w $6d9a                ; timer expired -> the code goes away
```

A per-match rank is not decremented to zero by the VBL handler. `$6d9a` is a
**timed effect currently in force**, and `$a24c` is where one is picked up:

```
$a292  andi.w #$80,d6 ; beq $a30c     ; bit 7 of the cell's HP word?
$a298  move.w ($02,a0,d5),d6
$a29c  andi.w #$0f,d6
$a2a0  move.w d6,($02,a0,d5)          ; strip bit 7 -- the cell is spent
$a2a8  bsr $9956
$a2ac  move.w $6e3a,d0
$a2b0  move.w d0,$6d9a                ; the code
$a2b4  subq.w #1,d0 ; lsl.w #2,d0 ; lea $9aa2,a0
$a2be  move.w (a0,d0),$6d9e           ; its payload
$a2c4  move.w ($02,a0,d0),$6d9c       ; its duration in frames
$a2ca  clr.w $6e3a ; clr.w $6e38
```

The table at `$9aa2`, four bytes per code, as it reads in the image:

| code | `$6d9e` | `$6d9c` (frames) | what the code gates |
|---|---|---|---|
| 1 | 5 | 0 (no timer) | `$a314 cmpi.w #1` applies the disc's `+$16` damage a second time |
| 2 | 0 | 500 (~10 s) | `$c09c` serves with `dir_kind` = -5 instead of `$6d8e` |
| 3 | 3 | 0 (no timer) | `$a32e`'s second damage path; also read by `$d0a0`/`$d0ca`/`$d158` in the AI |
| 4 | 0 | 1000 (~20 s) | `$c9b4`, `$c9c8` in player 2's hit test |
| 5 | 0 | 1000 (~20 s) | `$cb56`, `$cb78`: the catch reach becomes a flat `$32` = 50 instead of `$6d32` |

Codes 1 and 3 have **no** timer and instead put 5 and 3 into `$6d9e`, so they
are consumable counts rather than durations — which is why four separate sites
(`$a32a`, `$a340`, `$111a6`, `$111bc`) `clr.w $6d9a` after acting on it.

This closes three beads at once and reframes a fourth:

* **bd discr-z8m** — `$6d9a` is not a damage multiplier variable, it is the
  bonus code, and doubling the damage is what code 1 happens to do.
* **bd discr-dc0** — the "second writer that sets and clears bit 7 of a tile's
  HP word" is this. **Bit 7 marks a cell as carrying a bonus**, and `$a29c
  andi.w #$0f` is the clear, on pickup. The `(1,5) -> (1,133)` transition the
  bead was filed against is a bonus being *placed*; the writer that places it
  has not been found.
* **bd discr-b4q** — "a second writer clears the tile type without zeroing hp"
  is the same instruction family: `$a29c` rewrites the HP word without touching
  the type, and `andi.w #$0f` on an HP of 1 with bit 7 set gives 1 back, so the
  frame-114 `(1,1) -> (0,1)` needs the *type* writer, still unidentified.
* **The interview's three claims** — tougher AI, tiles take more hits, more
  discs — do not map onto `$6d9a`. Tiles taking more hits is code 1 and code 3;
  more discs is `$6d8a`/`$6d8c`; a tougher AI would be a different rule table at
  `$6da2`, whose writer has not been found. Recorded as three separate leads,
  not one resolved rank.

`bonus_6d9a` is 0 on all 240 frames of the reference run, so **every row of that
table is [code only]**. A trace in which a disc strikes a bit-7 cell is now the
highest-value fixture this project does not have.

## bd discr-m4x closed: what triggers a serve (Part 10)

`$c06e` has **zero** references — like `$a9a0` before it, it is not an entry
point but a block reached by falling through. It sits inside `FUN_0000abb2`,
**player 2's control routine**, which dispatches on `$6d2e` (p2's state index)
through a table:

```
$abb2  movea.l $6d4a,a2
$abb6  tst.b $6d2e ; bne $afb0
...
$afb0  move.b $6d2e,d0 ; ext.w ; lsl.w #2,d0
$afb8  lea ($1732,pc),a1
$afbc  movea.l (a1,d0.w),a1
$afc0  jmp (a1)                     ; -> $c068 for one of the states
```

and the block itself gates on the animation cursor:

```
$c068  move.b #$0f,$6d29
$c06e  cmpi.l #$4602,$6d5a          ; is p2's animation AT the release frame?
$c076  bne.w $ac40                  ;   no -> just advance the animation
$c07a  ... build the disc record from p2's position and input ...
$c0c0  bsr.w $a972                  ; SERVE
$c0c4  move.b #$11,$6d2e            ; p2 state := 17
```

So a serve is not triggered by a timer or by the disc: **player 2's control
routine enters a throw state, the throw animation plays, and the disc is
released on the single frame where `$6d5a` — the animation-sequence pointer —
equals `$4602`.** `$a972` has 12 call sites, nine of them elsewhere in `$abb2`,
so the same release happens from several throw states.

And in one-player mode the input that puts p2 into that state came from
`$6da1`, which is to say: **the AI presses fire, and five frames of animation
later a disc appears.** bd discr-fnl's "the dwell exit coincides with p2
entering state 17" and bd discr-m4x's "what triggers a serve" were the same
sentence read from two ends.

## The AI's mechanism: rules write continuations, not button presses (Part 10)

`$d2cc`'s loop (above) calls a rule's ACTION and then remembers the rule's
IDENTITY routine in `$6da6`. Reading one action and two identities shows what
those three pointers per table entry are actually for. **[code only]**

The action does not touch `$6da1` at all. It writes a small record into a
buffer, through the cursor `$6dfc` which `$d310` points at `$6dac`:

```
$e214  move.l #$e30a,(a1)+    ; a CONTINUATION routine
$e21a  move.w d1,(a1)+        ; parameter 1  (the test left them in d1/d2)
$e21c  move.w d2,(a1)+        ; parameter 2
$e21e  clr.l (a1)+
$e220  rts
```

and the identity routine, which `$d332` calls every frame while the behaviour is
latched, re-runs that continuation until its precondition fails:

```
$e222  cmpa.l $6e00,a5 ; bne $e232   ; still the same disc record?
$e228  tst.l (a1)      ; beq $e232   ; still a continuation to run?
$e22c  movea.l (a1),a2 ; jsr (a2)    ; run it -- this is what writes $6da1
$e230  rts
$e232  clr.l $6da6                   ; otherwise DROP THE LATCH
$e236  clr.w $6daa
$e23a  move.l #$6dac,$6dfc           ; and reset the plan cursor
```

So the three pointers are **test / plan / keep-going**: a rule that fires
compiles a two-parameter plan into `$6dac`, and its identity routine executes
that plan once per frame and is also the thing that decides the behaviour is
finished. That is why priority is a latch rather than a sort key -- the identity
routine holds `$6daa` up until it releases it.

`$6da6` in `rally_f100.seed` is `$e244`, the identity of table entry 5.

### The two unconditional reflexes

Entries 0 and 1 are the only ones with threshold 255, so they are the reflexes.
Both open with the same four-way guard -- **do nothing at all while player 2 is
in state `$15`, `$16`, `$1d` or `$1e`** (21, 22, 29, 30), which is presumably
mid-animation:

```
$e0d8  cmpi.b #$15,$6d2e ; beq out      (and $16, $1d, $1e)
```

**Entry 0, priority 50: "the floor under me is gone."**

```
$e0f8  move.w $6d30,d1        ; player 2's grid_cell
$e0fc  subi.w #$9,d1 ; bmi out          ; 9 is the floor bank's base
$e102  lsl.w #3,d1
$e104  lea $759e,a2           ; the FAR grid, one cell in
$e108  move.w ($02,a2,d1),d1  ; that cell's hp word
$e10c  andi.w #$7f,d1 ; bne out         ; only fires when hp (bonus bit masked
                                        ; off) is ZERO
$e114  ... $6d30 - 9 indexes byte tables at $1556 and $155e ...
```

It fires only when the cell player 2 is standing on has been reduced to zero,
and then reads a per-cell table at `$1556`/`$155e` -- an escape route. Highest
priority in the table, and unconditional: **not falling through the floor is the
opponent's first concern.**

**Entry 1, priority 30: "a disc is in my window -- play it."** It walks the 8
disc records and applies four range tests to the first live one:

```
$e178  lea $6e3e,a2 ; moveq #7,d2
$e17e  tst.b ($10,a2) ; bpl next         ; only a simulated slot
$e186  d0 = $6d24 + $6d40               ; 99 + p2+$20  -> 80
$e18e  d1 = disc+$02 (world_y)
$e192  if d1 <  d0 -> next              ; too low
$e198  d0 += $6d42                      ; + p2+$22 (17) -> 97
$e19c  if d1 >  d0 -> next              ; too high
$e1a2  d1 = disc+$04 (world_z)
$e1a6  if d1 <= $6d26 -> next           ; not yet at player 2's depth
$e1ae  d1 = $6d22 - 8 + $6d3c           ; an X window around player 2
```

So the reflex is a **box test in three dimensions**: `world_y` inside a band
whose height and depth come from player 2's own record (`+$20`, `+$22`),
`world_z` past player 2's own `world_y`, and `world_x` inside a window around
player 2's `world_x`. Every threshold is a per-player field rather than a
literal, which is exactly the shape a difficulty rank would use -- and remains a
hypothesis, because only one set of values has been observed.

The remaining 18 entries -- 11 distinct tests, 7 actions, the sensor pass
`$cea6`, and the eight priority-10 test/action pairs that read like a coin flip
between two responses to one situation -- are **not decoded**. bd discr-b6x
stays open for them.

## There are TWO 16-cell tile banks, not one 17-cell grid (Part 10)

`$9f5e` reading `$7596` where `$a24c` reads `$7616` put a second grid on the
board; entry 0 of the AI table pins down the layout. `$7596 + 16 * 8 = $7616`,
so the two banks are adjacent and identically shaped, and the seed reads them as
two plausible boards rather than one board and some noise.

The indices that address them come out at:

| who | index | where |
|---|---|---|
| a disc at the near wall (`$a24c`, `d5`) | 1..8 | `$7616` |
| a player's `grid_cell` (`$f836`) | 9..16 | `$7616` for player 1 |
| player 2's own cell (`$e104`) | `$6d30 - 9`, so 0..7 | `$759e`, i.e. `$7596` one cell in |

**This makes `disc-core`'s `TILE_CELLS = 17` wrong.** A bank is 16 cells; the
17th (`$7616 + 16*8` = `$7696`) is the first word past the end of the near bank
and has never been part of the grid. It happens to read `(1,1)`, so nothing has
ever noticed. Every tile event ever observed was at index 6, 7, 8 or 14, all
inside the bank, so no result depends on it -- but the differ and the fixture
both carry one word that is not a tile, and the far bank has never been carried
at all. bd discr-ovl.3 and bd discr-ovl.5.

## The player state machine, decoded (Part 10b)

`$f104` is player 1's control routine and its first two instructions are the
whole architecture. **[code+trace]**

```
$f104  movea.l $6cca,a2
$f108  tst.b $6cae ; bne $f5d0     ; state != 0 -> dispatch
       ; state == 0 falls through: THE IDLE PATH IS INLINE
$f110  d0 = $6cba ; clr.w $6cba ; $6ca2 += d0    ; apply the animation's X delta
```

**State 0 is not a table entry.** Entry 0 of `$10e2c` is a null longword; the
idle behaviour is the code after the `bne`. That is why nine phases of tracing
never found a "state 0 handler".

### Every handler ends in the same animation tail, and that tail is the clock

`$f1c4`, reached by a `bra` from the bottom of every state handler:

```
$f1c4  a1 = $6cda              ; the animation SEQUENCE CURSOR
$f1c8  a1 = (a1)               ; -> this cell's frame block
$f1ca  copy 20 bytes of it into $6ce4, $6cd6, $6cb6, $6cba, $6cbc, $6cc0,
                               $6cb4, $6cb5
$f1ea  a1 = $6cda
$f1ee  subq.w #1,$6ce2         ; frames left on this cell
$f1f2  bne $f1fc
$f1f4  addq.l #6,a1            ; expired -> the next cell, six bytes on
$f1f6  $6ce2 = ($04,a1)        ;   and its hold count
$f1fc  tst.l (a1) ; bne $f218  ; a zero longword TERMINATES the sequence
$f202  a1 = $2c78              ; ended: fall back to the idle sequence
$f206  $6ce2 = ($04,a1) ; $6cda = a1
$f210  move.b #$00,$6cae       ;   and the state becomes 0
$f218  $6cda = a1 ; rts
```

So an animation sequence is a list of **six-byte cells -- a four-byte frame
pointer and a two-byte hold count -- ending in a zero longword**, and *running
off the end of a sequence is what changes state*. `$f1c4`'s own ending goes to
state 0; state 20's copy of the tail (`$1099a`) writes `$6caa` instead.

`$6cda` (the cursor) and `$6ce2` (the count) were listed
`excluded:rendering` in `docs/state-schema.md` before this. **They are not
rendering. They are the state machine's clock**, and that entry is now
`waived:discr-75o`.

Two consequences worth stating separately:

* **`$6cba` is a per-frame X delta lifted out of the animation frame block** and
  applied by the idle path at `$f118`. Some movement is authored in the
  animation data rather than in code. The walk states do their own `subq.w #3`
  on top.
* **The whole 32-state machine is one mechanism**, not 32 unrelated handlers.
  What is still missing for the other 28 is only their sequence data and what
  each handler does on the way.

### `$6ca9` is the state whose handler last ran — bd discr-xfw answered

Every handler's *first* instruction stamps its own state number:

```
$f5e2  move.b #$01,$6ca9      (state 1, walk left)
$f7f6  move.b #$02,$6ca9      (state 2, walk right)
$1094a move.b #$14,$6ca9      (state 20, the turn)
$109aa move.b #$15,$6ca9      (state 21)
$f1c0  clr.b  $6ca9           (the idle path, when the joystick reads zero)
```

A handler may then change `$6cae` before the frame is over, so at the sampling
point `$6ca9` holds *the state that ran this frame* and `$6cae` holds *the state
that will run next*. That is the one-frame lag, exactly. It is not a facing flag
and not quite "the previous state" either -- 1 and 2 look like left and right
only because those happen to be the two walk states.

### The turn transient, frame for frame

`golden.ndjson` frames 10-14 read `0, 20, 20, 20, 1` and every step of that is
now accounted for:

```
$f260  btst #2,(a0) ; beq $f2b2        ; LEFT
$f266  tst.b $6ca9  ; bne $f296        ; already mid-something -> skip the turn
$f26c  $6cde = $2a8a                   ; the sequence to run AFTER the turn
$f274  $6caa = 1                       ; the pending state
$f27a  a1 = $2f7e ; $6ce2 = ($04,a1)   ; the turn sequence: ONE cell, hold 4,
$f284  $6cda = a1                      ;   then a zero terminator
$f288  $6cae = $14
$f292  bra $f1c4                       ; and run the tail in the SAME tick
```

`$2f7e` in the image is one cell with a hold of **4** followed by the
terminator. The entering tick runs the tail too, so the count is already 3 when
the frame is sampled; three more handler runs take it to 0, the cursor reaches
the terminator, and `$1099a` writes `$6caa` into `$6cae`. **Three sampled frames
of state 20, then the walk** -- which is what the fixture shows at f11-f13, and
again at f29-f31 when the stick is released (`$f7b8` clears `$6caa` first, so
the pending state is 0).

The `tst.b $6ca9; bne` at `$f266` is why the transient plays from a standing
start and not mid-move: the idle path clears `$6ca9` on every frame the joystick
reads zero.

Leaving a walk is a whole-byte test, not a bit test: `$f654 cmpi.b #$04,(a0);
bne $f7b8`. Anything other than exactly Left ends the walk.

### What this bought

`disc-core` models states 0, 1, 2 and 20 as of Part 10b, which is every state
player 1 reaches in either fixture before its hit test `$10fd8` fires. Both
fixtures now run **51 ticks** with nothing resynced but `players[1].*`, against
10 before. Supplying the discs instead, the player half alone reaches 63 and
stops where `$10fd8` puts player 1 into state 11 and moves its `world_y` --
which is bd discr-ovl.1's other half.

## Player 2's state table is at `$c6ec`, not `$10e2c` (Part 10b)

`$afb8 lea (...,pc),a1` in `$abb2` resolves to **`$c6ec`**, a second 32-entry
table with the same shape as player 1's at `$10e2c` -- entry 0 null, 31 handler
addresses in `$afc2`..`$c69a`. Entry 15 is `$c068`, which is the block that
contains the serve at `$c06e`. So **the serve is player 2's state 15**, and
`$c0c4 move.b #$11,$6d2e` moves it to state 17 on the frame the disc leaves.

## The serve, completed: two throw states and where every field comes from (Part 10c)

Part 10 decoded `$c068`, player 2's state 15. There is a second one, and the
fields it builds are now checked against both fixtures rather than read only.
**[code+trace]**

### `$c0fe` is state 16, the same code with two constants swapped

`$c6ec` entry 16 is `$c0fe`, and it is `$c068` line for line with exactly two
differences:

| | state 15 (`$c068`) | state 16 (`$c0fe`) |
|---|---|---|
| animation-cursor gate | `$c06e cmpi.l #$4602,$6d5a` | `$c104 cmpi.l #$45da,$6d5a` |
| `world_x` | `$c07e subi.w #$09,d0` | `$c114 addq.w #$03,d0` |
| the `$6d29` id it stamps | `$0f` | `$10` |

Everything else -- `world_y` 81, `world_z` = `$6d26 - 1`, the `$6d8e` dir_kind,
the `$6d9a == 2` override, the up-bit `vel_y`, the left/right `vel_x`, and
`move.b #$11,$6d2e` afterwards -- is identical. Six more `bsr $a972` sites exist
inside `$abb2` (`$b462`, `$b47a`, `$b492`, `$b512`, `$b52a`, `$b542`), so there
are further throw states with further parameter builds; these are the two the
fixtures exercise.

Golden frame 76 is the state-16 serve and it fits exactly: player 2 at `x` 49
puts the disc at 49 + 3 = **52**, which is what the trace reads, where the
state-15 offset of -9 would have given 40.

### The rest of the slot fill: `$a9b8`-`$a9cc`

The Part 9 transcription stopped at `$a9b4`. The four instructions after it are
where three long-standing unknowns were hiding:

```
$a9b8  st     ($10,a1)          ; active := $ff
$a9bc  clr.b  ($11,a1)          ; owner  := 0
$a9c8  move.l a2,($12,a1)       ; the steering hook, from $6d4a (player+$2a)
$a9cc  move.w $6d90,($16,a1)    ; damage := the thrower's +$70
```

**`disc+$16` comes from `player+$70`** -- `$6d90` reads 3 for player 2 and
`$6d10` reads 1 for player 1, the same magnitudes as their `+$6e` dir_kinds. So
a player's throw carries one number that is at once its depth speed and its
damage. `docs/state-schema.md` used to say where `$a9a0` got the damage from was
not recovered; it is `$6d90`.

### The animation cursor is the gate, and it is `player+$3a`

`$6d5a` is player 2's `+$3a` -- the same animation sequence cursor as player 1's
`$6cda`, the one the Part 10b state machine runs. So the serve is not gated on a
timer or on the disc: **it fires on the single frame of the throw animation
where the cursor reaches one exact value.** The oracle emits `+$3a` now, and the
fixtures show it plainly:

```
golden  f49 state 15 anim $45fc     f73 state 16 anim $45d4
        f50 state 15 anim $45fc     f74 state 16 anim $45d4
        f51 state 15 anim $4602 <-  f75 state 16 anim $45da <-
        f52 state 17 anim $4602     f76 state 17 anim $45da
```

### `$6da1` is written and consumed inside one VBL

A measurement that matters for any replay. `$6c58` is written by the IKBD
interrupt handler asynchronously, so the byte sampled at the VBL entry of frame
N is the byte frame N's work will consume. **`$6da1` is not**: `$10ec6 bsr
$d2cc` writes it and `$10ece bsr $abb2` consumes it two instructions later, both
inside the same VBL, so the byte a frame's work uses only becomes visible at the
*next* sampling point.

Measured rather than assumed: at `tile_damage.ndjson` frame 51 the sampled
`$6da1` is `$81` (fire + up), and the disc served on frame 52 has `vel_y` 0 --
which the up bit would have made -5. Frame 52's byte is `$80`, which serves 0.
`tracecheck` therefore drives player 2 from the frame it is predicting, and
player 1 from the frame it starts at, and says so.

## The hit test `$10fd8`, and the whole golden fixture (Part 10d)

Player 1's hit test is called from the disc loop at `$a652`, **between the
integration and the write-back**, which is the detail that makes it work: it
receives the three candidate coordinates in `d0`/`d1`/`d2` and can put the disc
back where it struck. **[code+trace]**

```
$10fd8  tst.b $6cac ; bne out            ; already out of energy
$10fe0  d5 = ($04,a5)                    ; the z it is LEAVING
$10fe4  tst.w ($0a,a5) ; bmi $10ffc
        dir_kind >= 0:  out unless d5 <  $6ca6 and d2 >= $6ca6
        dir_kind <  0:  out unless d5 >  $6ca6 and d2 <= $6ca6
                                         ; i.e. it CROSSED the player's depth
$1100c  tst.b ($11,a5)                   ; owner: states $12/$13/$1b branch away
$11030  state 7..10 -> the RACKET path at $11044
$110fc  d0 inside [px - 8 + $6cbc, that + 8 + $6cbe] ?
$11118  d1 inside [$6ca4 + $6cc0, that + $6cc2] ?
$11178  $6d16 -= ($16,a5), clamped at 0; at 0, st $6cac
$111ce  neg.w ($0a,a5) ; d2 += it ; clr.l ($12,a5)
$111da  and then a state, chosen by the one it interrupted
```

Two things about it are worth stating on their own.

**The box comes out of the animation.** `$6cbc`, `$6cbe`, `$6cc0` and `$6cc2`
are four of the words `$f1ca` copies out of the current animation cell's frame
block every frame, so **the hit box changes shape as the sprite does**. Player 1
reads `[-3, 11, -20, 18]` standing and `[-4, 11, -19, 16]` on the first frame of
being knocked down. `$6ca4` -- the constant 99 Part 9 identified -- is the
vertical origin the box is measured from.

**The struck state is chosen by the state it interrupted** (`$111da`):

| interrupted state | what happens |
|---|---|
| 1, 2, 3, 4, `$15`, `$16` | the state is kept; `$11256` forces an outgoing disc to `dir_kind` +1 |
| anything else, disc going away | `$11210`: animation `$2d60`, **state 12** |
| anything else, disc coming back | `$11226`: animation `$2d50`, **state 11**, and `$1123a` writes the disc's `dir_kind` to exactly **+1** |

That last write is why golden frame 64 shows `dir_kind` going -3 to +1 rather
than the +3 the `neg.w` at `$111ce` produced two instructions earlier.

### `player+$76` is energy, and `player+$0c` is "out"

`$11178`-`$111c6` reads `$6d16`, subtracts the striking disc's `+$16`, stores it
back and clamps at 0, and `$111ca st $6cac` marks the player out. Player 1's
energy across the golden fixture is **5, then 2 after the first strike, then 0
after the second** -- damage 3 each time, from the thrower's `player+$70`.

Two bonus branches sit in that path and neither is modelled, because no trace
carries a bonus code: `$1117c` skips the subtraction entirely when the code is 4
(**a shield**) and `$11188` applies it a second time when the code is 1.

### States 11, 12 and 23, and what actually paces them

State 11 (`$10554`) is four instructions of substance:

```
$10554  move.b #$0b,$6ca9
$10560  d0 = the current animation cell's frame block
$10562  cmp.l $6ce4,d0 ; beq $f1c4      ; unchanged since last frame -> nothing
$1056a  cmpi.w #$02,$6ca6 ; ble         ; floor
$10574  subq.w #1,$6ca6                 ; else sink one row
```

**A knocked-down player sinks one row per animation *cell*, not per frame.**
`$6ce4` holds the block `$f1ca` copied last frame, so the comparison is "has the
sequence advanced". `$2d50` is two cells of four frames each, which is exactly
the 18, 17, 17, 17, 17, 16, 16, 16 the fixture reads across frames 63-70, and
the sequence running out at frame 71 puts the player back in state 0. State 12
(`$1057c`) is the same code with `addq` and a ceiling of `$19`.

State 23 (`$10a72`) is entered from the idle path when `$6cac` is set --
`$f11c tst.b $6cac; bne $f170`, which loads `$2d70` and writes `$6cae = $17`.
Its handler is a **variant of the animation tail** and the difference is the
whole point: it tests for the terminator *before* copying, and on reaching it
does not change state. It bumps `$6cab` by 3 and `$6c83` by 1 and returns. So
**state 23 is terminal** -- the round is over for that player.

### What this bought

`disc-core` now reproduces **the whole of `tests/fixtures/golden.ndjson`**: 99
ticks, no divergence, with only `players[1].*` resynced. Player 1's walk, both
turn transients, both strikes, the energy 5 -> 2 -> 0, the death sequence, and
the disc's entire flight including two serves, a floor bounce and two returns.

### Still not decoded, in the same routine

**The racket path, `$11030`-`$110a8`.** States 7..10 catch the disc in a second,
wider box built from `$6cc6`/`$6cc8` and add `$6cc4` to its `vel_x`.
bd discr-ovl.1.

**CORRECTED in Part 11: `$113e2` is NOT in the racket path.** It is player 1's
own anticipation cascade -- the exact mirror of player 2's `$cb2c`-`$cc9a`, with
`$6cb2` for the reach, `$e` = 14 for the row threshold and `$7616` for the bank.
The racket path installs nothing. That mattered because it made discr-ovl.1's
"player-1 half" look like an unreachable block when it is in fact the same
cascade already decoded for player 2 with three constants swapped.

## A tile bank is eight tiles held twice, and destruction is delayed (Part 10e)

The frame-119 anomaly in `tests/fixtures/tile_damage.ndjson` -- a cell's type
cleared while its hp stayed 1, which `$a354` cannot do -- had been open since
Part 8 as bd discr-b4q. **[code+trace]**

It was found with a new tool rather than more reading: `--watch LO HI` on the
oracle reports every write into a range with the PC that made it. One run over
the whole of both banks, 215 frames, gives the complete census:

```
$ ./oracle/disc-oracle --seed seeds/diff.seed --frames 215 \
      --trace /dev/null --watch 0x7596 0x7696
watch frame  69  pc $00a34c  write.w $007648 = $0000     cell 6 hp
watch frame  69  pc $00a354  write.w $007646 = $0000     cell 6 type
watch frame 118  pc $014bb8  write.w $007686 = $0000     cell 14 type   <--
watch frame 169  pc $00a34c  write.w $007650 = $0001     cell 7 hp
watch frame 207  pc $00a34c  write.w $007658 = $0001     cell 8 hp
```

Five writes in 215 frames, and exactly one of them is not the damage path.

### `$a3a6` explains the whole layout

The destroy path does not stop at `$a354`. It spawns an effect:

```
$a354  clr.w (a0,d5)             ; hp reached 0 -> clear the type
       ... the destruction sample ...
$a382  bsr $9956
$a388  lea $779e,a2              ; THE effect record -- there is one
$a38c  tst.b (a2) ; bne $a3b2    ; already busy? then no animation at all
$a390  st (a2)                   ; claim it
$a392  lea ($58,pc),a0           ; = $a3ec, a sprite table indexed by d5
$a396  move.l (a0,d5),($08,a2)
$a39c  move.l ($04,a0,d5),($0c,a2)
$a3a2  clr.w ($02,a2)
$a3a6  lea $7656.w,a0            ; $7616 + 8 * 8
$a3aa  adda.w d5,a0              ;   + the struck cell's byte offset
$a3ac  move.l a0,($04,a2)        ; -> the cell the effect will clear
```

`$7656` is **eight cells past `$7616`**. Put that beside the two index formulas
and the layout closes:

```
a disc's cell   ($a250)  = column(world_x + 4) + (4 if world_y > $46)   1..8
a player's cell ($f836)  = 8 + column(world_x) + (4 if world_y > 14)    9..16
```

**Cells 1..8 and 9..16 are the same eight tiles.** 1..8 is the record the disc's
damage path writes; 9..16 is the copy the movement code `tst.w`s for
walkability. That is why a fresh round reads hp 4 or 5 in the eight low cells
and a dummy hp of 1 in all eight high ones -- the high copy only ever needs a
type. Both banks are laid out the same way, `$7596` for player 2 and `$7616` for
player 1.

### The collapse, tick by tick

`--watch 0x779e 0x77b0` over the same run gives the effect's whole life:

```
tick  69  $a390  st $779e             claimed, busy = $ff
          $a3ac  $77a2 = $7686        target: the struck cell + 8
tick  70  $14c7a $77aa advances       the frame cursor, one entry of the
 ..  117                              $5be4 list per tick -- 48 of them
tick 117  $14c76 addq.b #2,(a6)       the list ran out: busy = $01, POSITIVE
tick 118  $14bb2 subq.b #3,(a6)       -> $fe
          $14bb8 clr.w (a0)           THE WALKABILITY COPY IS CLEARED
          $14c76 addq.b #2,(a6)       -> $00, the slot is free again
```

The busy byte is a three-state machine in one byte: `$ff` = animating (negative,
so `$14bac bmi` sends it to the blitter), `$01` = animation done, `$00` = free.
The list at `$5be4` has **48 entries** before its zero terminator, which is
where the 48 comes from; the clear lands on the tick after, so a tile's
walkability survives its own destruction by **49 ticks**.

Two consequences worth stating plainly:

* **There is one collapse slot, not a queue.** `$a38c tst.b (a2); bne $a3b2`
  means a second tile destroyed while one is still collapsing gets no animation
  -- and, since the clear lives in the animation, **its walkability copy is
  never cleared at all**. Nothing in the code queues a second one. That is a
  game quirk, not a simplification of one.
* **The type word is cleared and the hp word is not**, which is exactly the
  `(1,1) -> (0,1)` the fixture reads and the reason the anomaly looked like a
  contradiction: the low copy's hp went to 0, the high copy's hp was never 4 to
  begin with.

## Each player has FOUR throw states, and two of them are running smashes (Part 10e)

`move.w #$51,d0` -- the first instruction of every serve parameter build --
appears exactly **eight** times in the image, four per player. Player 2's are
`$b426`, `$b4d6`, `$c084` and `$c118`, reached from `$c6ec` entries 3, 4, 15 and
16; player 1's are `$fa44`, `$faf4`, `$1078c` and `$10820`, from `$10e2c`'s same
four. **[code+trace]**

They are one routine written four times with three constants swapped:

| state | ST | animation gate | `world_x` | sideways step | wind-up |
|---|---|---|---|---|---|
| 3 | `$b3ee` | `$4754` | `p2.x - $b` | 2 | `subq.w #1,$6d22` per frame, then `-$a` at `$4742`, stamping `$6d29 = 3` |
| 4 | `$b4a0` | `$471a` | `p2.x + 4` | 2 | `addq.w #1,$6d22` per frame, then `+$a` at `$4708`, stamping `$6d29 = 4` |
| 15 | `$c068` | `$4602` | `p2.x - 9` | 1 | none, `$6d29 = $f` |
| 16 | `$c0fe` | `$45da` | `p2.x + 3` | 1 | none, `$6d29 = $10` |

So **3 and 4 are running smashes** -- the player slides one unit a frame toward
the wall during the wind-up, jumps ten at one animation frame, and the disc
leaves with twice the sideways step -- and **15 and 16 are the standing
throws**. Each pair is left and right.

`tile_damage.ndjson` frame 190 is a state-4 smash: player 2 at `x` 85 puts the
disc at 89 with `vel_x` **4**, where a standing throw would have given 2.

### And the column table is 160 bytes, not 152

Recorded because it cost a frame. `$7bfe` is four blocks of forty giving 1, 2,
3, 4 -- **160 bytes** -- and then zeros. An earlier note here said 152, which
was a short dump, and the consequence was precise: a disc at `world_x` 151,
which the `$9b` ceiling allows, reads index 155, and treating the table as 152
long made `disc_cell` give up on exactly the frame `tile_damage.ndjson` destroys
a cell (208). Outside the table the byte is 0, the same "not in the arena" value
the player's own lookup produces.

## `$c826`'s anticipation cascade: what installs the steering hooks (Part 10f)

Player 2's hit test is `$10fd8` mirrored -- same crossing test, same owner
check, same states 7..10 racket path, same body box, with `$6d2c`/`$6d26`/
`$6d2e`/`$6d22`/`$6d46` for player 1's `$6cac`/`$6ca6`/`$6cae`/`$6ca2`/`$6cc6`.
What player 1's does **not** have is the tail. **[code+trace]**

`--watch` over the disc-0 hook word counts the writers across 215 frames:

```
$ ./oracle/disc-oracle --seed seeds/diff.seed --frames 215 \
      --trace /dev/null --watch 0x6e50 0x6e54
     28 $00cb70      install $a7d8 -- start tracking
      1 $00cbae      state 27: reach
      1 $00cc1e      install $a816, state 18: step across
      2 $00a276      cleared by the tile-damage path
      1 $00a5aa      cleared by the world_x clamp
      2 $00a602      cleared by the world_z clamp
      2 $00a9c8      set by the serve, from $6d4a
```

**`$113e2`, player 1's racket install, never fires** -- neither player ever
swings at a disc in either fixture.

The cascade, `$cb2c`-`$cc9a`:

```
$cb2c  tst.b $6d2e ; bne out            ; only from state 0
$cb34  cmpi.b #$7,$6d29 ; beq out
$cb3e  tst.w ($0a,a5) ; bmi/beq out     ; only a disc travelling AWAY
$cb4a  tst.b ($11,a5) ; bne out         ; only one owner value
$cb52  d5 = $6d26 - $6d32               ; own depth minus own reach...
$cb56  ...or minus $32 under bonus code 5
$cb6a  if d2 < d5 -> out                ; not deep enough yet: nothing at all
$cb70  move.l #$a7d8,($12,a5)           ; START TRACKING
$cb78  d5 += reach ; d5 -= $c ; if d2 < d5 -> out
$cb96  d5 += 2      ; if d2 > d5 -> out ; a two-unit deep window
$cb9e  d5 = $6d22 - 3
       a ladder on the disc's X either side of d5, mirrored:
         within $c of it        -> $cbae  REACH
         $f past it             -> $cbae  REACH
         further, but not $22   -> probe the cell $c over
         further still          -> out
$cbae  keep $a7d8, animation $466a, state $1b = 27
$cc1e  install $a816, animation $4612, state $12 = 18
```

Note that **`$cb70` fires whether or not either state is entered** -- 28 times
against one each of `$cbae` and `$cc1e`. Tracking a disc and committing to a
response are separate decisions.

### The choice between reaching and stepping across

`$cc02`-`$cc1c` is a small piece of real judgement:

```
$cbdc  d6 = $6d22 - $c            ; twelve units over
$cbe4  if d6 < 8 -> $cc16         ; off the arena -> reach
$cbea  d6 = colTable[d6] + 8 ; if $6d26 > $3a: d6 += 4
$cc02  cmp.w $6d30,d6 ; beq $cc1a ; already standing there -> step
$cc0a  lea $7596,a0 ; tst.w (a0,d6) ; bne $cc1a  ; walkable -> step
$cc16  d6 = 0  -> reach
$cc1a  d6 = -1 -> step across
```

**Step across only if the cell twelve units over is somewhere you could stand**,
otherwise just reach. The row threshold is `$3a` = 58, not the movement code's
14, because a player's own `world_y` is 54. Both fixtures exercise the decision
once each and in opposite directions: `tile_damage.ndjson` frame 21 steps across
(state 18) and frame 111 reaches (state 27).

### State 18's handler, `$c196`

```
$c196  move.b #$12,$6d29
$c19c  if $6d5a is $4624 or $4634 -> $c1b4, else just advance the animation
$c1b4  btst #7,(a0) ; beq out          ; fire must be HELD
$c1bc  btst #1,(a0) ; bne out          ; down must not be
$c1c4  if $6d8a == $6d8c -> out        ; the two disc counters must differ
$c1d0  subq.w #6,$6d22                 ; step six units left
$c1d4  animation $45f0
$c1e2  move.b #$f,$6d2e                ; and into state 15, the standing throw
```

So the intercept is: play the reach animation, and on the one frame it reaches
`$4624`, if fire is still held, commit -- **six units left in a single step**,
straight into a throw. `golden.ndjson` frame 39 has the cursor at `$4624` and
frame 40 has player 2 at `x` 57 from 63, in state 15.

`disc-core` stops there, because `$6d8a` and `$6d8c` are the possession counters
the disc loop moves at four sites and this crate does not model.
`// UNKNOWN: see bd discr-b6x`.

### Every handler stamps `$6ca9`, so that much is universal

A small thing with a large effect. Every entry in either table opens by writing
its own index to `player+$09` -- `$f5e2` writes 1, `$f7f6` 2, `$10554` `$0b`,
`$1057c` `$0c`, `$1094a` `$14`, `$109aa` `$15`, `$10a72` `$17`, `$10ac4` `$18`,
`$c196` stamps `$6d29` and its own state via the same shape. So `disc-core`
stamps it once for all 32 entries, **including the 25 whose behaviour is not
modelled**, and `players[n].facing` is then correct for every state either
player reaches. Only state 0 differs: its inline path *clears* the byte, and
only when the joystick reads zero (`$f1c0`).

## bd discr-0fm CLOSED: the dwell was a caught disc (Part 10g)

`disc+$10`'s four writers, from `--watch 0x6e4e 0x6e4f` over 215 frames:

```
frame  33  pc $00caae  write.b $006e4e = $03     player 2 catches it (state 18)
frame  33  pc $012588  write.b $006e4e = $02       and the countdown's first step
frame  34  pc $012588  write.b $006e4e = $01
frame  35  pc $012588  write.b $006e4e = $00       free
frame  51  pc $00a9b8  write.b $006e4e = $ff     the serve claims the slot
frame 123  pc $00cb1e  write.b $006e4e = $03     caught again (state 27)
...
```

and on the golden programme one more:

```
frame  97  pc $00a570  write.b $006e4e = $03     the ROUND ENDED
```

So the byte's whole life is:

| PC | what |
|---|---|
| `$a9b8` | `st` -- the serve claims a free slot |
| `$caae` | `addq.b #4` -- player 2 catches it from state 18 |
| `$cb1e` | `addq.b #4` -- ...or from state 27 |
| `$a570` | `addq.b #4` -- the round is over, clear the board |
| `$012588` | `subq.b #1` -- the **render pass** counts a retired slot down |

`$ff + 4` is `$03`, and `$012582`'s countdown runs in the same tick as the
catch, so a caught disc reads 2, 1, 0 over the next three frames and its record
never moves again. **The "dwell at `world_z` 54" was a disc that had been
caught.** Not a `world_z` phase, not an anomaly, and nothing to do with the
`$4f` bound. `disc-core` models all four writers and `discs[n].active` is a
compared row.

The countdown living in `$012582` -- the render routine, which draws a live disc
and counts down a retired one -- is why nine phases of reading `$a4ea` never
found it.

### The catch, and what missing it costs

`$c826`'s head is `$10fd8`'s, and after the owner check three of player 2's own
states get a catch window before the body box is reached:

```
$c860  state $12 -> $ca96      ; the intercept's catch: x within $6d22 +/- $1a
$c86a  state $13 -> $cad0      ; not modelled -- no fixture reaches state 19
$c874  state $1b -> $cb06      ; the reach's catch: x in [$6d22 - $10, + $20]
```

A catch is two instructions -- `addq.b #4,($10,a5)` and `subq.w #1,$6d8a` -- and
**missing it falls through to the strike** (`$cab8`/`$cb28` branch on to
`$c934`, the mirror of `$110fc`). Reach for a disc, miss, and it hits you. State
18's miss also sets state 17 first (`$cac6`).

### `player+$0d` ends a round

`$a564 tst.b $6d2d; bne $a570` -- the disc loop retires every disc in play when
the *other* player's `+$0d` is set, and `$f1b4 st $6d2d` sets it three
instructions after player 1 enters the death state. So a round ends by clearing
the board, and `golden.ndjson` frame 97 is exactly that: player 1 hit state 23
at frame 97 and disc 0 was retired on the same tick, by `$a570` rather than by a
catch.

## Player 2's state 18 handler, and the one stub in 64 states (Part 10g)

`$c196`, the commit half of the intercept:

```
$c196  move.b #$12,$6d29
$c19c  if $6d5a is $4624 or $4634 -> commit; otherwise just run the animation
$c1b4  btst #$7,(a0) ; beq out           ; fire must still be HELD, not an edge
$c1bc  btst #$1,(a0) ; bne out           ; and down must not be
$c1c4  if $6d8a == $6d8c -> out          ; already at the disc cap
$c1d0  subq.w #6,$6d22                   ; six units left, in a single step
$c1d4  animation $45f0
$c1e2  move.b #$f,$6d2e                  ; and straight into state 15
```

`$6d8c` is the cap on `$6d8a`, never written anywhere in the image: **4 for
player 2 and 0 for player 1**, whose count is also 0 -- so player 1 can never
throw from this state, which is consistent with it never throwing in either
fixture.

`btst #$7,(a0)` wants the fire bit as a **level**, not the edge the walk
handlers see (`$f606`/`$f81a` consume it with `bclr`), which is why
[`crate::Input`] carries both.

### State 17 is the only handler in either table with no body

Comparing each of the 64 table entries with the next handler in address order
finds exactly one four-byte stub per player, and it is **state 17** in both:
`$1089a bra $f1c4` and `$c192 bra $ac40`. It stamps nothing, so it is the one
exception to "every handler writes its own index into `player+$09`" -- and the
fixtures show it plainly: player 2's `+$09` holds 15 for all seven frames it
spends in state 17 after a throw.

Leaving state 17 is the shared animation tail running out, which needs the
sequence the entering state loaded -- `$45f0` after an intercept, `$462e` after
a missed catch. `disc-core` carries hold counts for the four sequences it has
transcribed and no more, so that is where the fully-compared run now stops.
`// UNKNOWN: see bd discr-75o`.

## The animation tables, and how a handler names a sequence (Part 10h)

The state machine's clock needed the tables themselves, and recovering them
turned up one thing worth knowing: **a handler does not always load the start of
a table.** `$c1d4 lea $45f0,a1` picks a cell partway into the block that begins
at `$45ea`, and the sequence then runs forward from there to the same zero
terminator. So a sequence is identified by *the cursor a handler loads*, not by
a table base.

Recovering them from the image is mechanical once you know that a real cell is
`(a plausible frame pointer, a small hold)` -- the packed tables sit adjacent, so
walking backwards from a known cell stops on the previous table's terminator,
and the hold field is what tells them apart: a real hold is 4, 6, 48 or 80,
whereas a frame pointer read as a hold is five figures.

The eleven sequences either fixture touches, all cross-checked against the `lea`
that loads them:

| ST | cells | holds | loaded by |
|---|---|---|---|
| `$2c78` | 16 | 6,6,6,6,**48**,6,6,6,**48**,6,6,6,**48**,6,6,6 | `$f202` -- player 1 standing |
| `$2a8a` | 6 | 4 x 6 | `$f296` -- player 1 walking left |
| `$2b0e` | 6 | 4 x 6 | `$f22a` -- player 1's state 5 |
| `$2d50` | 2 | 4, 4 | `$11226` -- knocked down |
| `$2d60` | 2 | 4, 4 | `$11210` -- knocked upward |
| `$2d70` | 16 | 4 x 16 | `$f1a0` -- out of energy |
| `$2f7e` | 1 | 4 | `$f27a` and three others -- the turn transient |
| `$468c` | 16 | the same shape as `$2c78` | player 2 standing |
| `$4612` | 4 | 6 x 4 | `$cc26` -- player 2 stepping across |
| `$466a` | 5 | 6, 6, 4, 4, 4 | `$cbb6` -- player 2 reaching |
| `$45f0` | 5 | 4 x 5 | `$c1d4` -- when the intercept commits |
| `$462e` | 2 | 6, 6 | `$cab8` -- a missed catch |

The three 48-frame holds in the idle sequence are the standing animation's
pauses, and they are why an idle player's `+$09` sits still for so long.

### `$45f0` is why state 17 ends when it does

Twenty frames of animation -- five cells of four -- shared between state 15 and
the state 17 that follows the serve. `golden.ndjson` spends twelve frames in
state 15 and seven in state 17, and the sequence runs out on the twentieth tick,
which is where `$ac8c` writes state 0. **A serve does not load a new sequence**:
`$c068`'s release path is `bsr $a972; move.b #$11,$6d2e; bra $ac40`, so state 17
inherits whatever state 15 was running, and the throw animation finishing is what
ends the follow-through.

That is the general shape of the whole machine, now that all the pieces are in
one place: a state loads a sequence, its handler runs each frame, the sequence's
holds pace whatever the handler does, and the sequence running out is the
transition. Nothing in it is a timer.

## Player 2's remaining handlers: the shape, and where the grind starts (Part 10i)

`--watch` over `$6d22` gives every writer of player 2's `world_x` across the
golden programme, which is the whole of what is left to transcribe on that field:

```
frame  0..5   pc $00b038   -3 per frame      state 1, walk left        modelled
frame 10..20  pc $00abc6   +0               the IDLE PATH's X delta    NOT modelled
frame 39      pc $00c1d0   -6               state 18's commit          modelled
frame 59      pc $00abc6   -4               the idle path again
frame 59      pc $00ae84   -4               state 16's ENTRY           NOT modelled
frame 83      pc $00abc6   -4               the idle path
frame 84      pc $00b24e   +3               state 2, walk right        modelled
frame 89      pc $00ae84   -4               state 16's entry again
```

Two things are missing, and both are now located to the instruction.

### `player+$1a` is a per-frame X delta the idle path consumes

```
$abbe  move.w $6d3a,d0        ; p1: $f110 move.w $6cba,d0
$abc2  clr.w  $6d3a           ;     $f114 clr.w  $6cba
$abc6  add.w  d0,$6d22        ;     $f118 add.w  d0,$6ca2
```

`player+$1a` is one of the words `$f1ca` copies out of the current animation
cell, so **some movement is authored in the animation data**, applied by the
idle path and cleared as it is used. Part 10b noted this and it is still not
modelled; it is the only reason player 2's `world_x` moves while it is standing
still. It needs one more fed column (`player+$1a`), in the same category as the
hit box.

### Every throw entry probes a cell and picks between two throws

The pattern repeats verbatim in at least three places -- `$cc02` (the
anticipation cascade), `$adf0` and `$ae54` -- and it is worth stating once
because everything left to transcribe is a variation on it:

```
d0 = own world_x +/- <offset>          ; $adce is +$d, $ae94 is -$26
if d0 outside 8..$98      -> not standable
d0 = colTable[d0] + 8 ; if own world_y > $3a: d0 += 4
if d0 == own grid_cell    -> standable
if own bank[d0] type != 0 -> standable
```

**CORRECTED in Part 10j.** The two probes are two arms of one rule, not two
rules with opposite polarity: `$adb2`-`$adc6` picks which side to probe from the
joystick (or from `player+$08`, the side of the last throw), and both arms then
ask *should I go left?* -- probing right and finding nowhere to go means yes
(`$ae0a beq $ae70`), and probing left and finding somewhere to go also means yes
(`$ae6e beq $ae0e` falls through to `$ae70`). State 16 steps left and throws;
`$ae0e` is state 15, which steps right and throws.

State 16's entry itself is four instructions:

```
$ae70  lea $45c2,a1 ; $6d62 = ($4,a1) ; $6d5a = a1
$ae7e  move.b #$10,$6d2e         ; state 16
$ae84  subq.w #4,$6d22           ; and step four units left
$ae88  st $6d28
```

So the remaining work on player 2 is **transcription, not discovery**: for each
of the states the fixtures reach, `--watch` the field it moves, `--disasm` the
writer, transcribe the probe and the offset, measure. The fully-compared run
moves a handful of ticks per handler. What is *not* yet located is what states 3
and 4 do during their wind-up, and player 2's strike and racket halves -- which
mirror `$10fd8`'s and are already modelled for player 1.

## Player 2's throw decision, and golden reproduced with nothing waived (Part 10j)

Three small reads finished the golden fixture off completely.

### `$ad82`-`$ae2a`: how player 2 decides to throw

```
$ad82  cmpi.b #$80,(a0) ; beq out      ; fire ALONE does nothing
$ad8a  btst #$1,(a0) ; bne $af50       ; fire+down goes elsewhere
$ad92  if $6d8a >= $6d8c -> out        ; already at the disc cap
$ad9e  if $6d29 == 1 -> $ae90          ; walking left  -> the smash chooser
$ada8  if $6d29 == 2 -> $aef0          ; walking right -> the other one
$adb2  btst #$2,(a0) ; bne $adca       ; LEFT held  -> probe RIGHT
$adba  btst #$3,(a0) ; bne $ae2e       ; RIGHT held -> probe LEFT
$adc2  tst.b $6d28 ; bne $ae2e         ; neither: the last throw's side picks
$ae70  state 16: sequence $45c2, x -= 4, st  $6d28     ; step LEFT and throw
$ae0e  state 15: sequence $45ea, x += 4, clr $6d28     ; step RIGHT and throw
```

So **states 15 and 16 are the same throw stepping opposite ways**, which is also
why their serves offset differently -- `p2.x - 9` for 15 and `p2.x + 3` for 16,
measured after the sidestep. `player+$08` records which way the last one went,
and it is what breaks the tie when the stick is not pushed either way.

The probe is [`can_stand`]'s, the same one the anticipation cascade uses, and its
row threshold matters: **`$3a` = 58 while a player's own `world_y` is 54**, so
the probe lands in the near row where `grid_cell` puts the player in the far one.
The cells therefore differ, the bank lookup decides, and reading that threshold
as the movement code's 14 makes the whole decision come out backwards.

`$6d29` -- which is `player+$09`, the same byte as the state stamp -- routes a
*walking* player somewhere else entirely: `$b1e0`-`$b1f8` in the walk handlers
send a fire press to `$ad82`, which sees the walk's own stamp (1 or 2) and goes
to `$ae90` or `$aef0`, the **running smash** choosers for states 3 and 4.
`tile_damage.ndjson` frame 162 is one: player 2 walking right with fire enters
state 4 and starts sliding a unit a frame. Not modelled.

### `tst.b (a0)` is a whole-byte test, and `$80` is not empty

`$f1ba tst.b (a0); beq $f1c0` is what clears `player+$09` in the idle path, and
it tests the **whole** input byte. A byte of `$80` -- fire held with no direction
-- is non-zero, so it does *not* reach the clear, and the stamp from whatever the
player was doing stays put. `tile_damage.ndjson` frame 60 is exactly that: the
AI holds `$80`, the byte keeps the 15 the throw left there, and a model that
treats "no direction bits" as "no input" drops it to 0 on a frame the ST leaves
alone.

### `player+$1a` is an X delta the idle path consumes

```
$abbe  move.w $6d3a,d0      ; p1: $f110 move.w $6cba,d0
$abc2  clr.w  $6d3a         ;     $f114 clr.w  $6cba
$abc6  add.w  d0,$6d22      ;     $f118 add.w  d0,$6ca2
```

One of the words `$f1ca` copies out of the animation cell, read and cleared once
per frame. It is the only reason a standing player's `world_x` moves, and nothing
recomputes `grid_cell` after it -- so a probe on the same frame compares against
the cell from the frame before.

### What that adds up to

`tests/fixtures/golden.ndjson` reproduces **99 of 99 ticks with nothing waived
and nothing resynced** -- every compared row of *both* players, including all
five of player 2's. The idle fixture reaches 161 of 214 on the same terms and
stops at the running smash.

## The running smash, and which handlers stamp `player+$09` (Part 10k)

### 28 of 31 handlers stamp, and the three that do not are 3, 4 and 17

Reading the **first instruction of all 64 handlers** -- 32 per player -- settles a
question this file got wrong twice, and both tables give the same answer:

| | |
|---|---|
| 28 states | open with `move.b #<their own index>,player+$09` |
| state 3 | `$fa0c` / `$b3ee` open with `cmpi.b #$3,player+$09` -- they **read** it |
| state 4 | `$fabe` / `$b4a0`, the same, mirrored |
| state 17 | `$1089a` / `$c192` open with `bra` -- the stub has no body |

State 0 is not in either table; its inline path *clears* the byte, and only when
the whole input byte is zero.

So `player+$09` does double duty: a stamp for 28 states, and for the two smashes
**a latch** -- "have I already lunged?". Part 10f called the stamp universal and
Part 10g called it universal-except-17; both were right about most states and
wrong about ones the fixtures spend time in. Deriving it from 64 first
instructions costs one script and cannot be wrong the same way.

### The smash: run, lunge, latch, release

```
$b4a0  cmpi.b #$4,$6d29 ; beq $b4c2      ; latched -> skip the slide
$b4a8  addq.w #1,$6d22                   ; else slide one unit a frame
$b4ac  cmpi.l #$4708,$6d5a ; bne $b4c2
$b4b6  addi.w #$a,$6d22                  ; at that frame, lunge ten more
$b4bc  move.b #$4,$6d29                  ; and latch, which stops the slide
$b4c2  cmpi.l #$471a,$6d5a               ; the release frame
```

State 3 mirrors it exactly: `subq.w #1`, `-$a`, lunge at `$4742`, release at
`$4754`. `tile_damage.ndjson` frames 162-165 are the slide -- `world_x` 59, 60,
61, 62 -- and frame 190 is the release, which was already modelled and is why
that serve carries `vel_x` 4.

### Getting into one: `$ae90` and `$aef0`

A fire press inside a walk (`$b1e0`-`$b1f8`) goes to the throw decision at
`$ad82`, which reads the walk's own stamp in `player+$09` -- 1 or 2 -- and routes
to the chooser for that direction. Each chooser is one probe:

```
$aef0  d0 = $6d22 + $26 ; ... can_stand? ; yes -> state 4, sequence $46f0
$ae90  d0 = $6d22 - $26 ; ... can_stand? ; yes -> state 3, sequence $472a
```

`$26` is **38 units** -- far enough that the question is "is there room for the
whole run", not "is the next step safe". No room falls back to the standing
throw's own 13-unit probe.

Three probes in the game now, all the same predicate and all differing only in
reach: **13** units for a standing throw (`$adce`/`$ae32`), **38** for a running
smash (`$aef4`/`$ae94`), and **12** for the intercept's step-across (`$cbe0`).

### Both fixtures now reproduce completely

`tests/fixtures/golden.ndjson` 99 of 99 and `tests/fixtures/tile_damage.ndjson`
214 of 214, **with nothing waived and nothing resynced** -- every compared row of
both players, every tick. `mise run core-check`'s four runs are four clean runs.

## There are FOUR steering hooks, not three (Part 11)

A fixture minted to look for a swing found something else: `$a78e`, a fourth
routine in `disc+$12`, which no earlier trace had ever installed.

```
$a78e  move.w $6ca2,d5 ; subq.w #4,d5    ; player 1's SHALLOW aim
$a794  cmp.w d0,d5 ; bgt/blt ...          ; the same three-case rule
```

With it the set is symmetric, two per player, and the pairing is exact:

| hook | aim | axes | installed by |
|---|---|---|---|
| `$a71a` | `$6ca2 - $13` | X, then `$a758`'s Y | `$113e2` |
| `$a78e` | `$6ca2 - $04` | **X only** | `$11334`, `$11372` |
| `$a7d8` | `$6d22 - $04` | **X only** | `$cb70`, `$cbae` |
| `$a816` | `$6d22 - $13` | X, then `$a854`'s Y | `$cc1e` |

So each player's cascade installs its shallow hook when it starts tracking a
disc (`$11334` / `$cb70`) and again if it only reaches (`$11372` / `$cbae`), and
its deep hook when it commits to stepping across (`$113e2` / `$cc1e`). One
routine, mirrored.

`disc_core::SteerHook` had three variants and `tracecheck`'s pointer mapping
**panics** on an unrecognised one rather than silently steering at nothing. That
choice is what turned a missing variant into a loud failure the first time a
trace installed `$a78e`, instead of a quiet mis-steer nobody would have noticed.

### And player 1's cascade is the same code with three constants swapped

```
                      player 1            player 2
reach                 $6cb2 (12)          $6d32 (26)
row threshold         $e  = 14            $3a = 58
own bank              $7616               $7596
shallow / deep hook   $a78e / $a71a       $a7d8 / $a816
```

The row threshold differs because each player probes at its own depth: player 1's
`world_y` is 18 and player 2's is 54. Everything else about the cascade --
`$11340`-`$113ee` against `$cb2c`-`$cc3e` -- is instruction for instruction the
same.

## Player 2's strike, and two asymmetries in otherwise-mirrored records (Part 11b)

`$c934`-`$ca10` is `$110fc`'s mirror: the same four comparisons against the same
animation-derived hit box, the same energy dock, the same bounce. Two things
about it are **not** mirrored, and both are traps.

### The owner gate is inverted

```
$1116e  tst.b ($11,a5) ; bne $111ce     ; player 1: non-zero SKIPS the dock
$c9a6   tst.b ($11,a5) ; beq $ca06      ; player 2: zero SKIPS it
```

Read together they say one thing: **the disc's owner byte says whose energy is at
risk.** 0 docks player 1, anything else docks player 2. Neither routine is
"the strike"; each is one half of it.

### The two energies are at different offsets

```
$11178  move.w $6d16,d5      ; player 1's energy -- player+$76
$c9b0   move.w $6d94,d5      ; player 2's energy -- player+$74
```

The records are mirrored in every other field this project has found, so this is
exactly the kind of thing a mirror assumption gets wrong -- and did: the oracle
emitted `+$76` for both players for four parts, which reported player 2's energy
as a **constant 0** while its real value sat at 15 two bytes away. Nothing caught
it because player 2 is never struck in any fixture, so a constant was
indistinguishable from a constant.

The bonus words are crossed in the same way and at different offsets again:
player 1's is `$6d1c` (`+$7c`) and player 2's is `$6d9a` (`+$7a`), and **each
player's strike reads the OTHER's** -- `$11188` reads `$6d9a` and `$c9c0` reads
`$6d1c`. That is the right way round for a bonus that belongs to the thrower and
modifies the damage they deal.

### A missed strike is where tracking begins

`$c940` and the three comparisons after it all branch to `$cb2c`, the
anticipation cascade. So a disc that crosses player 2's depth and neither is
caught nor connects is a disc player 2 **starts tracking** -- the miss and the
anticipation are one code path, not two.

## The tile collapse runs in the render pass, after the players (Part 11c)

Part 10e put `collapse_step` **first** in the tick because that made the 49-frame
delay come out right. It is wrong, and `p1_walk` frame 143 is what catches it:
player 2 walks off cell 15 on the very frame the collapse clears that cell's
type, and the ST lets it. Clearing before the player update blocks the step.

Both constraints hold once the **busy byte is modelled instead of a frame
count**. `$779e` is a three-state byte and `$14bac tst.b (a6); bmi` sends a
*negative* one to the blitter, so the claiming tick's own pass advances the
sprite cursor and counts nothing else down:

```
claim tick   $a390 st (a2)                busy = $ff, cursor = 48 cells
             ... the pass runs, advances the cursor, does not free anything
+48 ticks    the $5be4 list runs out
             $14c76 addq.b #2,(a6)        busy = $01, positive
next tick    $14bb2 subq.b #3,(a6)        busy = $fe
             $14bb8 clr.w (a0)            THE TYPE IS CLEARED
             $14c76 addq.b #2,(a6)        busy = $00, the slot frees
```

With the effect pass **last** in the tick and the byte modelled, the delay is
still 49 and the walk is no longer blocked. Modelling the byte rather than a
counter is what removed the fudge: an "extra one for the claiming tick" would
have produced the same number and hidden the reason.

## Player 2's walk probe is not the obvious expression (Part 11c)

Player 1's walk probes 24 units ahead and reads `$7616` --
`$f60e sub.w #$0018,d0` then `$f63e tst.w ($00,a1,d0.w)` with `a1 = $7616`. It is
verified over three fixtures.

Player 2's is **not** the mirror of that, and this is measured rather than
argued. Its walk handler reads `$7596` at several sites inside `$abb2`, so
`own_bank[grid_cell(x - 24, y)]` looks obviously right. Trying it:

```
                        near bank ($7616)   own bank ($7596)
p1_walk                 143 ticks           99 ticks
tile_damage, no flags   clean (214)         201 ticks
```

Both regressions are player 2 taking a step the ST does not, so with its own
bank the probe reads *walkable* where the ST reads *blocked*. Either the probe
distance differs from 24 or the index is not `grid_cell`'s. The near bank is
closer and its one wrong frame is `p1_walk` 144, where a cell of player 1's floor
collapses under the same index -- which is the sort of coincidence that makes a
wrong rule look right for 143 frames. `// UNKNOWN: see bd discr-b6x`.

Recorded because "it reads its own bank, obviously" was tried and is false.

## The walk probe is not a gate (Part 11d)

Both walk handlers probe 24 units ahead and look the cell up, and `disc-core`
treated that as "may I step there". It is not. `$f60a`-`$f64a` in full:

```
$f60a  d0 = $6ca2 - $18            ; 24 ahead
$f612  cmp.w #$8,d0 ; blt $f644    ; off the arena -> d0 = 0
$f618  d0 = colTable[d0] + 8
$f624  cmpi.w #$e,$6ca6 ; ble      ; +4 for the far row
$f630  cmp.w $6cb0,d0 ; beq $f648  ; the cell you are ON is always fine
$f638  lea $7616,a1 ; tst.w (a1,d0)
$f64a  bne $f650
$f64e  st d2                       ; <- the answer goes into a FLAG
$f650  cmpi.b #$04,(a0) ; bne      ; and THIS is what gates the move
$f658  subq.w #3,$6ca2             ; unconditional once the direction matches
$f65c  ... a SECOND lookup, on the new x
```

So the probe sets `d2` and the step happens anyway. What reads `d2` is further
down the handler and is not decoded -- plausibly the fall-through-a-hole path,
given the second lookup at `$f65c` runs on the *new* position.

**Both committed fixtures agreed with the wrong model for eleven parts**, because
in neither does a walking player ever probe a destroyed cell. `p1_walk` frame 100
is the frame that tells them apart, and it says the player moves. Removing the
gate took that fixture from 143 ticks to 191.

### The probe's own three constants, since it is decoded even if unused

| | player 1 (`$f60a`) | player 2 (`$afea`) |
|---|---|---|
| distance | `-$18` = 24 | the same |
| off-arena | `cmp.w #$8; blt` -> blocked | the same |
| far row | `+4` when `$6ca6` **>** `$e` (14) | `+4` when `$6d26` **<=** `$3a` (58) |
| own-cell shortcut | `cmp.w $6cb0; beq` | `cmp.w $6d30` |
| bank | `$7616` | `$7596` |

The far-row test's **polarity is inverted** between the two, and both add 4 at
the depths the fixtures use -- 18 and 54 -- so the difference is invisible in the
data and would only bite a player at an unusual depth. `crate::player::walk_probe`
is that function, exposed and unit-tested; nothing calls it.

Two things that went wrong on the way here, both worth keeping:

* switching the bank alone made two fixtures **worse**, because without the
  own-cell shortcut a player standing on a collapsed tile could not leave it.
  Half a transcription is not a partial improvement;
* three rounds of arithmetic on which bank and which threshold produced three
  wrong answers. Reading `$afc2` end to end took one command.

## The disc loop runs twice per frame in one fixture, and only past frame 191 (Part 11e)

Counting `$a65e`'s writes to `disc+$00` per frame, across all three fixtures:

```
golden       100 frames   exactly one write per frame, always
tile_damage  215 frames   exactly one write per frame, always
p1_walk      275 frames   one per frame for 191 frames, then TWO on alternate
                          frames from 192 -- 37 such frames
```

`$6ab4` still advances by exactly 1 on every frame of all three, so this is **two
iterations inside one VBL**, not a dropped or doubled frame. And the boundary is
exact: `p1_walk` run for only its first 191 frames shows the pattern not at all.

That reframes the `p1_walk` gate. Its wall at frame 192 -- `discs[0].world_x` 27
against 29 -- is not a missing disc rule. It is the first frame on which the ST
steps the disc twice and `disc-core`, which steps once per tick by construction,
steps once. **191 is the last frame that behaves like the two validated
fixtures**, which is a better reason to stop there than a missing rule.

Two readings, with opposite consequences:

* **the game double-steps** -- a catch-up or rate mechanism -- in which case
  `disc-core` must model it and the trace is faithful;
* **the oracle has drifted** from the real machine, in which case `p1_walk` past
  191 is not evidence about the game at all.

The experiment that separates them is a Hatari reference for this input
programme run through `scripts/oracle_diff.py`. None exists: the 275-frame tier-1
figure this fixture's provenance claimed was measured for the **idle** programme
and does not transfer to a different input. That claim has been corrected.

`$a4ea` has **zero** absolute references and `$a4e8` is an `rts`, so the disc loop
is its own routine entered through a pointer. Finding the indirect caller would
also answer this. bd discr-ovl.7.

### The watch cap now says when it truncates

`--watch` stopped reporting silently at 4000 lines, which cost a confusing empty
result mid-investigation. It says so now. A measurement tool that stops measuring
without telling you is worse than one that refuses.

## The game update is in the MAIN LOOP, not the VBL (Part 11f)

This is the correction that resolves bd discr-ovl.7, and it is about the shape of
a frame rather than any single rule.

`--callers 0xa4ea` -- a new oracle flag that reports the return address every
time execution reaches an address -- named the call site in one command, after a
static search for pointers to `$a4ea` found none. (That search was also the wrong
question: "`$a4ea` has zero references" only means zero references *in the code
Ghidra disassembled*, and a caller it never reached looks identical to a caller
that does not exist.)

The caller is `$96be`, and its surroundings are the whole answer:

```
$96b6  bsr $a4bc
$96ba  move.w $6ab8,-(a7)     ; push a repeat count
$96be  bsr $a4ea              ; the disc loop
$96c2  bsr $10eac             ; the player control dispatcher
$96c6  bsr $9c52
$96ca  subq.w #1,(a7)
$96cc  bpl $96be              ; and again while it is still >= 0
$96ce  addq.l #2,a7
$96d0  bsr $10f16
```

Two things follow, and the second is the one that matters:

* one pass of that loop is **`$6ab8 + 1` updates**;
* **`$96ba` is in the main loop, not in the VBL handler** -- and the sampling
  point is the VBL (`PC == $8198`). So between two samples the main loop
  completes however many passes it got round to.

Measured, by counting entries to `$a4ea` between samples:

```
golden       1 pass on every one of its 99 ticks
tile_damage  1 pass on every one of its 214
p1_walk      1 on 200 ticks, 2 on 37, and 0 on 37
```

So **"one tick is one update" was a model of the sampling, not of the game**, and
it survived eleven parts because both clean fixtures happen to run exactly one
pass per frame. The oracle was faithful the whole time; `disc-core` was the wrong
shape.

The oracle emits the count as an `updates` column and `GameState::tick` runs
`update()` that many times, which took `p1_walk` from 191 ticks to 223 with both
clean fixtures unchanged. What paces the main loop is not modelled, so `updates`
is a fed input like the rest.

`$6ab8` is emitted too, as `repeat_6ab8`, but it is explanatory: it accounts for
the 2s and not for the 0s.

## Each update pass consumes its own joystick bytes (Part 11g)

Part 11f established that a sampled frame holds 0, 1 or 2 update passes. The
inputs are per **pass**, not per frame, and the reason is inside the loop:

```
$96be  bsr $a4ea      ; the disc loop
$96c2  bsr $10eac     ; -> $10ec6 bsr $d2cc   REWRITES $6da1
                      ;    $10ece bsr $abb2   consumes it
$96cc  bpl $96be      ; and round again
```

So with two passes there are **two different AI bytes**, and a trace that samples
once per frame only records the last. `p1_walk` frame 224, measured by recording
both bytes at `$96c6` on every pass:

```
frame 224   updates 2   pass_ai [$08, $00]   sampled ai_6da1 $00
```

Driving both passes from `$00` loses the walk step the first one made, which is
exactly the frame-224 wall. The oracle emits `pass_joy` and `pass_ai` now,
`GameState::tick_passes` takes one input pair per pass, and `tick` is the
one-pass case. That took `p1_walk` from 223 ticks to 237.

Two details worth keeping:

* **the fire edge runs across the flattened pass sequence**, not per frame,
  because that is the sequence the ST saw. Two passes of `$80` are one edge and
  one held frame, not two edges;
* **an empty `pass_ai` means two different things** -- "zero passes this frame"
  on a Part-11g trace, and "no such column" on an older one. Only `updates`
  tells them apart, so it is the authority on the count and the arrays supply
  only the bytes. Getting that wrong put the fixture back to 191 for one
  measurement.

## A frame is not one outer iteration either (Part 11h)

Part 11f split a frame into passes. There is a **second** count, and it is not
the same one:

```
$96b6  bsr $a4bc      ; the collapse advance -- ABOVE the repeat target
$96be  bsr $a4ea      ; the disc loop          <-- $96cc branches HERE
$96c2  bsr $10eac     ; the player dispatcher
$96cc  bpl $96be
```

`$96cc bpl` goes back to `$96be`, not to `$96b6`. So the collapse advance runs
once per **outer** iteration while the disc loop and the dispatcher inside it run
once per pass -- and an outer iteration is not once per sampled frame either.
Measured over `walkleft`: **237 outer iterations across 275 sampled frames**,
absent on exactly the frames that carry two passes.

```
frame  191:1/1  192:0/0  193:1/2  194:0/0  195:1/2  ...   (outer/updates)
```

The size of the error, measured with `--watch 0x779e 0x77b0`:

```
frame 188  $a390  st $779e            cell 6 destroyed, slot claimed
frame 189  $14c7a                     47 more advances, but only on
 ..  271                              frames that ran an outer iteration
frame 273  $14c76 addq.b #2,(a6)      the list ran out
frame 275  $14bb2/$14bb8              cell 14's type is cleared
```

**85 frames, not 48.** One collapse step per frame cleared cell 14 at 238, and
`p1_walk`'s wall was exactly there. With `outer` emitted and
`GameState::tick_frame(passes, outer)` stepping the collapse that many times:
**237 -> 255 ticks**. The earlier collapse in the same fixture (claim 93, clear
143) is 50 frames because that stretch runs one outer iteration per frame, which
is why the single-slot timing looked right for two parts.

Two things fell out of reading `$a4bc`:

* **the collapse takes 50 steps, not 49** -- 48 list entries, one step for
  `$14c72`'s terminator, one for the clear. The 49 in the Part 10e note counted
  from the wrong end;
* **RETRACTION: there are four collapse slots, not one.** `$a4bc` is
  `moveq #3,D6 ... lea ($10,A6),A6; dbra D6` over `$779e`, `$77ae`, `$77be`,
  `$77ce`. `disc-core` still models one, which is correct only while no trace
  destroys two tiles inside 50 steps -- none of the three fixtures does.
  `discr-pu8`.

The new wall is frame 256, `players[1].world_y` -- player 2's own state handler,
already waived under `discr-b6x`. Every non-waived row matches on that frame.

## Player 2's knock-down, and its mirror (Part 11i)

`p1_walk`'s frame-256 wall was player 2 being struck. Three pieces were missing,
and all three are mirrors of code already modelled for player 1.

**The cascade, `$ca12`-`$ca78`** — the tail of player 2's strike, reached from
`$ca0e` and called from the disc loop at `$a656`:

```
$ca12  cmpi.b #$1,$6d2e ; beq $ca7a     ; a walk/turn/throw is not knocked over
 ..    #$2, #$15, #$16, #$3, #$4        ; six pending states, then
$ca42  tst.w ($a,A5) ; bmi $ca5e        ; the ALREADY-NEGATED dir_kind
$ca48  lea $4774.w,A0 ... $6d2e = #$c   ; positive -> state 12
$ca5e  lea $4764.w,A0 ... $6d2e = #$b   ; negative -> state 11,
$ca72  move.w #$ffff,($a,A5)            ;   and the disc leaves at exactly -1
$ca7a  $6d4e = #$141f6 ; $6d52 = #$4    ; the interrupt arm, two fields we do
$ca88  tst.w ($a,A5) ; bpl              ;   not model, and $11256's mirror:
$ca8e  move.w #$ffff,($a,A5)            ;   force an outgoing disc to -1
```

Set against `$111da`, this is `$11226`/`$11210` with **the polarity flipped**:
a negative `dir_kind` sends player 1 to state 12 and player 2 to state 11. Each
player is knocked the way the disc was already going, so the sign that means
"away" for one means "toward" for the other.

**States 11 and 12** were shared between the tables and are not:

| | player 1 | player 2 |
|---|---|---|
| state 11 | `$1056a cmpi.w #$02,$6ca6; ble` then `subq.w #1` | `$be6a cmpi.w #$45,$6d26; bge` then `addq.w #1` |
| state 12 | `$10592 cmpi.w #$19; bge` then `addq.w #1` (both arms) | `$be90 cmpi.w #$32; ble` then `subq.w #1` (both arms) |

Both gate the step on the animation cell changing (`cmp.l $6ce4` / `cmp.l
$6d64`), so a player travels one row per cell, not per frame.

**The two sequences**, read out of the image at `--window 0x4760 0x4790` rather
than guessed. A cell is six bytes -- a four-byte frame-block pointer and a
two-byte hold -- and a zero pointer terminates:

```
$4764  ptr $3a40 hold 4 | ptr $3a56 hold 4 | $4770 ptr 0     state 11
$4774  ptr $3a6c hold 4 | ptr $3a82 hold 4 | $4780 ptr 0     state 12
```

`[4, 4]` each, the same shape as player 1's `$2d50`/`$2d60`. The trace confirms
it independently: player 2's `anim` column reads 18276 = `$4764` on the frame it
enters state 11.

**`p1_walk` 255 -> 271.** The new wall is frame 272 and it is a different
animal: `players[0].facing` = `$12`, because `$113e2`-`$113fe` -- **player 1's
anticipation cascade**, the mirror of `$cb2c` -- installs steering hook `$a71a`,
enters `$2bfe` and sets state 18. That is `discr-ovl.1` exactly, now located to
the instruction, and three ticks from the end of this fixture.

## Player 1's anticipation cascade, and the owner byte (Part 11j)

`$112f4`-`$1147a` is player 1's anticipation cascade, the tail of `$10fd8` and
the exact counterpart of player 2's `$cb2c`. Every non-crossing and every
body-box miss branches into it -- `$10fee`, `$10ff6`, `$11000`, `$11008`,
`$11108`, `$11114`, `$11122` -- so a disc that fails to hit a player is a disc
that player starts tracking. That answers `discr-ovl.1` from the player-1 side:
`$11334` and `$11372` install `$a78e`, `$113e2` installs `$a71a`.

Set side by side with `$cb2c`, **the depth axis mirrors and the X axis does
not**:

| | player 2 (`$cb2c`) | player 1 (`$112f4`) |
|---|---|---|
| idle + `facing != 7` | `$cb2c`, `$cb34` | `$112f4`, `$112fc` -- the same |
| disc travelling away | `dir_kind > 0` (`bmi`/`beq`) | `dir_kind < 0` (`bpl`) |
| owner byte | `== 0` (`bne` exits) | `!= 0` (`beq` exits) |
| near edge | `depth - reach` (`$cb52`) | `depth + reach` (`$11316`) |
| bonus-5 arm | `$6d9a`, `- $32` | **`$6d1c`**, `+ $32` |
| exit when | shallower (`$cb6a`) | deeper (`$11330`) |
| wide hook | `$a7d8` | `$a78e` |
| narrow window | `[depth-$c, depth-$a]` | `[depth+$a, depth+$c]` |
| the X ladder | `$c`/`$18` right, `$f`/`$22` left, `$c` probe | **identical** |
| reach | `$466a`, state `$1b` | `$2c56`, state `$1b` |
| intercept | `$4612`, state `$12`, hook `$a816` | `$2bfe`, state `$12`, hook `$a71a` |

Three sign flips, one different bonus word, four different addresses -- and
seven identical constants in the X ladder, because X is the same direction for
both players and depth is not. `$2bfe` is `[6, 6, 6, 6]` and `$2c56` is
`[6, 6, 4, 4, 4]`, matching `$4612` and `$466a` cell for cell.

`can_stand` also has a per-player threshold that was hardcoded: `$113ba` and
`$11432` test `$6ca6` against `$e`, where `$cbf6`/`$cc6e` test `$6d26` against
`$3a`. Both are "greater than", so the polarity is *not* inverted the way the
movement code's is.

### The owner byte moves, and that was the real blocker

Transcribing all of the above changed nothing at first: the cascade still never
fired. The reason is not in the cascade at all. Its third gate is `$1130e tst.b
($11,a5); beq` -- the disc's **owner byte** -- and `disc-core` has no writer for
that field. `types.rs` said "every trace we have reads 0 on every live slot", and
that stopped being true: in `p1_walk`, disc 0 reads `$ff` from frame 268 on. With
the field frozen at its frame-0 seed, `aim` was `One` for the whole replay and
the gate rejected every disc.

So `disc+$11` joins the fed inputs -- **the first disc-side field this replay has
ever had to feed**, and the banner `tracecheck` prints says so. What writes it is
still open (`discr-ovl.2`), and so is which value names which player; feeding it
is what lets the rest be measured rather than guessed.

**`p1_walk` is now clean: 274 of 274 ticks, nothing waived.** All three fixtures
are clean for the first time in the project.

## Part 12 (tiles) — a re-measurement, four collapse slots confirmed, $6d9a still open

### discr-qsf, closed: the frame-238 divergence was already fixed

Re-measuring the bead's own reproduce line (`cargo run -q -p disc-tools --bin
tracecheck -- tests/fixtures/p1_walk.ndjson`, with and without
`--skip-waived`) now returns 274/274 clean both ways. The bead was filed at
`400606e` before Parts 11h/11i/11j landed; `b66be7a` (Part 11h, "the collapse
advances per outer iteration, not per frame -- 237 -> 255") names the exact
bug in its own commit message and fixes it: the collapse was advancing once
per sampled frame instead of once per `$96b6` outer-loop iteration, so
`disc-core` raced ahead of the ST and destroyed `tiles[14]` 47 frames early
(frame 238 instead of the ST's real timing). `7dba893` (Part 11i) and
`19ac647` (Part 11j) carried the fixture the rest of the way to 274. See
`reports/part12-tiles.md` for the full citation.

### discr-pu8, closed: four collapse slots, and $a38c's scan confirmed

Ghidra (`tmp/ghidra_proj`) confirms the claim loop byte-for-byte:

```
$a386  moveq  #3,D6
$a388  lea    $779e.w,A2
$a38c  tst.b  (A2)          ; loop top
$a38e  bne.b  $a3b2         ; busy -- try the next slot
$a390  st     (A2)          ; free -- claim it, then init the slot
$a3b2  lea    ($10,A2),A2   ; next slot, 16 bytes on
$a3b6  dbf    D6w,$a38c
```

One function, `sub_a354` (152 bytes, `$a354`-`$a3ec`), four `dbf` iterations
over `$779e`/`$77ae`/`$77be`/`$77ce`. **`$a38c` scans all four for the first
free slot and claims that one; it does not queue behind a busy slot and does
not merely test `$779e`.** If all four read busy, the `dbf` exhausts and falls
through unclaimed -- the destroy's collapse animation is silently dropped,
which `tile::damage` now models by finding no free slot and doing nothing
further.

Caveat, not a retraction: the pre-existing `$a4bc` citation for the *advance*
loop (walks all four slots per outer iteration, `jsr $14ba4` per busy one,
called from `$96b6`) could not be re-confirmed in this Ghidra snapshot --
`$a4bc` sits in a span with no defined function and zero xrefs to `$14ba4`.
`disc-core`'s `tile.rs`/`lib.rs`/`disc.rs` now model all four slots
(`COLLAPSE_SLOTS = 4`); every gate is green with the same numbers as the
single-slot model (behavior-preserving on all three fixtures, as the modelling
was only ever correct while no trace destroyed two tiles within 50 collapse
steps).

A two-collapse fixture (to actually exercise slots 1-3) was attempted and
found not cheap with the tools available in the time budgeted -- idle,
fire-held and right+fire-held oracle runs over `seeds/match_challenge.seed`
all produced the identical single destroy at the identical frame, meaning a
short scripted hold doesn't perturb which tile the opponent's own play
destroys. The recipe for a deliberate one is in `reports/part12-tiles.md`.

### discr-z8m, still open: a null result from a live rally watch

A Hatari change-watch on `$6d9a` (`scenarios/watch_6d9a_rally.yaml`, `mode:
training` -- no bonus board, so the bead's named suspect `$9aa2` cannot be
the explanation here regardless) over ~389 frames of a confirmed-live round
(round clock visibly decrementing across three screenshots) found **zero
writes**. `$9aa2` is ruled out as the source of *this* null by mode alone;
`$6d9a`'s actual writer is still unlocated. See `reports/part12-tiles.md` for
the caveat on what "confirmed live" does and doesn't establish here.

## The ten tier-1 states, decoded (Part 12)

`bd discr-75o` named ten states whose handler addresses were tier-1 known but
whose behaviour was not: 5, 11, 14, 19, 20, 21, 23, 24, 27, 31. By the time
this part started, four had already been modelled by other work -- 11 and 23
in Part 10b/10d (`struck_down`/`dead`), 20 in Part 10b (`turn`), 27 in Part
10i (`run_out`, via `STATE_REACH`) -- so what follows covers the remaining
six: 5, 14, 19, 21, 24, 31. All six were pulled from Ghidra 12.1.3 over
`seeds/rally_f100.seed`; two of the six (19, 21) turn out to be fully
determined by fields this crate already carries and are implemented in
`player.rs`; the other four end in mechanisms (an undecoded throw commit, an
undecoded parabola calculation, an opaque installed hook, and -- state 31 --
an unconditional round reset) this crate has no representation for, and are
documented rather than guessed at.

### State 19, player 1's third catch state: decoded and implemented

`$108f4` sits in the `$10e2c` table (confirmed by reading the table's raw 32
longwords directly: entry 19 is `$108f4`, matching Part 8's tier-1 list
exactly). It is gate-for-gate the same shape as state 18's commit:

```
$108f4  move.b #$13,$6ca9
$108fa  if $6cda is $2c3c or $2c4c -> commit, else just run the animation
$10912  btst #$7,(a0) ; beq out        ; fire must be HELD
$1091a  btst #$1,(a0) ; bne out        ; down must not be
$10922  if $6d0a == $6d0c -> out       ; a disc must be available
$1092e  addq.w #6,$6ca2                ; six units RIGHT, in one step
$10932  animation $2bb4 (5 cells of 4, read out of the image)
$10940  move.b #$10,$6cae              ; and straight into state 16
```

Two differences from `intercept()` (state 18) are real, not noise: the step
is `addq` (RIGHT), where state 18's is `subq` (LEFT); and the committed state
is 16 (`STATE_THROW_LEFT`), where state 18 commits to 15
(`STATE_THROW_STANDING`). Both are exactly what the disassembly shows, so
`catch19()` in `player.rs` mirrors `intercept()` with those two constants
flipped -- `CATCH19_STEP`, `STATE_THROW_LEFT` -- and the release checkpoints
(`CATCH19_RELEASE_A`/`B`) are their own pair, not a reuse of state 18's.

What is **not** decoded is what enters state 19 in the first place:
`anticipate()`'s X ladder only ever picks `STATE_INTERCEPT` or `STATE_REACH`,
so a third branch this crate has not found yet must select state 19 --
exactly matching `p2_hit_test`'s own honest note ("`$cad0` is state 19's; no
fixture reaches state 19"). No fixture and no Hatari probe in this part
reached it either. Fully implemented, fully gated on fields `disc-core`
already carries (`anim_cursor`, `discs_out`, `disc_cap`, `world_x`), tested in
`player.rs` (`state19_commits_on_its_release_frame_with_fire_and_a_disc`,
`state19_is_a_pass_through_off_its_release_frame`) -- but **not
cross-validated against a live trace**, because nothing yet drives player 1
into it.

### `intercept()` had player 2's gate values hardcoded for both players -- `retract`

Decoding state 19 meant reading state 18's real player-1 address for
comparison, and that turned up a bug in code this crate already ships.
`intercept()` (state 18, `STATE_INTERCEPT`) is called for whichever player
reaches state 18, but its release-checkpoint constants,
`INTERCEPT_RELEASE_A`/`B` (`$4624`/`$4634`), are **player 2's** -- read off
`$c19c`/`$c1a8`. Player 1's own copy of the handler is at `$1089e` (four bytes
past the state-17 stub, `$1089a`, which is why nothing had gone looking for
it), and it gates on `$2c10`/`$2c20` instead:

```
$1089e  move.b #$12,$6ca9
$108a4  if $6cda is $2c10 or $2c20 -> commit, else just run the animation
$108bc  btst #$7,(a0) ; beq out
$108c4  btst #$1,(a0) ; bne out
$108cc  if $6d0a == $6d0c -> out
$108d8  subq.w #6,$6ca2                ; the same LEFT step as $c1d0
$108dc  animation $2bdc (5 cells of 4)
$108ea  move.b #$f,$6cae               ; the same target state, 15
```

The step magnitude and sign and the committed state are identical to player
2's -- only the two gate checkpoints differ, because each player's animation
data lives at its own addresses (`$2c10` is exactly `ANIM_P1_INTERCEPT`'s
fourth cell, `$2bfe + 3*6`, matching how `$4624` is `ANIM_INTERCEPT`'s fourth
cell, `$4612 + 3*6`). Before this part, a player 1 that reached state 18 and
its real release frame would stamp `$6ca9`, hold the pose, and **never
commit**, because its `anim_cursor` would never equal player 2's addresses.

This is a real divergence from the ST, but it is currently inert: the only
fixture that puts player 1 in state 18 is `p1_walk.ndjson`, and its window
ends three frames after entry (272-274) with `discs_out == disc_cap` (`2 ==
2`) throughout -- the disc-availability gate alone would have blocked the
commit regardless of which checkpoint constants were used, and the recorded
`anim_cursor` (`$2bfe`, i.e. `11262`) never reaches either checkpoint within
the window either. So all five gates in this bead's list still pass at their
required counts with the fix applied. `intercept()` now takes `who` and reads
the correct pair via `intercept_release()`; `INTERCEPT_RELEASE_A`/`B` keep
their names and player 2's values, documented as such.

### State 21: decoded and implemented, an unconditional slide

`$109aa`, immediately after state 20's turn transient in the table (entry
21), is a plain three-unit slide left with **no gate at all**:

```
$109aa  move.b #$15,$6ca9
$109b0  subq.w #3,$6ca2          ; world_x -= 3, EVERY call, unconditionally
$109b4  [the 20-byte tail copy, inlined -- the same shape state 20 duplicates]
$109ec  tst.l (a1) ; bne out     ; sequence not done -> just save the cursor
$109f2  lea $2c78,a1 ; ... ; $6cae = 0   ; ended -> straight to idle
```

No held-input test, and -- checked directly against the disassembly, not
inferred -- no clamp at `WALK_X_MIN`/`WALK_X_MAX` either, unlike `walk()`.
And the sequence running out lands on state 0 directly, not through
`player.pending_state` the way state 20's does. Entry 22 (`$10a0e`) is the
mirror, `addq.w #3` instead of `subq.w #3`, otherwise byte-identical; it is
left opaque, since inferring its body from 21's would be exactly the kind of
guess the house rules forbid, and neither state was reached by any fixture or
by the Hatari probe below.

Implemented as `slide_left()`, which is `world_x -= SLIDE_STEP` followed by
`run_out()` -- `run_out` already is exactly this handler's ending, so no new
machinery was needed. Tested in `player.rs`
(`state21_slides_unconditionally_and_ends_at_idle`); not cross-validated live,
since nothing in this part's probing reached it.

### States 5, 14, 24 and 31: decoded, not implemented, and one live cross-validation

These four were pulled from the same Ghidra pass but are **not** wired into
`step()`, because each one's real behaviour depends on a mechanism this crate
does not carry, and guessing at any of the four would be the speculative
implementation the house rules forbid.

**State 5** (`$fb6e`), player 1's rise, entered from idle under Up
(`$f222`-`$f238`):

```
$fb6e  move.b #$5,$6ca9
$fb74  btst #$7,(a0) ; beq $fb94        ; fire not held -> just check Up
$fb7c  btst #$1,(a0) ; bne $f306        ; fire+Down -> an aerial-throw commit,
$fb84  d0 = $6d0a (discs_out)           ;   undecoded (see idle()'s own $f21e
$fb88  cmp.w $6d0c,d0 (disc_cap)        ;   bmi to the same $f306, bd discr-b6x)
$fb8c  bne $f306                       ; fire+!Down+a disc available -> the same
$fb90  bclr #$7,(a0)                    ; otherwise: consume fire, keep rising
$fb94  btst #$0,(a0) ; beq $fb9c        ; Up not held -> straight to idle ($2c78)
$fbb2  addq.w #1,$6ca6                  ; world_y += 1, EVERY frame Up is held
$fbb6  cmpi.w #$19,$6ca6 ; ble $fbde    ; below the clamp -> continue rising
$fbc0  move.w #$19,$6ca6                ; at the clamp: pin world_y at 25
$fbc6  animation $2a22 (8 cells of 4)
$fbd4  move.b #$18,$6cae                ; -> state 24
$fbde  btst #$3,(a0) ...                ; Right/Left while rising: a large
                                        ;   undecoded column/parabola lookup
                                        ;   and bsr $aae8 -- an aerial attack
                                        ;   calculation, a NEW undecoded wall
```

**State 14** (`$106b2`), Right+Fire from idle -- already tier-1 known as an
entry condition, newly decoded as a body:

```
$106b2  move.b #$e,$6ca9
        [the same inlined 20-byte tail copy]
$106f6  btst #$3,(a0) ; beq $10754      ; Right no longer held -> idle
$106fc  cmpi.w #$98,$6ca2 ; beq $10754  ; already at WALK_X_MAX -> idle
$10704  addq.w #2,$6ca2                 ; else: two units right
$10708  [primes a sound cue: $6c5c, Timer A via $63312, $fa1f/$fa19]
$10732  animation $300a (6 cells of 4)
$10740  move.b #$1f,$6cae               ; -> state 31
$10746  move.l #$1334a,$6cce            ; installs a function-pointer hook
$1074e  clr.w $6cd2
```

**State 24** (`$10ac4`), the hover atop a state-5 rise, is the same shape as
14 with the direction bit swapped for Up and a second, **unclamped**
`world_y += 1` on the frame it hands off to 31:

```
$10ac4  move.b #$18,$6ca9
        [the same inlined tail copy]
$10b08  btst #$0,(a0) ; beq $10b5e      ; Up no longer held -> idle
$10b0e  addq.w #1,$6ca6                 ; world_y += 1 again, no clamp this time
$10b12  [the same sound-cue priming as state 14]
$10b3c  animation $2f92 (6 cells of 4)
$10b4a  move.b #$1f,$6cae               ; -> state 31
$10b50  move.l #$1334a,$6cce            ; the SAME hook address as state 14's
$10b58  clr.w $6cd2
```

**State 31** (`$10dda`) is where both paths land, and it is not a normal
transient at all:

```
$10dda  move.b #$1f,$6ca9
$10de0  st.b $6d2d      ; SET, unconditionally, EVERY call -- player 2's own
                        ;   +$0d, the exact field $a564 polls to retire every
                        ;   disc and end the round, and the exact field $f1b4
                        ;   sets three instructions after player 1 enters
                        ;   state 23 (death)
$10de4  st.b $6cac      ; SET, unconditionally -- player 1's own "down" flag,
                        ;   the same field idle() tests to enter the death path
$10de8  if $6cda's sequence pointer is null -> $10aba, STATE 23's OWN
                        ;   terminal return (bumps $6cab by 3, $6c83 by 1) --
                        ;   otherwise the usual inlined tail copy, and rts
                        ;   (no bra to $f1c4: no further state change here)
```

So reaching state 31 sets the *same two* fields the disc-damage death path
sets -- via a completely different trigger, a sustained rise rather than
being hit -- and shares state 23's own terminal code once its sequence runs
dry. Nothing in `Player`/`GameState` models "end the round"; inventing a
field or event for it here would be exactly the guess this bead exists to
prevent, so states 24 and 31 stay opaque pass-throughs in `step()`, same as 5
and 14.

#### Live cross-validation: `dumps/state5_hunt`

A scenario (`mode: training`, watching `$6cae` while holding Up for 90
frames) drove player 1 through this entire chain in one run and recorded
**exactly** the three write sites this decode predicts, in order:

```
[watch] $6cae: 3 hit(s) from PC $f23e, $fbda, $10b50
```

`$f23e` is the `bra $f1c4` immediately after `$f238`'s `move.b #$5,$6cae` --
idle entering state 5 on Up. `$fbda` is the `bra $f1c4` immediately after
`$fbd4`'s `move.b #$18,$6cae` -- state 5's rise clamp handing off to state 24.
`$10b50` is the hook-install instruction immediately after `$10b4a`'s
`move.b #$1f,$6cae` -- state 24 handing off to state 31. (Hatari's
change-watch reports the PC reached *after* the write completes, one
instruction past each `move.b`, consistently across all three hits.)

A dump taken 10 frames later (`dumps/state5_hunt/b.bin`, VBL 17922 against
the baseline's 16684) shows player 1 fully reset: `$6cae` = 0, `$6ca6` = 18
(the starting `world_y`, not the clamped 25), `$6cac` = 0, and player 2's
`$6d2d` = 0 -- all back to their pre-jump values. **Entering state 31 forces
an immediate round reset**, confirmed live and not just from the disassembly:
sustaining an upward jump is a way to end the current round outright, not (as
first guessed, before this run) some kind of aerial power move that leaves
the jumping player briefly vulnerable.

### Status table

| state | address | semantics | status |
|---|---|---|---|
| 5 | `$fb6e` | rise on Up; `world_y += 1`/frame, clamp 25 -> state 24; fire (+availability) or fire+Down bail to the undecoded `$f306`; Left/Right bail to the undecoded `$aae8` parabola calc | decoded, not implemented |
| 11 | `$10554` | knocked down, sinks one row per animation cell, floor at `world_y` 2 | decoded + implemented (Part 10b/11i) |
| 14 | `$106b2` | Right+Fire windup; on completion, Right still held -> `world_x += 2`, sound cue, -> state 31 with the `$1334a` hook installed; else -> idle | decoded, not implemented |
| 19 | `$108f4` | player 1's third catch/commit state; gate on `anim_cursor` + fire + !Down + a disc available; commits `world_x += 6` -> state 16 | decoded + implemented (this part) |
| 20 | `$1094a` | the turn transient, redirects through `pending_state` | decoded + implemented (Part 10b) |
| 21 | `$109aa` | unconditional `world_x -= 3`/frame, no gate, no clamp; sequence end -> idle directly | decoded + implemented (this part) |
| 23 | `$10a72` | out of energy; terminal, tests the terminator before copying | decoded + implemented (Part 10b/10d) |
| 24 | `$10ac4` | the hover atop state 5's rise; on completion, Up still held -> unclamped `world_y += 1`, sound cue, -> state 31 with the same `$1334a` hook; else -> idle | decoded, not implemented |
| 27 | `$10c8a` | reaching for a disc without moving, runs its sequence out via `run_out` | decoded + implemented (Part 10i) |
| 31 | `$10dda` | reached from 14 or 24; unconditionally sets player 2's `$6d2d` and player 1's `$6cac` every call -- an immediate round reset, live-confirmed | decoded, not implemented |

Plus one correction to code that shipped before this part: `intercept()`
(state 18) was using player 2's release checkpoints for both players; it now
takes `who` and reads the right pair (`intercept_release()`), currently inert
against both committed fixtures (see above).

Two new walls this part surfaced and did not chase, both worth a follow-up
bead of their own rather than folding into `discr-75o`: `$f306` (state 5's
and idle's shared fire+Down / fire+available-disc branch -- an aerial throw
commit) and `$aae8` (the column/parabola calculation state 5's Left/Right
branch runs while rising, and the `$1334a` hook both 14 and 24 install before
handing off to state 31).

## Player 2's AI policy: the table is fully decoded, two of twenty rows are implemented (Part 12, discr-b6x)

Full detail and every command in `reports/part12-ai.md`. Summary, in the
same voice:

`$d2cc` (the writer of `$6da1`, Part 10) walks a 20-entry priority table at
`$efa8` once a frame: `priority:u8, threshold:u8, test:fn, action:fn,
identity:fn`, 14 bytes each, terminated by an all-zero 21st row at `$f0c0`.
Ghidra's `dis`/`dec` cannot read this table at all -- it is data, and
`getInstructionAt` failing silently falls back to `getInstructionAfter`,
which is how the first attempt printed a confident wrong answer starting at
`$f104`. The fix was a raw byte read of `discram.bin` (file offset == ST
address, checked against `$d2cc`'s own opcode bytes) -- every table row and
raw table (`$1556`/`$155e`/`$15fe`) below came from that, not from the
disassembler.

**The dispatch mechanism is fully decoded.** A row's reaction roll (`$d2f8`-
`$d308`: `$6c5d += $6ab5`, fail if the running total exceeds the row's
threshold) runs *before* its test, for every row whose priority exceeds the
currently latched one -- so `$6c5d`'s own evolution depends on which rows
were eligible, which depends on the latch, which is the outcome of earlier
rolls. A `u8` roll can never exceed a threshold of 255, and exactly two of
the twenty rows carry threshold 255 (priority 50, the escape at `$e0d8`; and
priority 30, the avoid at `$e158`) -- those two, and only those two, do not
depend on `$6c5d` at all. They share an identity pointer (`$e290`), so the
ST treats them as one latch: once either fires the other cannot preempt it
until the maneuver ends on its own. The "plan" mini-VM their shared action
(`$e214`) and step executor (`$e30a`) compile into is fully decoded too: a
buffer at `$6dac` holds a step (a function pointer plus its parameters), run
once a frame by the identity, that walks player 2 toward a target
`(world_x, world_y)` and ends the maneuver when player 2's own state enters
one of four values or the target is reached within a small tolerance box.

**Those two rows are implemented**, in `crates/disc-core/src/ai.rs`
(`Ai::p2_policy`) -- entry 0's escape (floor cell destroyed under player 2,
looked up via three raw tables at `$1556`/`$155e`/`$15fe`) and entry 1's
avoid (an active disc in a box built from player 2's own hit box, side-
stepped via the same floor-cell check `$d062`/`$e2d0` use independently at
two addresses, falling back to entry 0's escape table exactly as the ST
falls through from `$e1e4`/`$e202` into `$e112`).

**Why not all twenty**: the other eighteen rows all carry threshold < 255,
so whether they fire depends on `$6c5d` -- a byte no fixture feeds, shared
with at least three call sites outside `$d2cc` entirely, and (argued in the
report) not reconstructable after the fact even from a fixture with a known
starting value, because its own increments are coupled to the AI's latch
history all the way back to a reset this project has never observed. That is
the actual wall, address-cited, not a shortage of transcription time.

**Measured, not assumed**: `cargo test -p disc-core --lib ai::agreement --
nocapture` compares `Ai::p2_policy` against `ai_6da1`/`pass_ai` directly from
the three committed fixtures (only on single-pass ticks -- Part 11f/11g's
own granularity wall applies here too). Golden 18/99, tile_damage 61/214,
p1_walk 22/200 -- both simpler fixtures never trigger rows 0/1 at all, so
their number is exactly the fraction of ticks whose own byte is already 0
(everything else in these traces comes from the eighteen undecoded rows).
`p1_walk` does trigger row 1 once, correctly, for most of its run; the
exception is four ticks right where player 2 enters state 11 (knocked down,
`$ca12`) -- this module keeps steering (nothing in `$e158`'s or `$e30a`'s own
exclusion lists names state 11), but the ST's own byte there (`$06`) is not
what a fresh steer computes either (`$04`), and is not silence. Left open
rather than papered over with a guessed fifth exclusion state.

**Also corrected in the same phase**: this section originally would have
cited `discr-ovl.2` ("every trace reads owner 0") as the reason rows 2-4
can never fire -- that closed mid-phase (`reports/part12-owner.md`,
`tests/fixtures/handover.ndjson`), and `p1_walk.ndjson` itself already has
`disc[0].own` flip nonzero at frame 220. The narrower, still-true claim: in
every case on hand, ownership turns nonzero at the same far-wall bounce that
flips `dir_kind` negative, and a negative `dir_kind` is exactly what
`$cea6`'s own candidate scan excludes -- so rows 2-4 still don't fire here,
but not for the reason first drafted.

The waiver (discr-b6x) stays open. `Ai` is not wired into `GameState::tick`;
feeding `$6da1` from it today would fail `core-check` in exactly the frames
above.

**Correction (Part 12b, `reports/part12-rng.md`)**: the line above about
`$6c5d` — "not reconstructable... a reset this project has never observed"
— was wrong on the "never observed" part. A raw byte-pattern scan of
`discram.bin` (independent of Ghidra's `xref`, which only sees code its own
analysis already disassembled) found an eleventh-hour reset: `$968a`,
`move.b $6ab5,$6c5d`, unconditional, inside an undissasembled init block
(clears `$6c83`/`$6c9c`/`$6ab8`/`$6c5a`/`$6ab6` too, then chains ~9 `bsr`s to
other subsystem setup). What still holds, now measured live rather than
argued: two independent cold boots of the identical scripted scenario
(`scenarios/watch_6c5d_rng.yaml`, `--fresh --state ''` both times) reach
"match live" 121 VBLs apart on the game's own frame counter despite running
the exact same input script, so `$968a`'s reseed copies a different `$6ab5`
each time — the wall is narrower and sharper now (an anchor exists; its own
input isn't reproducible even under this project's own harness), not gone.
