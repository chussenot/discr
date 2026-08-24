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
    DirBits, DiscSlot, Event, FACING_LEFT, FACING_RIGHT, FAR_ROW_Y, GRID_CELL_BASE,
    GRID_CELL_FAR_ROW, Input, Player, PlayerId, SteerHook, TILE_CELLS, TILE_TYPE_DESTROYED, Tile,
    WALK_STEP, WALK_X_MAX, WALK_X_MIN,
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

/// How low a knocked-down player sinks. ST `$1056a`: `cmpi.w #$02,$6ca6; ble`.
pub const STRUCK_Y_FLOOR: i16 = 2;

/// ST state 11: knocked down and sinking (`$10554`).
pub const STATE_STRUCK_DOWN: u8 = 0x0b;

/// ST state 12: knocked down and rising (`$1057c`).
pub const STATE_STRUCK_UP: u8 = 0x0c;

/// ST state 23: out of energy (`$10a72`). Terminal -- nothing leaves it.
pub const STATE_DEAD: u8 = 0x17;

/// The value [`Player::anim_shown`] takes on entering a sequence.
///
/// ST: `$6ce4` still holds the *previous* sequence's frame block, so the
/// `cmp.l $6ce4,D0` a handler opens with can never match on the entry frame.
/// 255 is out of range for any sequence this crate carries.
pub const NO_CELL: u8 = 0xff;

/// One animation sequence: the hold counts of its cells, in order.
///
/// ST: a list of six-byte cells -- a four-byte frame-block pointer and a
/// two-byte hold -- ending in a zero longword. The frame blocks are sprite and
/// hit-box data this crate does not carry, so only the holds are here, which is
/// all the state machine's timing depends on.
pub type AnimSeq = &'static [u16];

/// ST `$2f7e`: the turn transient. One cell, hold 4, then the terminator.
///
/// Loaded at `$f27a`, `$f2ce`, `$f7c4` and `$f9e0`.
pub const ANIM_TURN: AnimSeq = &[4];

/// ST `$2d50`: knocked down. Two cells, holds 4 and 4. Loaded at `$11226`.
pub const ANIM_STRUCK_DOWN: AnimSeq = &[4, 4];

/// ST `$2d60`: knocked upward. Two cells, holds 4 and 4. Loaded at `$11210`.
pub const ANIM_STRUCK_UP: AnimSeq = &[4, 4];

/// ST `$2d70`: the death sequence. Sixteen cells, hold 4 each. Loaded at
/// `$f1a0`, and state 23 never leaves it -- see [`dead`].
pub const ANIM_DEAD: AnimSeq = &[4; 16];

/// ST `$466a`: player 2 reaching for a disc, loaded at `$cbb6`. Its holds are
/// not transcribed -- state 27's handler is not modelled, so nothing runs it.
pub const ANIM_REACH: AnimSeq = &[4];

/// ST `$4612`: player 2 stepping across, loaded at `$cc26`. Same caveat.
pub const ANIM_INTERCEPT: AnimSeq = &[4];

/// What one run of the animation tail did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimStep {
    /// The current cell still has frames left. ST: `$f1f2 bne`.
    Holding,
    /// The cursor moved to the next cell. ST: `$f1f4 addq.l #6,a1`.
    Advanced,
    /// The cursor reached the zero longword. ST: `$f1fc tst.l (a1); beq`, which
    /// is where a state ends.
    Ended,
}

/// Load a sequence and enter its first cell. ST: the three-instruction preamble
/// every handler writes before setting `$6cae` --
/// `lea <seq>,a1; move.w ($04,a1),$6ce2; move.l a1,$6cda`.
pub fn enter_anim(player: &mut Player, seq: AnimSeq) {
    player.anim_cell = 0;
    player.anim_shown = NO_CELL;
    player.anim_hold = seq[0];
}

