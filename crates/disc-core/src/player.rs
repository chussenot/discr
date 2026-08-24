//! Player movement and state dispatch. Owned by bead `discr-40e` (I2).
//!
//! ST `$f5d0` reads `player+$0e` (`$6cae`) and jumps through the 32-entry
//! table at `$10e2c`. Only two entries are implemented here, because only two
//! have known *behaviour*: 1 = walk left (`$f5e2`) and 2 = walk right
//! (`$f7f6`). Every other entry is an opaque pass-through -- see
//! `bd discr-75o` (handler addresses known, behaviour not) and `bd discr-rf9`
//! (states 16 and 17, never observed in Hatari at all).
//!
//! ```text
//! $f5d0  player state dispatch (jump table at $10e2c)
//! $f5e2  state 1 -- walk left    $f60e sub.w #$0018,d0   $f658 subq.w #3,$6ca2
//! $f7f6  state 2 -- walk right   $f822 add.w #$0018,d0   $f86c addq.w #3,$6ca2
//! $f638  lea.l $7616,a1 / lsl.w #3,d0 / tst.w ($00,a1,d0.w)  -- walkability gate
//! $f65c  lea.l $7bfe,a1          -- column table, indexed by world X 8..152
//! $f836  addq.w #$08,d0          -- cell = column + 8
//! $f838  cmp.w #$000e,$6ca6      -- Y > 14 selects the far row
//! $f842  addq.w #$04,d0          -- ... which adds 4 to the cell
//! ```
//!
//! Fire is not read here. `$f606` / `$f81a` `bclr #7,(a0)` *consumes* the fire
//! bit at the top of each walk handler, so [`Input::fire_edge`] arrives already
//! consumed; what the walk handlers then do with it is not in the notes.
//!
//! # The state machine (Part 10)
//!
//! `$f104` is player 1's control routine and its first instruction is the whole
//! architecture: `tst.b $6cae; bne $f5d0`. **State 0 is not a table entry** --
//! entry 0 of `$10e2c` is a null pointer -- it is the code that follows, the
//! idle path. Every other state dispatches.
//!
//! Every handler ends in the same animation tail, `$f1c4`:
//!
//! ```text
//! $f1c4  a1 = $6cda            ; the animation sequence cursor
//! $f1c8  a1 = (a1)             ; -> this cell's frame block
//! $f1ca  copy 20 bytes of it into $6ce4, $6cd6, $6cb6, $6cba, $6cbc, ...
//! $f1ee  subq.w #1,$6ce2       ; frames left on this cell
//! $f1f2  bne $f1fc
//! $f1f4  addq.l #6,a1          ; expired -> next cell, 6 bytes on
//! $f1f6  $6ce2 = (4,a1)        ; reload its hold count
//! $f1fc  tst.l (a1) ; bne out  ; a zero longword terminates the sequence
//! $f202  ... the sequence ENDED: load the next one and change state
//! ```
//!
//! So an animation sequence is a list of 6-byte cells -- a 4-byte pointer and a
//! 2-byte hold count -- ending in a zero longword, and **running off the end is
//! what changes state**. `$f1c4`'s own ending goes to state 0; state 20's copy
//! of the tail (`$1099a`) goes to `$6caa`, the pending state.
//!
//! That makes the observed `0 -> 20 -> 20 -> 20 -> 1` exactly reproducible.
//! Pressing Left from idle (`$f260`) writes `$6caa = 1`, loads the sequence at
//! `$2f7e` -- **one cell, hold 4, then the terminator** -- sets `$6cae = $14`
//! and falls into the tail in the same tick, so the count is already 3 when the
//! frame is sampled. Three more handler runs take it to 0, the cursor hits the
//! terminator, and `$6caa` becomes the state.
//!
//! `$6cba` is a per-frame X delta lifted out of the animation frame block and
//! applied by the idle path at `$f118` (`$6ca2 += $6cba`), so some movement is
//! animation-driven. The walk states do their own `subq.w #3` instead. Only the
//! turn transient's sequence is modelled here; the frame blocks live in data
//! this crate does not carry. `// UNKNOWN: see bd discr-75o`.

