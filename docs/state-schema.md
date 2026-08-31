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
  discs are 8 records of stride `$42` from `$6e3e`; tiles are 16 cells of
  stride 8 from `$7616` -- the near bank; the far bank is 16 more from `$7596`
  (`$7596 + 16*8 = $7616`), compared as `tiles_far[n]` since discr-ovl.3
  (Part 12).
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
| `players[n].energy` | `i16` | `player+$76` (`$6d16`) | compared |
| `discs[n].hook` | `SteerHook` | `disc+$12` | compared |
| `discs[n].active` | `u8` | `disc+$10` | compared |
| `tiles_far[n].tile_type` | `u16` | `far_tile+$00` (`$7596+n*8`) | compared |
| `tiles_far[n].hp` | `i16` | `far_tile+$02` | compared |
| `players[0].discs_out` | `i16` | `player+$6a` (`$6d0a`) | compared |
| `players[0].disc_cap` | `i16` | `player+$6c` (`$6d0c`) | compared |
| `players[0].anim_cursor` | `u32` | `player+$3a` (`$6cda`) | compared |
| `players[0].x_delta` | `i16` | `player+$1a` (`$6cba`) | compared |
| `players[0].hit_box` | `[i16; 4]` | `player+$1c`..`+$22` (`$6cbc`..`$6cc2`) | compared |

25 compared fields.

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
* `tiles[n].*` -- **n is 0..15**: a bank is 16 cells (Part 10, discr-ovl.5),
  so 32 tile checks per frame. The three committed fixtures predate that and
  carry a 17-pair grid column; the 17th pair is the word past the bank's end
  (`$7696`, reads `(1,1)`, never a tile) and tracecheck parses and drops it. A
  regenerated fixture emits exactly 16. Note `grid_cell` still *reads* 16 on
  the far-right far-row cell -- the index goes one past the bank on the ST
  too; the value is compared as a value, not used as a tile row.
* `players[n].energy` -- **added in Part 10d.** `player+$76`, the word the
  strike at `$11178` subtracts the striking disc's `+$16` from, clamped to 0 at
  `$111c6`, at which point `$111ca` sets `$6cac` and the player is out. Player 1
  reads 5, 2 and 0 across the golden fixture and `disc-core` produces all three.
  Two bonus branches are not modelled: code 4 skips the subtraction entirely
  (`$1117c`, a shield) and code 1 applies it twice (`$11188`).
* `tiles_far[n].*` -- **added in Part 12, discr-ovl.3.** `$9f5e` is `$a24c`
  instruction-for-instruction with `lea $7596` substituted for `$7616` and
  `$6d1c` for `$6d9a` (`docs/disc-notes.md`, "There are TWO 16-cell tile
  banks"), so the far bank's cell-index formula, HP subtraction and type
  clear are the same code (`disc::disc_cell`, `tile::damage`) already used
  for the near bank -- `disc::step`'s far-wall branch calls them against
  `tiles_far` instead of duplicating them. The oracle has emitted this
  column since Part 10e (`banks`, both bank's 16 cells each); this bead is
  the first to seed *and compare* it rather than only seed it. Same known
  gap as `tiles[n].hp` above (bit 7, discr-dc0) applies identically here --
  `tests/fixtures/farbank.ndjson` hits exactly that gap at its own
  min-agree boundary. **Untested by any committed fixture is the damage
  subtraction itself** (`$a5d6`'s arm, gated on the disc already being
  owned by real player 1) -- every trace that reaches the far wall does so
  on a disc's first arrival (the transfer arm, `$a5e2`), the same
  modelled-but-fixture-unexercised shape as discr-ovl.1's player-1 racket
  path. See `reports/part12-farbank.md`.
* `players[0].discs_out` / `players[0].disc_cap` -- **added in discr-st8
  (Part 12/round).** `player+$6a`/`+$6c`, how many discs this player has in
  play and the cap on that count. Three writers, all in `disc-core` now:
  serve bumps the thrower's `discs_out` (`GameState::update`, `$a9aa`); a
  catch decrements the catcher's (`player::hit_test`/`p2_hit_test`,
  `$caae`/`$cb1e`); and the wall transfer moves BOTH fields for BOTH players
  in lockstep (`crate::round::transfer_at_far_wall`/`transfer_at_near_wall`,
  called from `disc::step`, `$a5ea`-`$a5fa` far wall / `$a62c`-`$a63c` near
  wall). `tests/fixtures/p1_walk.ndjson` reaches the far-wall transfer live
  (frame 220) within its own `--min-agree 274` gate. Full chain:
  `reports/part12-round.md`.
* `players[0].anim_cursor` / `x_delta` / `hit_box` -- **added in discr-rxx.1
  (Part 12).** `player+$3a`/`$1a`/`$1c`..`$22`, reconstructed from the
  animation sequence tables (`crate::player::Anim`/`Frame`, extracted from
  `discram.bin` with their ST address ranges cited in `player.rs`) rather
  than read off the trace. `anim_cursor` is `anim_base + 6*anim_cell`, kept
  live by `enter_anim`/`anim_tick`; `x_delta`/`hit_box` are the two fields of
  each cell's frame block this crate carries, copied in every `anim_tick`
  call the same tick the ST's `$f1ca` would. **Player 1 only** -- `golden`,
  `tile_damage` and `p1_walk` all reproduce these three fields for player 1
  across their WHOLE established `--min-agree` window (99/214/274 ticks)
  with nothing waived or resynced. Player 2's copies stay fed (folded into
  `players[1].*` below): its own sequences are not fully catalogued (a sixth
  table surfaces within the first few ticks of `golden.ndjson` alone, ST
  `$449e`), and the serve gate (`crate::disc::THROW_STATES`) reads player 2's
  `anim_cursor` directly, so a wrong reconstructed value there would desync
  the serve and corrupt the disc simulation for both players --
  `crate::player::step` snapshots and restores player 2's three fields around
  the call that would otherwise touch them, so retiring the feed for player 1
  cannot regress player 2. Full format spec and the per-fixture agreement
  table: `reports/part12-anim.md`.

