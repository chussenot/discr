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
* `players[0].facing` -- the notes read `$6ca9` as 1 = left (`$f5e2`), 2 =
  right (`$f7f6`). **`tests/fixtures/golden.ndjson` contradicts that** and the
  row stays `compared` so the contradiction stays visible: `+$09` takes
  `{0,1,11,20,23}` for p1 and `{0,1,2,15,16,18,20}` for p2 -- exactly the sets
  their `state_index` takes, one frame behind (96 of 99 p1 frame pairs). It
  looks like the previous state, not a facing flag, and 1/2 is the only region
  where the two readings agree. **discr-xfw**. `disc-core` still writes 1/2, so
  this row is right for the wrong reason wherever it passes.
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
| -- (player-2 inputs) | -- | `$6c59` | waived:discr-b6x |
| `discs[n].active` | `bool` | -- | waived:discr-m4x |
| `discs[n].aim` | `PlayerId` | -- | waived:discr-m4x |
| -- (round init, score, win) | -- | `$aa50`, `$6d8a` | waived:discr-st8 |
| -- (player state semantics) | -- | `$10e2c` entries 5,11,14,19,20,21,23,24,27,31 | waived:discr-75o |
| -- (player states 16, 17) | -- | `$10e2c` entries 16, 17 | waived:discr-rf9 |
| -- (tile HP bit-7 writer) | -- | `tile+$02` bit 7 | waived:discr-dc0 |
| -- (`disc+$02`'s writer) | -- | whatever advances `world_y` | waived:discr-tan |
| -- (the `$a71a` steering gate) | -- | `$a71a`, `$a758` | waived:discr-217 |
| -- (disc collision test) | -- | `$a606`'s condition, `$a31c`'s `d5` | waived:discr-5w5 |
| -- (damage multiplier) | -- | `$6d9a` (`$a314`, `$a32e`) | waived:discr-z8m |
| -- (disc screen X) | -- | `disc+$0c` | excluded:projection |
| -- (disc screen Y) | -- | `disc+$0e` | excluded:projection |
| -- (disc sub-record pointers) | -- | `disc+$1a`, `disc+$3e` | excluded:pointer |
| -- (tile trailing long) | -- | `tile+$04` | excluded:always-zero |
| -- (animation cursor, countdown) | -- | `$6cda`, `$6ce2` | excluded:rendering |
| -- (sound, palette, screen base) | -- | `$6c5b`, `$6c5c`, `$6aac`, `$6ab0` | excluded:io |

18 waived or excluded rows: 12 waived against a filed unknown, 6 excluded by
scope. With the 15 compared rows above, that is the whole table, and
`tracecheck`'s header prints these three numbers so a drift between tool and
file is visible on every run.

Why each waiver or exclusion:

* `discs[n].active` and `aim` are **modelled, not mirrored**. `disc+$0a` is a
  direction/kind word, not a live flag (Part 7), so the ST encoding of an
  unused slot is not known; and there is no possession -- a disc is always in
  flight and always homing on a target player, whose identity is implied by
  which steering routine runs (`$a71a` reads `$6ca2`, `$a7d8` reads `$6d22`).
  Both are filed under **discr-m4x** because the serve trigger is what decides
  when a slot goes live and who it aims at.
* Opponent AI (**discr-b6x**): `disc-core` takes both players' `Input` from its
  caller. Whatever drives player 2 on the ST is not in this crate. That is why
  `players[1].*` is waived and not merely "expected to be wrong": the trace
  carries no `$6c59` column, so a replay drives p2 with nothing while the ST
  walks it, and those five rows can never match. They were marked `compared`
  until discr-z72; leaving them there made `tracecheck` stop on frame 1 of
  every trace and report a waiver as a divergence, which is the tool
  contradicting this file. `tracecheck --skip-waived` resyncs them from the
  trace each tick instead.
* Round, scoring and win (**discr-st8**): `GameState::default()` is all zeroes,
  which is deliberately *not* the ST's round-init state -- `$aa50` initialises
  the 8 disc records and their sub-records with values not yet recovered.
* Player state semantics (**discr-75o**, **discr-rf9**): the handler addresses
  are known and the state numbers are compared, but what those handlers *do* is
  not. States 16 and 17 have only ever been seen in an oracle autopilot run,
  never in Hatari, so they are not even notes-grade yet.
* Screen X/Y are recomputed from world `(x, y, z)` through LUTs every frame at
  `$a6b2`/`$a6b6`. They are rendering output derived from compared state, so
  comparing them would test the projection, not the rules.
* The four rules discovered *during* wave 2 -- **discr-tan** (`disc+$02`'s
  writer), **discr-217** (the `$a71a` gate), **discr-5w5** (the collision test)
  and **discr-z8m** (`$6d9a`) -- are waived as ST *behaviour*, not as fields.
  The fields they would move (`discs[n].world_y`, `vel_x`, `dir_kind`,
  `tiles[n].*`) stay `compared`: they are mirrored and seeded correctly, they
  match for as long as the ST leaves them alone, and the frame on which one
  stops matching is the most useful number this phase produces. Waiving the
  field would delete that signal; waiving the rule records the gap.
* **discr-g38** (what damps `vel_y`) is CLOSED, resolved by evidence rather
  than by a new model: the damping is the literal at-target decay at
  `$a728`-`$a736`, and the oscillation it was filed against cannot occur
  because `vel_y` is 0 throughout. What was left of it moved to discr-tan.

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
above and **discr-xfw**. Whoever answers it should also decide whether
`Player::facing` keeps its name: if `+$09` is the previous state, then
`disc-core` currently has no field for facing and an extra one for state, and
the row above is passing for the wrong reason.