use crate::{
    DirBits, Event, FACING_LEFT, FACING_RIGHT, FAR_ROW_Y, GRID_CELL_BASE, GRID_CELL_FAR_ROW, Input,
    Player, TILE_CELLS, Tile, WALK_STEP, WALK_X_MAX, WALK_X_MIN,
};

/// The transient state a player passes through when starting or stopping a
/// walk. ST `$10e2c` entry 20 = `$1094a`.
pub const STATE_TURN: u8 = 0x14;

/// ST state 1: walk left (`$f5e2`). Also the value `$6ca9` takes while it runs.
pub const STATE_WALK_LEFT: u8 = 1;

/// ST state 2: walk right (`$f7f6`).
pub const STATE_WALK_RIGHT: u8 = 2;

/// ST state 0: idle, handled inline in `$f104` rather than through the table.
pub const STATE_IDLE: u8 = 0;

/// Frames the turn transient holds. ST `$2f7e`, the sequence loaded at `$f27a`,
/// `$f2ce` and `$f7c4`: **one cell with a hold of 4, then a zero terminator**.
///
/// The tick that enters state 20 also runs the animation tail, so the count is
/// 3 at the first sample and the state changes on the fourth handler run --
/// which is the three consecutive frames of state 20 the fixture shows, at
/// f11-f13 and again at f29-f31.
pub const TURN_ANIM_HOLD: u16 = 4;

/// How far ahead of the player the destination cell is probed.
///
/// ST `$f60e`: `sub.w #$0018,d0`; ST `$f822`: `add.w #$0018,d0`.
const PROBE_AHEAD: i16 = 24;

/// Width of one floor column in world X units.
///
/// ST `$7bfe`: 152 bytes indexed by world X, 4 columns of 40 X-units. Shared
/// with [`crate::disc::disc_cell`], which reads the same table 4 bytes in.
use crate::COLUMN_WIDTH;

/// First world X past the last column, i.e. outside the arena.
///
/// ST `$7bfe`: X 0-39 -> 1, 40-79 -> 2, 80-119 -> 3, 120-159 -> 4, 160+ -> 0.
const ARENA_X_END: i16 = 4 * COLUMN_WIDTH;

/// The `$7bfe` column of a world X: 1..4 inside the arena, 0 outside it.
fn column(world_x: i16) -> u16 {
    if (0..ARENA_X_END).contains(&world_x) {
        // Guarded to 0..160 just above, so the division is 0..3.
        1 + (world_x / COLUMN_WIDTH) as u16
    } else {
        0
    }
}

/// The floor grid cell a player at `(world_x, world_y)` stands on.
///
/// ST `$f836`/`$f838`/`$f842`: `column(X) + 8`, plus 4 when `$6ca6 > 14`. The
/// result is at most `4 + 8 + 4 = 16`, so it always indexes the 17-cell grid.
fn grid_cell(world_x: i16, world_y: i16) -> u16 {
    GRID_CELL_BASE
        + column(world_x)
        + if world_y > FAR_ROW_Y {
            GRID_CELL_FAR_ROW
        } else {
            0
        }
}