/// The animation tail every handler ends in. ST `$f1c4`, faithfully in order:
/// the frame block is copied *first* (which is what makes `$6ce4` the
/// previous frame's cell for the next handler run), then the hold is
/// decremented, then the cursor may advance.
pub fn anim_tick(player: &mut Player, seq: AnimSeq) -> AnimStep {
    // $f1ca: the copy, before anything else. Only the cell identity matters here.
    player.anim_shown = player.anim_cell;

    // $f1ee: subq.w #1,$6ce2.
    player.anim_hold = player.anim_hold.saturating_sub(1);
    if player.anim_hold > 0 {
        return AnimStep::Holding;
    }
    // $f1f4/$f1f6: six bytes on, and reload the hold from the new cell.
    player.anim_cell = player.anim_cell.saturating_add(1);
    match seq.get(player.anim_cell as usize) {
        Some(&hold) => {
            player.anim_hold = hold;
            AnimStep::Advanced
        }
        // $f1fc: the zero longword.
        None => AnimStep::Ended,
    }
}

/// Whether the sequence advanced since the last handler run.
///
/// ST `$10560`-`$10566`: `move.l (A1),D0; cmp.l $6ce4,D0; beq` -- the handler
/// compares the cell it is about to show against the one `$f1ca` copied last
/// frame. It is what paces a knocked-down player's vertical movement.
#[must_use]
pub const fn anim_advanced(player: &Player) -> bool {
    player.anim_cell != player.anim_shown
}

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
    enter_anim(player, ANIM_TURN);
    // $f292 / $f7d8: the entering tick falls straight into the tail, so the
    // count is already down one before the frame is sampled.
    anim_tick(player, ANIM_TURN);
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
    // $f11c: `tst.b $6cac; bne $f170` -- out of energy, so the idle path plays
    // the death sequence instead and never comes back. $f1b4 also sets $6d2d,
    // player 2's +$0d, which this crate does not model.
    if player.down {
        player.state_index = STATE_DEAD;
        enter_anim(player, ANIM_DEAD);
        // $f1b8 bra $f1c4: the tail runs on the entering tick too.
        anim_tick(player, ANIM_DEAD);
        return;
    }

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
    // $1099a: the sequence running out is what writes $6caa into $6cae.
    if anim_tick(player, ANIM_TURN) == AnimStep::Ended {
        player.state_index = player.pending_state;
    }
}

/// State 11, knocked down. ST `$10554`.
///
/// ```text
/// $10554  move.b #$0b,$6ca9
/// $10560  d0 = the current cell's frame block
/// $10562  cmp.l $6ce4,d0 ; beq $f1c4    ; unchanged -> just run the tail
/// $1056a  cmpi.w #$02,$6ca6 ; ble       ; floor at 2
/// $10574  subq.w #1,$6ca6               ; else sink one row
/// ```
///
/// So the player sinks **one row per animation cell**, not per frame, which is
/// why `golden.ndjson` reads 18, 17, 17, 17, 17, 16, 16, 16 across frames
/// 63-70: the `$2d50` sequence is two cells of four frames each.
fn struck_down(player: &mut Player) {
    player.facing = STATE_STRUCK_DOWN;
    if anim_advanced(player) && player.world_y > STRUCK_Y_FLOOR {
        player.world_y -= 1;
    }
    // $10578 bra $f1c4: the plain tail, so the sequence ending lands on state 0.
    if anim_tick(player, ANIM_STRUCK_DOWN) == AnimStep::Ended {
        player.state_index = STATE_IDLE;
    }
    player.grid_cell = grid_cell(player.world_x, player.world_y);
}

/// State 23, out of energy. ST `$10a72`.
///
/// A **variant of the animation tail**, and the difference is the point: it
/// tests for the terminator *before* copying, and on reaching it does not
/// change state at all. It bumps `$6cab` -- the control-lockout counter -- by 3
/// and `$6c83` by 1, and returns. So state 23 is terminal, which is what makes
/// it the end of a round rather than another transient.
/// `// UNKNOWN ($6c83, and what ever leaves this state): see bd discr-st8`.
fn dead(player: &mut Player) {
    player.facing = STATE_DEAD;
    anim_tick(player, ANIM_DEAD);
}

