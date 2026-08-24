# `disc-core` state schema

The single source of truth shared by `disc-core` and the tracecheck tool. One
row per field that a trace comparison looks at: what it is called in Rust, what
type it has, which Atari ST address it mirrors, and whether a trace comparison
is expected to check it.

Everything here comes from `docs/disc-notes.md`. If a row and the notes
disagree, the notes win and this file is the bug.

## How to read it

* **field path** -- the path from `disc_core::GameState`. `[n]` is an array
  index. A row with `--` has no Rust field: it is ST state `disc-core` does not
  model, listed so nobody has to rediscover that the omission was deliberate.
* **ST address** -- absolute for singletons, `base + n*stride + offset` for the
  arrays. `$6ca0` is player 1, `$6d20` player 2 (same layout, stride `$80`);
  discs are 8 records of stride `$42` from `$6e3e`; tiles are 17 cells of
  stride 8 from `$7616`.
* **status**
  * `compared` -- tracecheck must assert this field matches the ST.
  * `waived:<bead-id>` -- not modelled in this phase because the behaviour
    behind it is an open unknown. The bead is where that unknown is tracked.
  * `excluded:<reason>` -- deliberately never part of core state, and never
    will be. Not an unknown; a scope decision with a reason.

`disc-core` uses plain integers only. There is no fixed point and there are no
floats anywhere in the crate. ST words map to `i16` where the value is signed
or is arithmetic (`hp` is subtracted from), and `u16` where it is an unsigned
tag or index.

## Compared fields

| field path | Rust type | ST address | status |
| --- | --- | --- | --- |
| `frame` | `u32` | `$6ab4` (word) | compared |
| `players[0].world_x` | `i16` | `player+$02` (`$6ca2`) | compared |
| `players[0].world_y` | `i16` | `player+$06` (`$6ca6`) | compared |
| `players[0].facing` | `u8` | `player+$09` (`$6ca9`) | compared |
| `players[0].state_index` | `u8` | `player+$0e` (`$6cae`) | compared |
| `players[0].grid_cell` | `u16` | `player+$10` (`$6cb0`) | compared |
| `discs[n].world_x` | `i16` | `disc+$00` | compared |
| `discs[n].world_y` | `i16` | `disc+$02` | compared |
| `discs[n].world_z` | `i16` | `disc+$04` | compared |
| `discs[n].vel_x` | `i16` | `disc+$06` | compared |
| `discs[n].vel_y` | `i16` | `disc+$08` | compared |
| `discs[n].dir_kind` | `i16` | `disc+$0a` | compared |
| `discs[n].damage` | `i16` | `disc+$16` | compared |
| `tiles[n].tile_type` | `u16` | `tile+$00` | compared |
| `tiles[n].hp` | `i16` | `tile+$02` | compared |

15 compared fields.

Notes on individual rows:

* `frame` -- `$6ab4` is a **word** and wraps; `$8198`'s first instruction is
  `addq.w #1,$6ab4`. `disc-core` widens it to `u32` so the simulation has an
  unambiguous frame number. Compare as `frame as u16`.
* **Only player 1 is compared.** The same five fields exist on player 2 at
  `$6d20` (stride `$80`) and `disc-core` models them identically, but they are
  waived: see `players[1].*` below.
* `players[0].world_x` -- walkable 8..152, `+/-3` per frame (`$f658`,
  `$f86c`, range-checked against 8 and `$98`).
* `players[0].world_y` -- `$f838` tests `> 14` to select the far row.
* `players[0].facing` -- **discr-xfw is answered (Part 10b) and the field is not
  a facing flag.** Every handler opens by writing its own state number to
  `$6ca9`: `$f5e2 move.b #$01`, `$f7f6 move.b #$02`, `$1094a move.b #$14`,
  `$109aa move.b #$15`, and the idle path clears it at `$f1c0` when the joystick
  reads zero. A handler may then change `state_index` before the frame ends, so
  the sampled `+$09` is *the state whose handler ran this frame* while
  `state_index` is the state that will run next -- the one-frame lag the fixture
  shows. 1 and 2 look like left and right only because those are the two walk
  states. `disc-core` writes it from the handler now, so the row passes for the
  right reason. The Rust field keeps its name for the moment; renaming it moves
  this table, the fixture column and the differ together.