/// One frame of a walk handler: face, probe, step, recompute the cell.
///
/// * `facing` is written unconditionally -- ST `$6ca9` is set at the handler
///   entry (`$f5e2` / `$f7f6`), before anything can block the step.
/// * The step is gated on a whole-byte compare of the decoded joystick against
///   the single direction bit (ST `$f650  cmp.b #$04,(a0)` and `$f864  cmp.b
///   #$08,(a0)`, each `bne`-ing past the write). It is a compare, not a `btst`,
///   so a diagonal does not walk. Bit 7 was already cleared by `bclr` above it.
/// * The destination is probed [`PROBE_AHEAD`] units away and its cell's type
///   word is `tst.w`'d as the walkability gate (`$f63e` / `$f852`); type 0 is a
///   destroyed cell, and the player does not step onto it.
/// * The probe index is clamped into the column table's domain rather than
///   blocking the step. ST `$f612  cmp.w #$0008,d0 / blt` and `$f826  cmp.w
///   #$0098,d0 / bgt` branch to the same "no cell" path as an unwalkable
///   destination, but treating that as a stop would floor the walk 24 units
///   short of each wall, and Part 4 measured the Left run reaching exactly 8
///   and the Right run exactly 152.
/// * X is clamped to 8..152 (`WALK_X_MIN`..`WALK_X_MAX`) rather than only
///   stepping by 3. Same evidence: from idle X = 117 the Right run ended on
///   152, which is not 117 + 3n (150 + 3 = 153, clamped), and the Left run
///   ended on 8, which is not 117 - 3n (9 - 3 = 6, clamped).
fn walk(
    player: &mut Player,
    input: Input,
    tiles: &[Tile; TILE_CELLS],
    facing: u8,
    held: DirBits,
    step_x: i16,
) {
    // ST $f5e2 / $f7f6: the handler sets $6ca9 on entry.
    player.facing = facing;

    // ST $f654 / $f868: `cmpi.b #$04,(a0); bne $f7b8` -- a WHOLE-BYTE compare,
    // so anything other than exactly this direction leaves the walk. The exit
    // clears the pending state ($f7b8 / $f9ce) and enters the turn transient
    // ($f7d2 / $f9e8 write #$14), which then lands on state 0.
    if input.dir != held {
        player.pending_state = STATE_IDLE;
        enter_turn(player);
        player.grid_cell = grid_cell(player.world_x, player.world_y);
        return;
    }

    {
        let probe = (player.world_x + step_x.signum() * PROBE_AHEAD).clamp(WALK_X_MIN, WALK_X_MAX);
        // ST $f63e / $f852: tst.w tile+$00 -- 0 = destroyed = unwalkable.
        if tiles[grid_cell(probe, player.world_y) as usize].walkable() {
            // ST $f658: subq.w #3,$6ca2; ST $f86c: addq.w #3,$6ca2.
            player.world_x = (player.world_x + step_x).clamp(WALK_X_MIN, WALK_X_MAX);
        }
    }

    // ST $f65c onward: the cell in $6cb0 is recomputed from the new X.
    player.grid_cell = grid_cell(player.world_x, player.world_y);
}

/// Enter the turn transient. ST `$f27a`-`$f288`, `$f2ce`-`$f2dc`, `$f7c4`-`$f7d2`
/// and `$f9e0`-`$f9e8`: load the `$2f7e` sequence, set `$6ce2` from its hold,
/// write `$6cae = $14`, and fall into the animation tail in the same tick --
/// which is the `- 1` here.
fn enter_turn(player: &mut Player) {
    player.state_index = STATE_TURN;
    player.anim_hold = TURN_ANIM_HOLD - 1;
}

/// State 0, the idle path inlined in `$f104` rather than reached through the
/// jump table -- entry 0 of `$10e2c` is a null pointer.
///
/// Only the two walk directions are modelled. Up (`$f222` -> state 5), down
/// (`$f240`), fire (`$f21e bmi` -> `$f306`) and the `$6cac`/`$6cad` paths that
/// reach state 26 all lead into handlers whose behaviour is unrecovered, so
/// this leaves `state_index` alone for them rather than entering a state it
/// cannot then run. `// UNKNOWN: see bd discr-75o`.
fn idle(player: &mut Player, input: Input) {
    // $f1ba: tst.b (a0); beq -> clr.b $6ca9 at $f1c0.
    if input.dir == DirBits(0) && !input.fire_edge {
        player.facing = 0;
        return;
    }

    let pending = if input.dir == DirBits::LEFT {
        STATE_WALK_LEFT
    } else if input.dir == DirBits::RIGHT {
        STATE_WALK_RIGHT
    } else {
        // Up, down, fire and every combination: unmodelled.
        return;
    };

    // $f266 / $f2ba: `tst.b $6ca9; bne` skips the turn entirely and enters the
    // walk state directly. So the transient plays only from a standing start,
    // where the idle path has just cleared $6ca9 -- which is why the fixture
    // shows it on f11 (after ten idle frames) and again on f29.
    player.pending_state = pending;
    if player.facing == 0 {
        enter_turn(player);
    } else {
        player.state_index = pending;
    }
}