/// State 18, stepping across to intercept. ST `$c196`.
///
/// ```text
/// $c196  move.b #$12,$6d29                    ; not modelled
/// $c19c  if $6d5a is $4624 or $4634 -> commit, else just run the animation
/// $c1b4  btst #$7,(a0) ; beq out              ; fire must still be HELD
/// $c1bc  btst #$1,(a0) ; bne out              ; and down must not be
/// $c1c4  if $6d8a == $6d8c -> out             ; already at the disc cap
/// $c1d0  subq.w #6,$6d22                      ; six units left, in one step
/// $c1d4  animation $45f0
/// $c1e2  move.b #$f,$6d2e                     ; and into state 15, the throw
/// ```
///
/// `golden.ndjson` frames 39 -> 40 are exactly this: the cursor reaches `$4624`
/// and player 2 goes from `x` 63 to 57, in state 15.
fn intercept(player: &mut Player, input: Input) {
    if player.anim_cursor != INTERCEPT_RELEASE_A && player.anim_cursor != INTERCEPT_RELEASE_B {
        return;
    }
    if !input.fire_held || input.dir.has(DirBits::DOWN) || player.discs_out == player.disc_cap {
        return;
    }
    player.world_x -= INTERCEPT_STEP;
    player.grid_cell = grid_cell(player.world_x, player.world_y);
    player.state_index = STATE_THROW_STANDING;
}

/// State 12, knocked upward. ST `$1057c`, the mirror of [`struck_down`].
///
/// Both arms of its `cmpi.w #$19,$6ca6` add one to `$6ca6`; the `>= 25` arm
/// (`$105a4`) then does more that is not decoded, so this models the add and
/// stops. `// UNKNOWN: see bd discr-75o`.
fn struck_up(player: &mut Player) {
    player.facing = STATE_STRUCK_UP;
    if anim_advanced(player) {
        player.world_y += 1;
    }
    if anim_tick(player, ANIM_STRUCK_UP) == AnimStep::Ended {
        player.state_index = STATE_IDLE;
    }
    player.grid_cell = grid_cell(player.world_x, player.world_y);
}