* `players[0].state_index` -- index into the 32-entry jump table at `$10e2c`.
  The *number* is compared; the transitions that produce most of those numbers
  are not modelled (see the waivers below), so a trace comparison stops here
  the first time the ST changes state -- golden frame 11, `0 -> 20`,
  **discr-75o**.
* `players[0].grid_cell` -- `8 + column(world_x) + (4 if world_y > 14)`,
  column from the 145-byte table at `$7bfe`; observed 9..16.
* `discs[n].dir_kind` -- the **sign** is the travel direction, flipped by
  `neg.w ($000a,a5)` at `$a606`, not by a comparison; the **magnitude** is the
  kind of disc. Observed `+1`, `-1`, `-3`. This is not a boolean live flag.
* `discs[n].world_y` -- mirrored but **never advanced**: `vel_y` is 0 on all 84
  frames of `dumps/disc_trace` while `world_y` still moves, so nothing in
  evidence integrates it and `$a758` never fired. It matches for 22 golden
  ticks and then goes `81 -> 82` without `disc-core` -- **discr-tan**.
* `discs[n].vel_x` / `vel_y` -- steered `+/-1` per frame toward the aimed
  player's coordinates and clamped to `[-2,+2]` (`$a722`-`$a860`). There is no
  angle table. The rule is exact; **its gate is not decoded and is off in every
  trace we have**, so `disc::step` does not call `disc::steer` -- **discr-217**.
* `discs[n].dir_kind` and `tiles[n].*` are compared but `disc-core` has no
  writer for either during flight: `$a606`'s turn-around condition and `$a31c`'s
  struck-cell index `d5` are both undecoded -- **discr-5w5**. `disc::reflect`
  and `disc::impact` exist as explicit calls with no trigger.
* `tiles[n].hp` -- `$a31c  sub.w ($0016,a5),d6` subtracts the striking disc's
  `+$16`; `$a34a  clr.w d6` clamps at 0, so it is never negative; `$a34c`
  stores it. A second, unidentified writer sets bit 7 of this word
  (`(1,5)->(1,133)`) and clears it later -- **discr-dc0**. Until that is
  explained, a trace comparison on `hp` may see a spurious `+128`.
* `tiles[n].tile_type` -- `{0,1,2}`; 0 = destroyed = unwalkable, and the
  movement code `tst.w`s it as its walkability gate. `$a354` clears it when HP
  reaches 0.

## Waived and excluded