## Waived and excluded

| field path | Rust type | ST address | status |
| --- | --- | --- | --- |
| `players[1].*` (8 fields) | as `players[0]` | `$6d20` + `$02`/`$06`/`$09`/`$0e`/`$10`/`$3a`/`$1a`/`$1c`..`$22` | waived:discr-b6x |
| -- (the opponent's policy) | -- | `$d2cc`, the rule table at `$efa8` | waived:discr-b6x |
| `discs[n].aim` | `PlayerId` | `disc+$11` | waived:discr-ovl.2 |
| `players[n].throw_dir_kind` / `throw_damage` | `i16` | `player+$6e` / `+$70` | waived:discr-qqt |
| -- (round init, round-over threshold, win/loss) | -- | `$aa50`, `$6c83`, `$6ca0`, `player+$72` | waived:discr-st8 |
| -- (player state semantics) | -- | `$10e2c` entries 5,11,14,19,20,21,23,24,27,31 | waived:discr-75o |
| -- (player states 16, 17) | -- | `$10e2c` entries 16, 17 | waived:discr-rf9 |
| -- (what places a bonus on a cell) | -- | `tile+$02` bit 7, and `$6e3a` | waived:discr-ovl.4 |
| -- (the bonus effects) | -- | `$6d9a`/`$6d9c`/`$6d9e`, table `$9aa2` | waived:discr-z8m |
| -- (the animation engine) | `pending_state`, `anim_hold`, `anim_cell`, `anim_shown` | `$6caa`, `$6cda`, `$6ce2`, `$6ce4` | waived:discr-75o |
| `players[n].reach` | `i16` | `player+$12` | waived:discr-b6x |
| -- (disc screen X) | -- | `disc+$0c` | excluded:projection |
| -- (disc screen Y) | -- | `disc+$0e` | excluded:projection |
| -- (disc sub-record pointers) | -- | `disc+$1a`, `disc+$3e` | excluded:pointer |
| -- (tile trailing long) | -- | `tile+$04` | excluded:always-zero |
| -- (sound, palette, screen base) | -- | `$6c5b`, `$6c5c`, `$6aac`, `$6ab0` | excluded:io |

16 waived or excluded rows: 11 waived against a filed unknown, 5 excluded by
scope. **Three waived rows came off in discr-rxx.1 (Part 12)**:
`players[n].anim_cursor`/`hit_box`/`x_delta`, the generic `discr-75o` rows
that used to cover both players uniformly. Player 1's copies are compared now
(above); player 2's are folded into `players[1].*`, which grows from 5 fields
to 8 rather than gaining a row of its own -- the same shape `disc_cap` used
when it lost its standalone waiver in discr-st8. Before that: the far wall's
tile grid and the far bank's 16 cells, both `waived:discr-ovl.3`, moved up to
the Compared table above as `tiles_far[n].tile_type`/`tiles_far[n].hp`; and
`players[n].disc_cap`'s standalone waiver row is gone (discr-st8, Part
12/round), folded into the compared `players[0].disc_cap` and the general
`players[1].*` waiver, the same way `discs_out` never had a row of its own.
Before that, the count had been unchanged from before Part 10, which was
already close to the honest number: five waivers were resolved and five new
ones filed from what the answers exposed -- a second tile grid, the bonus
placer, the hook installer, the owner polarity, and what retires a disc -- and
one row moved from `excluded:rendering` to `waived:discr-75o`, because
`$6cda`/`$6ce2` turned out to be the state machine's clock rather than a
rendering detail. What changed is not how many unknowns there are but where
they sit: six of them used to stand between the disc model and the fixture,
and now one does.
`reports/part10-report.md` has the before-and-after gate numbers. With the 25
compared rows above, that is the whole table, and
`tracecheck`'s header prints these three numbers so a drift between tool and
file is visible on every run.