/// Player 1's hit test. ST `$10fd8`, called from the disc loop at `$a652`
/// **between the integration and the write-back**, which is why it takes the
/// three candidate coordinates and returns a possibly-modified `world_z`.
///
/// Only the strike -- the disc hitting the player's body -- is modelled. Two
/// other paths through the same routine are not:
///
/// * **states 7..10 are the racket** (`$11030`-`$11096`): the player is
///   swinging, the disc is caught inside a second, wider box built from
///   `$6cc6`/`$6cc8`, and `$110a6` adds `$6cc4` to its `vel_x`. That is the
///   path that installs the `$a71a` steering hook at `$113e2`, so decoding it
///   is what would let `disc-core` stop being fed `disc+$12`.
///   `// UNKNOWN: see bd discr-ovl.1`.
/// * **the three owner-specific states** `$12`, `$13` and `$1b` (`$11012`).
///
/// The strike, transcribed:
///
/// ```text
/// $10fd8  tst.b $6cac ; bne out          ; already down
/// $10fe4  did the disc CROSS $6ca6 this frame, in its direction of travel?
/// $11030  state 7..10 -> the racket path instead
/// $110fc  x candidate inside [px - 8 + b0, px - 8 + b0 + 8 + b1]?
/// $11118  y candidate inside [99 + b2, 99 + b2 + b3]?
/// $11178  energy -= the disc's +$16, clamped at 0; at 0, $6cac is set
/// $111ce  neg.w ($0a,a5) ; d2 += it ; clr.l ($12,a5)
/// $111da  and then a state, chosen by the state it interrupted
/// ```
///
/// The energy path has two bonus branches this crate does not model (`$1117c`
/// skips the subtraction entirely when the bonus code is 4 -- a shield -- and
/// `$11188` applies it twice when the code is 1). No trace carries a non-zero
/// bonus code. `// UNKNOWN: see bd discr-z8m`.
#[must_use]
pub fn hit_test(
    player: &mut Player,
    disc: &mut DiscSlot,
    x_cand: i16,
    y_cand: i16,
    z_before: i16,
    z_cand: i16,
) -> i16 {
    // $10fd8: a player already down is not hit again.
    if player.down {
        return z_cand;
    }

    // $10fe4-$11008: the disc must have crossed the player's depth this frame,
    // in the direction it is travelling. Two separate comparisons, not a range
    // test: where it WAS on the near side and where it WILL BE on the far one.
    let crossed = if disc.dir_kind >= 0 {
        z_before < player.world_y && z_cand >= player.world_y
    } else {
        z_before > player.world_y && z_cand <= player.world_y
    };
    if !crossed {
        return z_cand;
    }

    // $11030: the racket states take a different path entirely.
    if (7..=10).contains(&player.state_index) {
        return z_cand;
    }

    // $110fc-$1112c: the body box, whose four words come out of the current
    // animation cell (see Player::hit_box).
    let [b0, b1, b2, b3] = player.hit_box;
    let left = player.world_x - 8 + b0;
    let right = left + 8 + b1;
    if x_cand < left || x_cand > right {
        return z_cand;
    }
    let bottom = crate::disc::PLAYER_HEIGHT_REF + b2;
    if y_cand < bottom || y_cand > bottom + b3 {
        return z_cand;
    }

    // $11130: state $1a is a case of its own. // UNKNOWN: see bd discr-75o.
    if player.state_index == 0x1a {
        return z_cand;
    }

    // $1116e-$111c6: only one owner value takes damage.
    if disc.aim == PlayerId::One {
        player.energy -= disc.damage;
        if player.energy < 0 {
            player.energy = 0;
            // $111ca st $6cac.
            player.down = true;
        }
    }

    // $111ce-$111d6: the disc bounces, and the bounce is applied to the
    // candidate the disc loop is about to write back.
    disc.dir_kind = disc.dir_kind.wrapping_neg();
    let z = z_cand + disc.dir_kind;
    disc.hook = crate::SteerHook::None;

    match player.state_index {
        // $111da-$11208: interrupting a walk or a turn keeps the state and only
        // touches two fields this crate does not model ($6cce, $6cd2)...
        1 | 2 | 3 | 4 | 0x15 | 0x16 => {
            // ...except $11256, which forces an outgoing disc to dir_kind +1.
            if disc.dir_kind >= 0 {
                disc.dir_kind = 1;
            }
        }
        // $11210: still travelling away -> knocked upward.
        _ if disc.dir_kind < 0 => {
            player.state_index = STATE_STRUCK_UP;
            enter_anim(player, ANIM_STRUCK_UP);
        }
        // $11226: knocked down, and the disc leaves at exactly +1.
        _ => {
            player.state_index = STATE_STRUCK_DOWN;
            enter_anim(player, ANIM_STRUCK_DOWN);
            // $1123a: move.w #$0001,($000a,a5).
            disc.dir_kind = 1;
        }
    }
    z
}

/// ST state 17: a four-byte stub, `$1089a bra $f1c4` / `$c192 bra $ac40`. It is
/// the only entry in either 32-state table that does not stamp `player+$09`,
/// and the only one whose handler has no body at all.
pub const STATE_STUB: u8 = 0x11;

/// The two animation cursor values state 18 commits on. ST `$c19c` / `$c1a8`.
pub const INTERCEPT_RELEASE_A: u32 = 0x4624;
/// The other one.
pub const INTERCEPT_RELEASE_B: u32 = 0x4634;

/// How far the intercept steps, in one move. ST `$c1d0`: `subq.w #6,$6d22`.
pub const INTERCEPT_STEP: i16 = 6;

/// The standing throw state the intercept commits into. ST `$c1e2`.
pub const STATE_THROW_STANDING: u8 = 0x0f;

/// The catch window's half-width from `$6d22`, for state 18. ST `$ca9a`.
pub const CATCH_WIDTH_INTERCEPT: i16 = 0x1a;

/// State 27's is asymmetric: `$cb0a` subtracts `$10` and `$cb14` adds `$20`.
pub const CATCH_LOW_REACH: i16 = 0x10;
/// The other half of state 27's window. ST `$cb14`.
pub const CATCH_HIGH_REACH: i16 = 0x20;