/// State 20, the turn transient. ST `$1094a`.
///
/// Stamps `$6ca9`, runs the animation tail, and on the frame the `$2f7e`
/// sequence runs out writes `$6caa` into `$6cae` (`$1099a`).
fn turn(player: &mut Player) {
    player.facing = STATE_TURN;
    match player.anim_hold.checked_sub(1) {
        Some(0) | None => player.state_index = player.pending_state,
        Some(left) => player.anim_hold = left,
    }
}

/// Advance one player by one frame.
///
/// `tiles` is read-only here: the movement code only `tst.w`s `tile+$00` as a
/// walkability gate. Only the disc loop writes tiles.
///
/// No player event exists, so `events` is never pushed to.
///
/// Part 10 made `state_index` an **output** for the four states the fixtures
/// exercise on player 1 -- idle, the turn transient, and the two walks. The
/// other 28 entries are still opaque pass-throughs: their handler addresses are
/// known and their behaviour is not, so entering one would mean running a state
/// this crate cannot simulate. `// UNKNOWN: see bd discr-75o`.
pub fn step(
    player: &mut Player,
    input: Input,
    tiles: &[Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
    // ST $f108: `tst.b $6cae; bne $f5d0` -- state 0 is the inline idle path and
    // everything else dispatches through the 32-entry table at $10e2c.
    match player.state_index {
        STATE_IDLE => idle(player, input),
        STATE_TURN => turn(player),
        1 => walk(player, input, tiles, FACING_LEFT, DirBits::LEFT, -WALK_STEP),
        2 => walk(
            player,
            input,
            tiles,
            FACING_RIGHT,
            DirBits::RIGHT,
            WALK_STEP,
        ),
        // Tier-1 states: the handler address is known, the behaviour is not.
        // Opaque pass-through -- moving a field we cannot justify would only
        // make a trace comparison diverge on the fields these do not touch.
        5 => {}       // $fb6e (tests fire, btst #7,(a0) at $fb74). UNKNOWN: see bd discr-75o
        11 => {}      // $10554. UNKNOWN: see bd discr-75o
        14 => {}      // $106b2, entered under Right+Fire. UNKNOWN: see bd discr-75o
        19 => {}      // $108f4. UNKNOWN: see bd discr-75o
        21 => {}      // $109aa. UNKNOWN: see bd discr-75o
        23 => {}      // $10a72. UNKNOWN: see bd discr-75o
        24 => {}      // $10ac4. UNKNOWN: see bd discr-75o
        27 => {}      // $10c8a. UNKNOWN: see bd discr-75o
        31 => {}      // $10dda. UNKNOWN: see bd discr-75o
        16 | 17 => {} // never seen in Hatari, oracle only. UNKNOWN: see bd discr-rf9
        // Every other index is unattested.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 17 cells walkable, as the floor is before any disc lands.
    const FLOOR: [Tile; TILE_CELLS] = [Tile {
        tile_type: 1,
        hp: 4,
    }; TILE_CELLS];

    fn walking(state_index: u8, world_x: i16) -> Player {
        Player {
            world_x,
            world_y: 18,
            state_index,
            ..Player::default()
        }
    }

    fn press(dir: DirBits) -> Input {
        Input {
            dir,
            fire_edge: false,
        }
    }

    fn run(player: &mut Player, input: Input, tiles: &[Tile; TILE_CELLS]) {
        step(player, input, tiles, &mut Vec::new());
    }

    /// The four `$6cb0` observations of the Part-4 `player_x_hunt` and
    /// `player_y_hunt` runs -- idle, Right, Left and Down -- recorded
    /// independently of each other and of the formula.
    #[test]
    fn grid_cell_matches_the_four_measured_samples() {
        assert_eq!(grid_cell(117, 18), 15); // idle
        assert_eq!(grid_cell(152, 18), 16); // Right
        assert_eq!(grid_cell(8, 18), 13); // Left
        assert_eq!(grid_cell(117, 2), 11); // Down
    }

    #[test]
    fn column_table_boundaries_match_7bfe() {
        // $7bfe: X 0-39 -> 1, 40-79 -> 2, 80-119 -> 3, 120-159 -> 4, 160+ -> 0.
        assert_eq!((column(0), column(39)), (1, 1));
        assert_eq!((column(40), column(79)), (2, 2));
        assert_eq!((column(80), column(119)), (3, 3));
        assert_eq!((column(120), column(159)), (4, 4));
        assert_eq!((column(160), column(-1)), (0, 0));
    }

    #[test]
    fn far_row_adds_four_at_y_fifteen_not_fourteen() {
        // $f838: cmp.w #$000e,$6ca6 -- strictly greater than 14.
        assert_eq!(grid_cell(117, FAR_ROW_Y), 11);
        assert_eq!(grid_cell(117, FAR_ROW_Y + 1), 15);
    }

    #[test]
    fn walking_steps_three_units_and_sets_facing() {
        let mut p = walking(1, 117);
        run(&mut p, press(DirBits::LEFT), &FLOOR);
        assert_eq!((p.world_x, p.facing, p.grid_cell), (114, FACING_LEFT, 15));

        let mut p = walking(2, 117);
        run(&mut p, press(DirBits::RIGHT), &FLOOR);
        assert_eq!((p.world_x, p.facing, p.grid_cell), (120, FACING_RIGHT, 16));
    }

    #[test]
    fn walking_clamps_to_the_measured_extremes() {
        // Right from 150 lands on 152, not 153; Left from 9 lands on 8, not 6.
        let mut p = walking(2, 150);
        run(&mut p, press(DirBits::RIGHT), &FLOOR);
        assert_eq!(p.world_x, WALK_X_MAX);

        let mut p = walking(1, 9);
        run(&mut p, press(DirBits::LEFT), &FLOOR);
        assert_eq!(p.world_x, WALK_X_MIN);
    }

    #[test]
    fn walking_into_a_wall_does_not_move() {
        let mut p = walking(2, WALK_X_MAX);
        run(&mut p, press(DirBits::RIGHT), &FLOOR);
        assert_eq!(p.world_x, WALK_X_MAX);

        let mut p = walking(1, WALK_X_MIN);
        run(&mut p, press(DirBits::LEFT), &FLOOR);
        assert_eq!(p.world_x, WALK_X_MIN);
    }

    #[test]
    fn a_destroyed_destination_cell_blocks_the_step() {
        // From X = 117 walking right, the probe is 141 -> column 4 -> cell 16.
        let mut tiles = FLOOR;
        tiles[16] = Tile {
            tile_type: 0,
            hp: 0,
        };
        let mut p = walking(2, 117);
        run(&mut p, press(DirBits::RIGHT), &tiles);
        assert_eq!(p.world_x, 117);
        // Blocked or not, facing and the cell are still written.
        assert_eq!((p.facing, p.grid_cell), (FACING_RIGHT, 15));
    }

    #[test]
    fn only_the_bare_direction_bit_walks() {
        // $f650/$f864 are cmp.b, not btst: a diagonal is not equal to $04/$08.
        for dir in [DirBits::NONE, DirBits::RIGHT, DirBits::LEFT.or(DirBits::UP)] {
            let mut p = walking(1, 117);
            run(&mut p, press(dir), &FLOOR);
            assert_eq!(p.world_x, 117, "dir {dir:?} must not walk left");
        }
    }

    #[test]
    fn fire_does_not_change_a_walk() {
        // bclr #7 at $f606/$f81a consumes the bit before the cmp.b.
        let mut p = walking(1, 117);
        step(
            &mut p,
            Input {
                dir: DirBits::LEFT,
                fire_edge: true,
            },
            &FLOOR,
            &mut Vec::new(),
        );
        assert_eq!(p.world_x, 114);
    }

    #[test]
    fn opaque_states_move_nothing() {
        // 0 and 20 left the list in Part 10: they are modelled now.
        for state in [5, 11, 14, 16, 17, 19, 21, 23, 24, 27, 31] {
            let before = walking(state, 117);
            let mut p = before;
            run(&mut p, press(DirBits::LEFT), &FLOOR);
            assert_eq!(p, before, "state {state} must be a pass-through");
        }
    }

    /// golden.ndjson f10-f14, the whole start-walking sequence. Idle with Left
    /// pressed enters the turn transient ($f274/$f288), which holds for exactly
    /// three sampled frames off the `$2f7e` hold of 4, and then becomes the
    /// pending walk state ($1099a).
    #[test]
    fn pressing_left_from_idle_turns_for_three_frames_then_walks() {
        let mut p = walking(STATE_IDLE, 117);
        let left = press(DirBits::LEFT);

        // f10 -> f11: enter the transient. The entering tick runs the animation
        // tail too, so the count is already 3.
        run(&mut p, left, &FLOOR);
        assert_eq!(
            (p.state_index, p.pending_state, p.anim_hold),
            (STATE_TURN, STATE_WALK_LEFT, 3)
        );
        assert_eq!(p.world_x, 117, "the transient does not move");

        // f11 -> f13: three more handler runs, the last of which transitions.
        for expected in [2, 1] {
            run(&mut p, left, &FLOOR);
            assert_eq!((p.state_index, p.anim_hold), (STATE_TURN, expected));
            assert_eq!(p.facing, STATE_TURN, "$1094a stamps $6ca9 on entry");
        }
        run(&mut p, left, &FLOOR);
        assert_eq!(p.state_index, STATE_WALK_LEFT);
        assert_eq!(p.world_x, 117, "and still has not moved");

        // f14 -> f15: now it walks, three units a frame.
        run(&mut p, left, &FLOOR);
        assert_eq!(
            (p.state_index, p.world_x, p.facing),
            (STATE_WALK_LEFT, 114, 1)
        );
    }

    /// golden.ndjson f28-f32: releasing the stick leaves the walk through the
    /// same transient, with a pending state of 0 ($f7b8 clr.b $6caa).
    #[test]
    fn releasing_the_stick_turns_back_to_idle() {
        let mut p = walking(STATE_WALK_LEFT, 75);
        let none = Input::default();

        run(&mut p, none, &FLOOR);
        assert_eq!(
            (p.state_index, p.pending_state, p.anim_hold),
            (STATE_TURN, STATE_IDLE, 3)
        );
        for _ in 0..2 {
            run(&mut p, none, &FLOOR);
            assert_eq!(p.state_index, STATE_TURN);
        }
        run(&mut p, none, &FLOOR);
        assert_eq!(p.state_index, STATE_IDLE);
        assert_eq!(p.world_x, 75, "leaving a walk does not move");
    }

    #[test]
    fn no_walk_produces_an_event() {
        let mut events = Vec::new();
        let mut p = walking(2, 117);
        step(&mut p, press(DirBits::RIGHT), &FLOOR, &mut events);
        assert!(events.is_empty());
    }
}