Why each waiver or exclusion:

* **`discs[n].hook` is COMPARED as of Part 10f** -- `disc-core` installs it
  itself, from `$c826`'s anticipation cascade, and both fixtures stay clean with
  the row compared rather than fed. That is 30 installs and every clear,
  reproduced frame for frame.
* **`discs[n].active` is COMPARED as of Part 10g, and discr-0fm is CLOSED.**
  `disc+$10` is a byte with three regimes, and all four of its writers are now
  known: `$a9b8` claims a slot, `$caae`/`$cb1e` retire it when player 2 catches
  the disc, `$a570` retires it when the round ends, and `$012588` counts a
  retired slot down from the render pass. The "dwell" was a caught disc.
* **`discs[n].aim` is a MIRRORED ST FIELD**, not a
  model. Part 10 disassembled `$a4ea`: `disc+$10` is a byte whose bit 7 says
  whether the ST simulates the record (`$a4f0 beq` free, `$a534 bpl` frozen),
  `disc+$11` is the owner the wall handlers flip, and `disc+$12` is a longword
  hook holding one of three steering routines. The old note here -- "the ST
  encoding of an unused slot is not known", "there is no possession" -- was
  wrong on both counts and is retracted.
  They stay waived because each is written by code **outside** the disc loop:
  the owner byte's
  polarity (**discr-ovl.2, Part 12**) is now settled -- raw `0` is PLAYER 2's
  disc and raw `0xFF` is PLAYER 1's. `$a9aa`/`$a9bc` (the serve routine, called
  only from player 2's own control routine `$abb2`) bump `$6d8a` and clear the
  owner byte together, so a freshly served disc is always owner-0 and charged
  to player 2's own throw cap; `tests/fixtures/handover.ndjson` (frames 259 and
  339) shows the wall handlers moving `players[0]`/`players[1]`'s
  `discs_out`/`disc_cap` in both directions in lockstep with the flip, matching
  `$6d8a--`/`$6d8c--`/`$6d0c++`/`$6d0a++` (far wall) and its mirror (near wall)
  read live at `$a5d0`-`$a63c`. Full chain in `reports/part12-owner.md`. The row
  stays **waived, not compared**: the field is fed every tick because
  `disc-core` still has no WRITER for the owner byte itself (the four
  possession counters it steers gained their writers in discr-st8, Part
  12/round -- see `players[0].discs_out`/`disc_cap` above), not because the
  polarity is unknown any more. **discr-ovl.8 (Part 12) is CLOSED**: the feed
  mapping in `main.rs`'s `seed()` and every internal `disc.aim ==
  PlayerId::One`/`Two` check in `disc.rs`/`player.rs` were flipped together in
  one commit, so `PlayerId::One` now names real player 1 consistently
  everywhere `disc.aim` is read, including here -- not the
  self-consistent-but-backwards internal convention `main.rs`'s comment used
  to cite as the reason this arm alone couldn't be flipped. All nine
  tracecheck gates held unchanged; see `reports/part12-farbank.md`.
  `tracecheck` feeds `active` in every tick, the way it feeds
  `$6c58`, and says so in its header. **discr-ovl.1's player-2 half is
  closed**; its player-1 half, the racket path at `$11030`-`$110a8`, is
  untested because neither player ever swings in either fixture.
* Opponent AI (**discr-b6x**): **the input channel is no longer waived.**
  `$10eac` selects one-player mode on `$6da0`, `$d2cc` writes a synthetic
  joystick byte to `$6da1`, and `$abb2` consumes it exactly where a human's
  `$6c59` would go -- so `tracecheck` feeds `$6da1` to player 2 the way it feeds
  `$6c58` to player 1, and player 2's rows now *do* match: 21 ticks of the
  golden fixture with nothing waived and nothing resynced, where before Part 10c
  they could not match a single frame.
  What stays waived is two things. The **policy** -- the 20-entry rule table at
  `$efa8`, its 11 test and 7 action routines, and the sensor pass `$cea6` -- and
  the **remaining states of player 2's own table at `$c6ec`**. As of Part 10j
  player 2's rows reproduce **the whole of `tests/fixtures/golden.ndjson` with
  nothing waived at all** -- so the waiver now records that the *policy* is
  unmodelled and that some of its states are, not that the rows cannot match.
  On the idle fixture the same run reaches 161 of 214 and stops at the running
  smash (`$aef0`).
  Note the timing, because a replay gets it wrong by default: `$6da1` is written
  and consumed inside one VBL (`$10ec6` then `$10ece`), so the byte a tick uses
  is the one visible at the *next* sampling point, not the current one.
* Round, scoring and win (**discr-st8**, Part 12/round, narrowed): the four
  possession counters have their own writer now (`players[0].discs_out`/
  `disc_cap` above) and came OFF this waiver. What is left is round init,
  the round-over threshold and win/loss, decoded from the static image only
  (`$aa50`'s disc-array reset; `$6c83`, a global per-round death tally;
  `$6ca0`, the mode byte gating its threshold at 1 death for training or 2
  otherwise; and `player+$72`, a previously undocumented field the round
  loop compares between the two players to mark the loser DOWN and the
  winner's own `round_over`). None of it is live-verified -- no fixture on
  hand crosses a round transition, the FDC boundary -- and none of it is
  implemented: `GameState::default()` is still all zeroes, deliberately not
  `$aa50`'s round-init state. Full chain: `reports/part12-round.md`.
* Player state semantics (**discr-75o**, **discr-rf9**): **narrowed twice.**
  Seven states are modelled -- 0 (the idle path inlined in `$f104`, not a table
  entry: `$10e2c`'s entry 0 is null), 1 and 2 (the walks), 20 (the turn
  transient), 11 and 12 (knocked down and up) and 23 (out of energy, terminal)
  -- which is every state player 1 reaches anywhere in either fixture. The mechanism behind all 32 is also known: each
  handler ends in the animation tail at `$f1c4`, which counts `$6ce2` down and
  advances the six-byte sequence cursor `$6cda`, and **running off the end of a
  sequence is what changes state**. `discr-rxx.1` (Part 12) decoded the cell
  format and this crate now carries the frame-block data (`x_delta`/`hit_box`)
  for the 20 sequences player 1's own seven states and player 2's already-
  modelled throw/smash/intercept/reach/struck states reach -- see the
  `players[0].anim_cursor`/`x_delta`/`hit_box` note above. What stays waived
  is the other 28 handlers' behaviour, and player 2's own idle/walk sequences
  specifically (not fully catalogued -- discr-b6x, not this bead). State 17
  is partly explained -- `$c0c4 move.b #$11,$6d2e` is the serve setting it.
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
* **The far wall's grid is COMPARED as of Part 12, discr-ovl.3.** `$9f5e` is
  `$a24c` with `$7596` substituted for `$7616` (and `$6d1c` for `$6d9a`), so
  the second 16-cell bank is the same code, not a second mechanism: `disc-core`
  already carried `tiles_far`, the oracle already emitted the column
  (`banks`, Part 10e), and this bead is what connects the two --
  `tiles_far[n].tile_type`/`tiles_far[n].hp` are now compared rows, and
  `scripts/oracle_diff.py`'s labeller covers the range instead of reporting it
  unlabelled. `tests/fixtures/farbank.ndjson` confirms the seeded bank against
  a live trace. What is **not** exercised by any committed fixture is the
  damage subtraction itself (`$a5d6`'s arm) -- every reachable far-wall
  crossing is a disc's first arrival (the transfer arm), the same
  modelled-but-untested shape as discr-ovl.1's player-1 racket path.
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