/// Player 2's hit test, `$c826`. Called from the disc loop at `$a656`, right
/// after player 1's, and like it takes the candidate coordinates because it
/// runs before the write-back.
///
/// Structurally it is `$10fd8` with a tail. The head is the same crossing test
/// and the same owner check; what is different is that three of player 2's own
/// states get a **catch** window of their own before the body box is reached:
///
/// ```text
/// $c860  state $12 -> $ca96     ; the intercept's catch
/// $c86a  state $13 -> $cad0     ; (not modelled -- no fixture reaches it)
/// $c874  state $1b -> $cb06     ; the reach's catch
/// $c87e  states 7..10 -> the racket path, not modelled
/// $c934  otherwise the body box, the mirror of $110fc, not modelled
/// ```
///
/// A catch is two instructions: `addq.b #4,($10,a5)` retires the disc and
/// `subq.w #1,$6d8a` drops the thrower's live count. Missing it
/// (`$cab8`/`$cb28`) drops through to the strike instead -- which is the
/// game: reach for a disc, miss, and it hits you.
///
/// Returns the possibly-modified `world_z`. Only the catch and the anticipation
/// tail are modelled; everything else returns the candidate unchanged.
/// `// UNKNOWN: see bd discr-b6x`.
pub fn p2_hit_test(
    player: &mut Player,
    disc: &mut DiscSlot,
    x_cand: i16,
    z_before: i16,
    z_cand: i16,
    own_bank: &[Tile; TILE_CELLS],
) -> i16 {
    // $c826: a player already out does not catch.
    if player.down {
        return z_cand;
    }

    // $c82e-$c856: exactly $10fd8's crossing test against its own depth.
    let crossed = if disc.dir_kind >= 0 {
        z_before < player.world_y && z_cand >= player.world_y
    } else {
        z_before > player.world_y && z_cand <= player.world_y
    };
    if !crossed {
        // $cb2c: no crossing this frame, so try to anticipate one.
        anticipate(player, disc, x_cand, z_cand, own_bank);
        return z_cand;
    }

    // $c85a-$c87a: the owner check, then the three catch states.
    if disc.aim == PlayerId::One {
        // $ca96 (state 18) and $cb06 (state 27), two windows on the disc's X.
        let caught = match player.state_index {
            STATE_INTERCEPT => {
                let lo = player.world_x - CATCH_WIDTH_INTERCEPT;
                (lo..=lo + CATCH_WIDTH_INTERCEPT).contains(&x_cand)
            }
            STATE_REACH => {
                let lo = player.world_x - CATCH_LOW_REACH;
                (lo..=lo + CATCH_HIGH_REACH).contains(&x_cand)
            }
            // $cad0 is state 19's, and the strike path is $c934. Neither is
            // modelled, and neither fixture reaches either.
            _ => return z_cand,
        };
        if caught {
            // $caae / $cb1e, then $cab2 / $cb22.
            disc.active = disc.active.wrapping_add(crate::disc::ACTIVE_RETIRE_STEP);
            player.discs_out = player.discs_out.saturating_sub(1);
        } else {
            // $cab8 / $cb28: the miss goes on to the strike, which is not
            // modelled, but state 18's miss does set state 17 first ($cac6).
            if player.state_index == STATE_INTERCEPT {
                player.state_index = STATE_AFTER_CATCH;
            }
        }
    }
    z_cand
}

/// The state a missed intercept drops into. ST `$cac6`: `move.b #$11,$6d2e` --
/// the same state a completed throw goes to, which is [`STATE_STUB`].
pub const STATE_AFTER_CATCH: u8 = STATE_STUB;

/// ST state 18: stepping across to intercept (`$c6ec` entry 18, `$c196`).
pub const STATE_INTERCEPT: u8 = 0x12;

/// ST state 27: reaching for a disc without moving (`$c6ec` entry 27).
pub const STATE_REACH: u8 = 0x1b;

/// The reach the bonus code 5 substitutes for [`Player::reach`]. ST `$cb5e`.
pub const BONUS_REACH: i16 = 0x32;