| field path | Rust type | ST address | status |
| --- | --- | --- | --- |
| `players[1].*` (all 5 fields) | as `players[0]` | `$6d20` + `$02`/`$06`/`$09`/`$0e`/`$10` | waived:discr-b6x |
| -- (the opponent's input channel) | -- | `$6da1`, written by `$d2cc` | waived:discr-b6x |
| `discs[n].active` | `bool` | `disc+$10` bit 7 | waived:discr-0fm |
| `discs[n].aim` | `PlayerId` | `disc+$11` | waived:discr-ovl.2 |
| `discs[n].hook` | `SteerHook` | `disc+$12` | waived:discr-ovl.1 |
| -- (round init, score, win) | -- | `$aa50`, `$6d8a` | waived:discr-st8 |
| -- (player state semantics) | -- | `$10e2c` entries 5,11,14,19,20,21,23,24,27,31 | waived:discr-75o |
| -- (player states 16, 17) | -- | `$10e2c` entries 16, 17 | waived:discr-rf9 |
| -- (tile type cleared with hp intact) | -- | `tile+$00`, the frame-119 writer | waived:discr-b4q |
| -- (what places a bonus on a cell) | -- | `tile+$02` bit 7, and `$6e3a` | waived:discr-ovl.4 |
| -- (the bonus effects) | -- | `$6d9a`/`$6d9c`/`$6d9e`, table `$9aa2` | waived:discr-z8m |
| -- (the far wall's tile grid) | -- | `$7596`, damaged by `$9f5e` | waived:discr-ovl.3 |
| -- (the animation engine) | `pending_state`, `anim_hold` | `$6caa`, `$6cda`, `$6ce2` | waived:discr-75o |
| -- (disc screen X) | -- | `disc+$0c` | excluded:projection |
| -- (disc screen Y) | -- | `disc+$0e` | excluded:projection |
| -- (disc sub-record pointers) | -- | `disc+$1a`, `disc+$3e` | excluded:pointer |
| -- (tile trailing long) | -- | `tile+$04` | excluded:always-zero |
| -- (sound, palette, screen base) | -- | `$6c5b`, `$6c5c`, `$6aac`, `$6ab0` | excluded:io |

18 waived or excluded rows: 13 waived against a filed unknown, 5 excluded by
scope. **The count is unchanged from before Part 10 and that is close to the
honest number**: five waivers were resolved and five new ones filed from what
the answers exposed -- a second tile grid, the bonus placer, the hook installer,
the owner polarity, and what retires a disc -- and one row moved from
`excluded:rendering` to `waived:discr-75o`, because `$6cda`/`$6ce2` turned out
to be the state machine's clock rather than a rendering detail. What changed is
not how many unknowns there are but where they sit: six of them used to stand
between the disc model and the fixture, and now one does.
`reports/part10-report.md` has the before-and-after gate numbers. With the 15
compared rows above, that is the whole table, and
`tracecheck`'s header prints these three numbers so a drift between tool and
file is visible on every run.

Why each waiver or exclusion:

* **`discs[n].active`, `aim` and `hook` are now MIRRORED ST FIELDS**, not
  models. Part 10 disassembled `$a4ea`: `disc+$10` is a byte whose bit 7 says
  whether the ST simulates the record (`$a4f0 beq` free, `$a534 bpl` frozen),
  `disc+$11` is the owner the wall handlers flip, and `disc+$12` is a longword
  hook holding one of three steering routines. The old note here -- "the ST
  encoding of an unused slot is not known", "there is no possession" -- was
  wrong on both counts and is retracted.
  They stay waived because each is written by code **outside** the disc loop:
  what retires a disc (**discr-0fm**) and what installs a hook (**discr-ovl.1**,
  the two hit tests `$10fd8`/`$c826`) are not decoded, and the owner byte's
  polarity (**discr-ovl.2**) cannot be settled because every trace reads 0 on
  every live slot -- no trace has ever seen a disc change hands. `tracecheck`
  feeds `active` and `hook` in every tick, the way it feeds `$6c58`, and says so
  in its header.
* Opponent AI (**discr-b6x**): the architecture is now known -- `$10eac` selects
  one-player mode on `$6da0`, `$d2cc` writes a synthetic joystick byte to
  `$6da1`, and player 2's control routine `$abb2` consumes it exactly where a
  human's `$6c59` would go. **So the waived input row is `$6da1`, not `$6c59`**:
  `$6c59` is 0 on every frame of every trace we have. What is still waived is
  the *policy* -- the 20-entry priority rule table at `$efa8`, its 11 test and
  7 action routines, and the sensor pass `$cea6`.
* Round, scoring and win (**discr-st8**): unchanged. `GameState::default()` is
  all zeroes, deliberately not `$aa50`'s round-init state.
* Player state semantics (**discr-75o**, **discr-rf9**): **narrowed in Part
  10b.** Four states are modelled now -- 0 (the idle path inlined in `$f104`,
  not a table entry: `$10e2c`'s entry 0 is null), 1 and 2 (the walks) and 20
  (the turn transient) -- which is every state player 1 reaches in the fixtures
  before its hit test fires. The mechanism behind all 32 is also known: each
  handler ends in the animation tail at `$f1c4`, which counts `$6ce2` down and
  advances the six-byte sequence cursor `$6cda`, and **running off the end of a
  sequence is what changes state**. What stays waived is the other 28 handlers'
  behaviour and the frame-block data their sequences point at, which this crate
  does not carry. State 17 is partly explained -- `$c0c4 move.b #$11,$6d2e` is
  the serve setting it.
* The tile unknowns are **narrower than they were**. Bit 7 of `tile+$02` is now
  known to mark a cell as carrying a bonus, and `$a29c andi.w #$0f` is the
  writer that clears it on pickup, which closes the old discr-dc0. What is left
  is who *places* a bonus (**discr-ovl.4**) and the separate frame-119 writer that
  clears a cell's *type* while leaving its hp (**discr-b4q**).
* The bonus system (**discr-z8m**, retargeted): `$6d9a` is not a damage
  multiplier and not a difficulty rank -- `$824c` clears it on a VBL countdown.
  It is the active bonus code, 1..5, with its payload and duration in the table
  at `$9aa2`. The five effects are documented in `docs/disc-notes.md` and none
  is modelled, because `bonus_6d9a` is 0 on every frame of both fixtures.
* The far wall's grid (**discr-ovl.3**): `$9f5e` is `$a24c` with `$7596`
  substituted for `$7616`, so there is a **second 8-cell bank** this crate does
  not carry and the differ has never looked at.
* Screen X/Y are projection, recomputed from world `(x, y, z)` through LUTs at
  `$a6b2`/`$a6b6`. Comparing them would test the projection, not the rules.

### Resolved in Part 10, and worth knowing why

Five waivers came off, and none of them by modelling harder -- all five were
answered by reading `$a4ea` and its callees in Ghidra.

* **discr-217** (what gates the `$a71a` steering block) -- nothing gates it. It
  runs while `disc+$12` holds a hook. Replaced by `discs[n].hook`/discr-ovl.1,
  which asks the narrower question of what *installs* one.
* **discr-tan** (what advances `disc+$02`) -- `$a556 add.w ($08,a5),d1`. It is
  `world_y += vel_y` after all; `$a640` decays `vel_y` toward zero after the
  integration, so an impulse is invisible at the sampling point. Both this file
  and `docs/disc-notes.md` said not to model it. Both were wrong.
* **discr-5w5** (the collision test) -- the test is `world_z` crossing a wall,
  and `d5` is `column(world_x + 4) + (4 if world_y > $46)`, which lands in
  1..=8. `disc::step` now calls `tile::damage`, and the tile event at frame 70
  of `tile_damage.ndjson` reproduces end to end.
* **discr-dc0** (the tile HP bit-7 writer) -- bit 7 marks a bonus cell; the
  clear is `$a29c`.
* **discr-m4x** (what triggers a serve) -- player 2's animation cursor `$6d5a`
  reaching `$4602` inside `$abb2`. It is a *player-2 behaviour*, so what remains
  of it is discr-b6x.

## Resolved since the first revision

**`player+$04` vs `+$06` -- settled, and the prediction was right.** This file
used to flag that `docs/disc-notes.md` puts world Y at `+$06` (`$6ca6`) while
`$a758 steer_at_p1_y` homes on `$6ca4` = `player+$04`, and to predict that
`+$04` would turn out to be a separate word. Part 9 of the notes measured it:
`+$04` is a **constant 99**, a height/altitude reference, and `$6ca6` (18 / 25
/ 2) is the walkable row. `disc-core` keeps `world_y` on `+$06` and carries
`+$04` as `disc::PLAYER_HEIGHT_REF` rather than as a `Player` field, because it
never changes. Nothing was quietly repointed.

## Open question for the next revision

**Is `player+$09` a facing flag at all?** See the `players[0].facing` note
above and **discr-xfw**. Part 10 did not touch it: it is a player-state
question, and the player state machine (discr-75o) is the next phase's subject. Whoever answers it should also decide whether
`Player::facing` keeps its name: if `+$09` is the previous state, then
`disc-core` currently has no field for facing and an extra one for state, and
the row above is passing for the wrong reason.