/// Player 2's anticipation cascade: `$cb2c`-`$cc9a`, the tail of its hit test
/// `$c826`.
///
/// This is what **installs the two player-2 steering hooks**, and therefore what
/// bd discr-ovl.1 was asking about from the player-2 side. Three outcomes, and
/// `--watch` over `tests/fixtures/tile_damage.ndjson` counts them: `$cb70`
/// installs [`SteerHook::AtP2Wide`] 28 times, `$cbae` reaches (state 27) once,
/// and `$cc1e` steps across (state 18) once.
///
/// ```text
/// $cb2c  only from state 0, only a disc travelling AWAY (dir_kind > 0) and
///        owned by the other value, and only if $6d29 != 7
/// $cb52  d5 = own depth - reach   (or - $32 under bonus code 5)
/// $cb6a  the disc must be at least that deep -> otherwise nothing at all
/// $cb70  INSTALL $a7d8: start tracking it
/// $cb78  a narrow depth window, [d5 + reach - $c, +2]; outside it, stop here
/// $cb9e  then a ladder on the disc's X relative to $6d22 - 3, mirrored left
///        and right, ending in one of two responses
/// $cbae  REACH: keep $a7d8, animation $466a, state $1b
/// $cc1e  INTERCEPT: install $a816, animation $4612, state $12
/// ```
///
/// The choice between the two is a genuine little decision: **step across only
/// if the cell twelve units over is somewhere you could stand** -- either it is
/// the cell you are already on, or its type is non-zero in your own bank
/// (`$cc02`, `$cc10`). Otherwise just reach.
///
/// Not modelled: the `$6d29 != 7` guard at `$cb34` (that byte is stamped by the
/// throw states and is never 7 in either fixture, so inventing a value for it
/// would be worse than saying so), and `$cc3a clr.b $6d28`.
/// `// UNKNOWN: see bd discr-b6x`.
pub fn anticipate(
    player: &mut Player,
    disc: &mut DiscSlot,
    x_cand: i16,
    z_cand: i16,
    own_bank: &[Tile; TILE_CELLS],
) {
    // $cb2c-$cb4e: four gates, all of them exits.
    // (Reached from p2_hit_test, or directly when the crossing test fails.)
    if player.state_index != STATE_IDLE || disc.dir_kind <= 0 || disc.aim != PlayerId::One {
        return;
    }

    // $cb52-$cb6a: the tracking window's near edge.
    let reach = player.reach;
    let mut d5 = player.world_y - reach;
    if z_cand < d5 {
        return;
    }

    // $cb70: from here on the disc is tracked, whatever else happens.
    disc.hook = SteerHook::AtP2Wide;

    // $cb78-$cb9a: and a narrow window inside that, two units deep.
    d5 = d5 + reach - 0x0c;
    if z_cand < d5 {
        return;
    }
    d5 += 2;
    if z_cand > d5 {
        return;
    }

    // $cb9e-$cc9a: the X ladder, mirrored either side of $6d22 - 3.
    let pivot = player.world_x - 3;
    let step_across = if x_cand > pivot {
        // $cc40: the right-hand half. Untested -- neither fixture puts a disc
        // to player 2's right at the moment it starts tracking.
        if x_cand <= pivot + 0x0c {
            false
        } else if x_cand > pivot + 0x0c + 0x18 {
            return;
        } else {
            let probe = player.world_x + 0x0c;
            probe <= 0x98 && can_stand(player, probe, own_bank)
        }
    } else if x_cand == pivot {
        // $cbaa: dead on the pivot -- reach, do not step.
        false
    } else {
        // $cbcc: the left-hand half, which is the one both fixtures take.
        if x_cand >= pivot - 0x0f {
            false
        } else if x_cand < pivot - 0x0f - 0x22 {
            return;
        } else {
            let probe = player.world_x - 0x0c;
            probe >= 8 && can_stand(player, probe, own_bank)
        }
    };

    if step_across {
        // $cc1e-$cc34.
        disc.hook = SteerHook::AtP2Deep;
        player.state_index = STATE_INTERCEPT;
        enter_anim(player, ANIM_INTERCEPT);
    } else {
        // $cbae-$cbc4: the hook stays $a7d8.
        player.state_index = STATE_REACH;
        enter_anim(player, ANIM_REACH);
    }
}

/// `$cc02`/`$cc10`: is the cell over there one this player could stand on?
///
/// True when it is the cell they are already on, or when its type word is
/// non-zero in their own bank. `$cc16` (0) means reach, `$cc1a` (-1) step.
fn can_stand(player: &Player, probe_x: i16, own_bank: &[Tile; TILE_CELLS]) -> bool {
    let mut cell = usize::from(column(probe_x)) + GRID_CELL_BASE as usize;
    // $cbf6 / $cc6e: cmpi.w #$3a,$6d26 -- 58, not the movement code's 14.
    if player.world_y > 0x3a {
        cell += GRID_CELL_FAR_ROW as usize;
    }
    cell == player.grid_cell as usize
        || own_bank
            .get(cell)
            .is_some_and(|t| t.tile_type != TILE_TYPE_DESTROYED)
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
    // Almost every handler opens by stamping its own index into player+$09 --
    // $f5e2 writes 1, $f7f6 writes 2, $10554 writes $0b, $1057c writes $0c,
    // $1094a writes $14, $109aa writes $15, $10a72 writes $17, $10ac4 writes
    // $18, and player 2's $c196 writes $12 into $6d29. So the stamp is done
    // here, once, for all 32 entries including the ones whose behaviour is not
    // modelled. Two states are different, and both are measured rather than
    // assumed:
    //
    // * state 0 is the inline idle path, which *clears* the byte, and only when
    //   the joystick reads zero ($f1c0);
    // * state 17's handler is a four-byte stub that does nothing but jump to
    //   the shared animation tail -- `$1089a bra $f1c4` for player 1 and
    //   `$c192 bra $ac40` for player 2 -- so it never stamps. Comparing each
    //   table entry with the next handler in address order finds exactly one
    //   such stub per player, and it is state 17 in both.
    if player.state_index != STATE_IDLE && player.state_index != STATE_STUB {
        player.facing = player.state_index;
    }

    // ST $f108: `tst.b $6cae; bne $f5d0` -- state 0 is the inline idle path and
    // everything else dispatches through the 32-entry table at $10e2c.
    match player.state_index {
        STATE_IDLE => idle(player, input),
        STATE_TURN => turn(player),
        STATE_STRUCK_DOWN => struck_down(player),
        STATE_STRUCK_UP => struck_up(player),
        STATE_DEAD => dead(player),
        STATE_INTERCEPT => intercept(player, input),
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
        14 => {}      // $106b2, entered under Right+Fire. UNKNOWN: see bd discr-75o
        19 => {}      // $108f4. UNKNOWN: see bd discr-75o
        21 => {}      // $109aa. UNKNOWN: see bd discr-75o
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
            fire_held: false,
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
                fire_held: true,
            },
            &FLOOR,
            &mut Vec::new(),
        );
        assert_eq!(p.world_x, 114);
    }

    #[test]
    fn opaque_states_move_nothing() {
        // 0, 11, 12, 18, 20 and 23 left the list as Part 10 modelled them. What
        // is left changes exactly one field: every handler stamps $6ca9 with
        // its own index as its first instruction, modelled or not -- with 17
        // the single exception, a four-byte stub that stamps nothing.
        for state in [5, 14, 16, 19, 21, 24, 31] {
            let before = walking(state, 117);
            let mut p = before;
            run(&mut p, press(DirBits::LEFT), &FLOOR);
            assert_eq!(p.facing, state, "every handler stamps $6ca9");
            assert_eq!(
                Player { facing: 0, ..p },
                before,
                "state {state} must otherwise be a pass-through"
            );
        }

        // $1089a / $c192: state 17's handler is `bra` and nothing else.
        let before = walking(STATE_STUB, 117);
        let mut p = before;
        run(&mut p, press(DirBits::LEFT), &FLOOR);
        assert_eq!(p, before, "the state-17 stub touches nothing at all");
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
