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

/// How far a knocked-down player travels before it stops. Player 1 sinks
/// (`$1056a cmpi.w #$02,$6ca6; ble`, then `subq.w #1`); player 2 does the same
/// thing with both signs flipped (`$be6a cmpi.w #$45,$6d26; bge`, then
/// `addq.w #1`). Part 11i.
pub const STRUCK_Y_FLOOR: i16 = 2;
/// Player 2's bound for the same move. ST `$be6a`.
pub const STRUCK_Y_CEILING_P2: i16 = 0x45;
/// Where a rising player stops: `$10592 cmpi.w #$19,$6ca6; bge` for player 1,
/// `$be90 cmpi.w #$32,$6d26; ble` for player 2.
pub const RISE_Y_CEILING: i16 = 0x19;
/// Player 2's bound for the rise. ST `$be90`.
pub const RISE_Y_FLOOR_P2: i16 = 0x32;

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

/// One animation sequence, identified by the ST address a handler `lea`s.
///
/// A handler does not always load the *start* of a table: `$c1d4 lea $45f0,a1`
/// picks a cell partway into the block at `$45ea`, and the sequence then runs
/// forward from there to the same zero terminator. So a sequence is identified
/// by its **starting cursor**, which is what [`Player::anim_base`] holds, and
/// the holds recorded here are the ones from that cursor on.
///
/// The frame-block pointers are sprite data this crate does not carry; only the
/// holds matter to the state machine's timing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Anim {
    /// The ST address the handler loads into `$6cda` / `$6d5a`.
    pub start: u32,
    /// The hold count of each cell from there to the terminator.
    pub holds: &'static [u16],
}

macro_rules! anims {
    ($($name:ident = $addr:literal, $holds:expr, $doc:literal;)*) => {
        $(
            #[doc = $doc]
            pub const $name: Anim = Anim { start: $addr, holds: &$holds };
        )*
        /// Every sequence transcribed so far, for [`anim_for`].
        pub const ANIMS: &[Anim] = &[$($name),*];
    };
}

anims! {
    ANIM_P1_IDLE      = 0x2c78, [6, 6, 6, 6, 48, 6, 6, 6, 48, 6, 6, 6, 48, 6, 6, 6],
        "ST `$2c78`, loaded at `$f202`: player 1 standing, with three long pauses.";
    ANIM_TURN         = 0x2f7e, [4],
        "ST `$2f7e`, loaded at `$f27a` / `$f2ce` / `$f7c4` / `$f9e0`: the turn transient.";
    ANIM_STRUCK_DOWN  = 0x2d50, [4, 4],
        "ST `$2d50`, loaded at `$11226`: knocked down.";
    ANIM_STRUCK_UP    = 0x2d60, [4, 4],
        "ST `$2d60`, loaded at `$11210`: knocked upward.";
    ANIM_DEAD         = 0x2d70, [4; 16],
        "ST `$2d70`, loaded at `$f1a0`: out of energy. State 23 never leaves it.";
    ANIM_P1_WALK_LEFT = 0x2a8a, [4, 4, 4, 4, 4, 4],
        "ST `$2a8a`, loaded at `$f296`: player 1 walking left.";
    ANIM_P2_IDLE      = 0x468c, [6, 6, 6, 6, 48, 6, 6, 6, 48, 6, 6, 6, 48, 6, 6, 6],
        "ST `$468c`: player 2 standing -- the same shape as player 1's.";
    ANIM_INTERCEPT    = 0x4612, [6, 6, 6, 6],
        "ST `$4612`, loaded at `$cc26`: player 2 stepping across to intercept.";
    ANIM_REACH        = 0x466a, [6, 6, 4, 4, 4],
        "ST `$466a`, loaded at `$cbb6`: player 2 reaching without moving.";
    ANIM_AFTER_INTERCEPT = 0x45f0, [4, 4, 4, 4, 4],
        "ST `$45f0`, loaded at `$c1d4` when the intercept commits. Five cells of \
         four, so state 15 and the state 17 that follows the serve share twenty \
         frames between them -- which is what ends state 17.";
    ANIM_MISSED_CATCH = 0x462e, [6, 6],
        "ST `$462e`, loaded at `$cab8` when a catch misses.";
    ANIM_P2_THROW_LEFT = 0x45c2, [4, 4, 4, 4, 4, 4],
        "ST `$45c2`, loaded at `$ae70`: player 2's throw after stepping left.";
    ANIM_P2_SMASH_LEFT = 0x472a, [4, 4, 4, 4, 4, 4, 4, 4, 4],
        "ST `$472a`, loaded at `$aed4`: player 2's running smash to the left.";
    ANIM_P2_SMASH_RIGHT = 0x46f0, [4, 4, 4, 4, 4, 4, 4, 4, 4],
        "ST `$46f0`, loaded at `$af34`: the same, to the right.";
    ANIM_P1_INTERCEPT = 0x2bfe, [6, 6, 6, 6],
        "ST `$2bfe`, loaded at `$113ea`: player 1 stepping across to intercept -- \
         the same four cells of six as player 2's `$4612`.";
    ANIM_P1_REACH     = 0x2c56, [6, 6, 4, 4, 4],
        "ST `$2c56`, loaded at `$1137a`: player 1 reaching without moving, the \
         same shape as player 2's `$466a`.";
    ANIM_P2_STRUCK_DOWN = 0x4764, [4, 4],
        "ST `$4764`, loaded at `$ca5e`: player 2 knocked down. Read out of the \
         image -- two cells of four, the same shape as player 1's `$2d50`.";
    ANIM_P2_STRUCK_UP = 0x4774, [4, 4],
        "ST `$4774`, loaded at `$ca48`: player 2 knocked upward, mirroring \
         player 1's `$2d60`.";
    ANIM_P2_THROW_RIGHT = 0x45ea, [4, 4, 4, 4, 4, 4],
        "ST `$45ea`, loaded at `$ae0e`: the same, stepping right. `$45f0` -- the \
         sequence the intercept commits into -- is this table's second cell.";
    ANIM_P1_CATCH19_COMMIT = 0x2bb4, [4, 4, 4, 4, 4],
        "ST `$2bb4`, loaded at `$10932` when state 19 commits: five cells of \
         four, the same shape as `$45f0`/`ANIM_AFTER_INTERCEPT`.";
}

/// The sequence a handler loaded, by its ST address.
///
/// `None` for an address no handler this crate models ever loads -- there are
/// far more sequences in the image than the fixtures touch, and guessing at one
/// would be worse than treating it as unknown. `// UNKNOWN: see bd discr-75o`.
#[must_use]
pub fn anim_for(start: u32) -> Option<&'static Anim> {
    ANIMS.iter().find(|a| a.start == start)
}

/// Which sequence a player falls back to when one runs out. ST `$f202` for
/// player 1 and the same shape in player 2's tail.
#[must_use]
pub const fn idle_anim(who: PlayerId) -> Anim {
    match who {
        PlayerId::One => ANIM_P1_IDLE,
        PlayerId::Two => ANIM_P2_IDLE,
    }
}

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
pub fn enter_anim(player: &mut Player, anim: Anim) {
    player.anim_base = anim.start;
    player.anim_cell = 0;
    player.anim_shown = NO_CELL;
    player.anim_hold = anim.holds.first().copied().unwrap_or(1);
}

/// The animation tail every handler ends in. ST `$f1c4`, faithfully in order:
/// the frame block is copied *first* (which is what makes `$6ce4` the
/// previous frame's cell for the next handler run), then the hold is
/// decremented, then the cursor may advance.
pub fn anim_tick(player: &mut Player) -> AnimStep {
    // The sequence is whichever one the handler that entered this state loaded.
    let Some(anim) = anim_for(player.anim_base) else {
        // A sequence this crate has not transcribed: hold, rather than invent a
        // length. // UNKNOWN: see bd discr-75o.
        return AnimStep::Holding;
    };
    let holds = anim.holds;
    // $f1ca: the copy, before anything else. Only the cell identity matters here.
    player.anim_shown = player.anim_cell;

    // $f1ee: subq.w #1,$6ce2.
    player.anim_hold = player.anim_hold.saturating_sub(1);
    if player.anim_hold > 0 {
        return AnimStep::Holding;
    }
    // $f1f4/$f1f6: six bytes on, and reload the hold from the new cell.
    player.anim_cell = player.anim_cell.saturating_add(1);
    match holds.get(player.anim_cell as usize) {
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

/// How far ahead of the player [`walk_probe`] looks.
///
/// ST `$f60e`: `sub.w #$0018,d0`; ST `$f822`: `add.w #$0018,d0`. Player 2's
/// `$afee` uses the same 24.
pub const PROBE_AHEAD: i16 = 24;

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
/// result is at most `4 + 8 + 4 = 16` -- which is one PAST the 16-cell bank
/// (`TILE_CELLS`, discr-ovl.5): the far-right far-row cell reads the word at
/// `$7696`, past `$7616`'s end. The value is a compared field in its own
/// right; everything that uses it as an index goes through `.get`, which
/// reads past-the-bank as blocked.
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
fn walk(player: &mut Player, input: Input, facing: u8, held: DirBits, step_x: i16) {
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

    // ST $f658 / $f86c: subq.w/addq.w #3, UNCONDITIONAL once the direction
    // matches. The probe at $f60a does **not** gate it: the result goes into d2
    // ($f64e st d2), is consumed further on, and a second lookup at $f65c runs
    // on the NEW x. See [`walk_probe`], which computes that flag and which
    // nothing here calls, because what reads d2 is not decoded.
    // // UNKNOWN: see bd discr-75o.
    player.world_x = (player.world_x + step_x).clamp(WALK_X_MIN, WALK_X_MAX);

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
    anim_tick(player);
}

/// State 0, the idle path inlined in `$f104` rather than reached through the
/// jump table -- entry 0 of `$10e2c` is a null pointer.
///
/// Only the two walk directions are modelled. Up (`$f222` -> state 5), down
/// (`$f240`), fire (`$f21e bmi` -> `$f306`) and the `$6cac`/`$6cad` paths that
/// reach state 26 all lead into handlers whose behaviour is unrecovered, so
/// this leaves `state_index` alone for them rather than entering a state it
/// cannot then run. `// UNKNOWN: see bd discr-75o`.
fn idle(player: &mut Player, who: PlayerId, input: Input, own_bank: &[Tile; TILE_CELLS]) {
    // $f110-$f118 / $abbe-$abc6: consume the animation's X delta. Read, cleared,
    // and added to world_x -- and nothing recomputes grid_cell here, so the cell
    // a probe compares against is the one from the previous frame.
    player.world_x += player.x_delta;
    player.x_delta = 0;

    // $f11c: `tst.b $6cac; bne $f170` -- out of energy, so the idle path plays
    // the death sequence instead and never comes back. $f1b4 also sets $6d2d,
    // player 2's +$0d, which this crate does not model.
    if player.down {
        player.state_index = STATE_DEAD;
        enter_anim(player, ANIM_DEAD);
        // $f1b8 bra $f1c4: the tail runs on the entering tick too.
        anim_tick(player);
        return;
    }

    // $f1ba: `tst.b (a0); beq` -- a WHOLE-BYTE test, so $80 (fire held with no
    // direction) does not reach the clear at $f1c0. Getting that wrong makes a
    // player's +$09 drop to 0 on a frame the ST leaves it alone, which is what
    // tile_damage.ndjson frame 60 catches: the AI holds $80 there, and the
    // stamp from the throw it just finished stays put.
    if input.dir == DirBits(0) && !input.fire_held {
        player.facing = 0;
        return;
    }

    // $ad82 onward for player 2: fire with a direction chooses a throw. Player
    // 1's idle path takes a different branch at `$f21e bmi $f306`, which is not
    // decoded. // UNKNOWN: see bd discr-b6x.
    if input.fire_held && who == PlayerId::Two {
        p2_throw_choice(player, input, own_bank);
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
    if anim_tick(player) == AnimStep::Ended {
        player.state_index = player.pending_state;
    }
}

/// State 21: an unconditional three-unit slide left. ST `$109aa`.
pub const STATE_SLIDE_LEFT: u8 = 0x15;

/// How far state 21 (and its mirror, state 22) slides each frame. ST
/// `$109b0 subq.w #3,$6ca2` / `$10a14 addq.w #3,$6ca2`.
pub const SLIDE_STEP: i16 = 3;

/// State 21, an unconditional slide left with no gating at all. ST `$109aa`.
///
/// ```text
/// $109aa  move.b #$15,$6ca9
/// $109b0  subq.w #3,$6ca2          ; world_x -= 3, EVERY frame, no gate
/// $109b4  [the 20-byte tail copy, inlined -- same shape as state 20's]
/// $109ec  tst.l (a1) ; bne out     ; sequence not done -> just save the cursor
/// $109f2  lea $2c78,a1 ; $6cae = 0 ; -- ended: straight to idle
/// ```
///
/// Two things distinguish it from the turn transient (state 20), both
/// measured rather than assumed: the step is **unconditional**, run on every
/// call regardless of held input, and there is **no clamp** at
/// `WALK_X_MIN`/`WALK_X_MAX` -- unlike [`walk`], nothing here bounds
/// `world_x` at all. And the sequence running out lands directly on state 0,
/// not through `player.pending_state` the way state 20's does -- exactly
/// [`run_out`]'s ending, which is why this reuses it.
///
/// State 22 (`$10a0e`) is the mirror, `addq.w #3` instead of `subq.w #3`,
/// otherwise identical down to the byte. Neither state appears in any
/// committed fixture, and forcing player 1's rise (Up held, see
/// `dumps/state5_hunt`) never reached either one, so what enters 21/22 is
/// still unknown; only 21 is modelled, since guessing at 22 from its mirror
/// alone would be exactly the kind of inference the house rules forbid.
/// `// UNKNOWN (what enters state 21/22): see bd discr-75o`.
fn slide_left(player: &mut Player, who: PlayerId) {
    player.facing = STATE_SLIDE_LEFT;
    player.world_x -= SLIDE_STEP;
    run_out(player, who);
}

/// State 11, knocked down. ST `$10554` for player 1, `$be54` for player 2.
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
///
/// Player 2's copy at `$be54` is the same six instructions with **both signs
/// flipped** -- `cmpi.w #$45,$6d26; bge; addq.w #1` -- so it travels the other
/// way and stops at `$45` instead of `$02`. `p1_walk` frame 256 is where that
/// matters: player 2 goes 54 -> 55, and a shared `-1` left it at 54. Part 11i.
fn struck_down(player: &mut Player, who: PlayerId) {
    player.facing = STATE_STRUCK_DOWN;
    if anim_advanced(player) {
        match who {
            // $1056a/$10574
            PlayerId::One if player.world_y > STRUCK_Y_FLOOR => player.world_y -= 1,
            // $be6a/$be72
            PlayerId::Two if player.world_y < STRUCK_Y_CEILING_P2 => player.world_y += 1,
            _ => {}
        }
    }
    // $10578 bra $f1c4: the plain tail, so the sequence ending lands on state 0.
    if anim_tick(player) == AnimStep::Ended {
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
    anim_tick(player);
}

/// State 17. ST `$1089a` / `$c192`: `bra` to the shared animation tail and
/// nothing else -- no stamp, no movement, no decision.
///
/// So all it does is run whatever sequence the state that entered it had
/// loaded, and **the sequence running out is what leaves it**. After a serve
/// that sequence is `$45f0`, five cells of four, shared with the state 15 that
/// preceded it: `golden.ndjson` spends twelve frames in state 15 and seven in
/// state 17 and then falls back to state 0 on frame 59.
fn stub(player: &mut Player, who: PlayerId) {
    run_out(player, who);
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
///
/// **`who` matters here, and it did not used to be a parameter.** Player 1's
/// own copy of this handler is at `$1089e` (four bytes past the state-17
/// stub, which is why nothing had gone looking for it): `$108a4`/`$108b0`
/// gate on `$2c10`/`$2c20` where player 2 gates on `$4624`/`$4634` -- two
/// entirely different sequence tables, since each player's animation data
/// lives at its own addresses. The step itself is the same `subq.w #6` for
/// both. Before this fix, `intercept()` used player 2's release constants
/// unconditionally, so player 1 could stamp `$6ca9`, hold the intercept pose
/// and never commit -- silently, because `p1_walk.ndjson`'s frame 272-274
/// window (`discs_out == disc_cap` there, so the commit gate would have
/// bailed either way) ends exactly where player 1 first reaches state 18 and
/// never reaches a frame that would have told the two constants apart.
/// `// retract: see docs/disc-notes.md, Part 12 (discr-75o)`.
fn intercept(player: &mut Player, who: PlayerId, input: Input) {
    let (release_a, release_b) = intercept_release(who);
    if player.anim_cursor != release_a && player.anim_cursor != release_b {
        return;
    }
    if !input.fire_held || input.dir.has(DirBits::DOWN) || player.discs_out == player.disc_cap {
        return;
    }
    player.world_x -= INTERCEPT_STEP;
    player.grid_cell = grid_cell(player.world_x, player.world_y);
    // $c1d4-$c1e2: the throw's own sequence, then the state, then $c1e8's bra
    // into the tail -- so the entering tick advances it once like every other.
    player.state_index = STATE_THROW_STANDING;
    enter_anim(player, ANIM_AFTER_INTERCEPT);
    anim_tick(player);
}

/// The two animation-cursor checkpoints state 18 commits on, per player. ST
/// `$c19c`/`$c1a8` for player 2, `$108a4`/`$108b0` for player 1.
///
/// Player 1's first checkpoint (`$2c10`) is exactly `ANIM_P1_INTERCEPT`'s
/// fourth cell (`$2bfe + 3*6`), matching how player 2's `$4624` is
/// `ANIM_INTERCEPT`'s fourth cell (`$4612 + 3*6`) -- the same shape, cited
/// separately because inferring one from the other would be a guess, not a
/// measurement. Read live: `dumps/state5_hunt` (below) confirms the write
/// sites this function's callers rely on; the two checkpoints themselves are
/// disassembly-only, since no fixture or Hatari probe has driven player 1's
/// intercept to commit.
#[must_use]
const fn intercept_release(who: PlayerId) -> (u32, u32) {
    match who {
        PlayerId::One => (0x2c10, 0x2c20),
        PlayerId::Two => (INTERCEPT_RELEASE_A, INTERCEPT_RELEASE_B),
    }
}

/// The four throw states, whose handlers -- `$b3ee`, `$b4a0`, `$c068`, `$c0fe` --
/// all end in `bra $ac40`, the shared tail.
///
/// What they *decide* (whether the animation cursor has reached the release
/// frame) is in [`crate::GameState::tick`], because it builds a disc record.
/// What is here is the part every handler shares: the animation runs, and the
/// sequence ending drops the state back to 0.
///
/// Not modelled: states 3 and 4 also slide the player one unit a frame during
/// the wind-up and jump ten at one animation frame. `// UNKNOWN: see bd
/// discr-b6x`.
fn throwing(player: &mut Player, who: PlayerId) {
    run_out(player, who);
}

/// The commonest handler shape in either table: stamp `player+$09` (done for
/// every state in [`step`]), run the animation, and let the sequence running out
/// drop the state back to 0 through the tail's own ending (`$f202`-`$f210`, or
/// `$ac8c` for player 2).
///
/// States 17, 27 and the four throw states are all exactly this, which is why
/// they share one function. What distinguishes the interesting states is what
/// they do *besides* this -- sink a row, commit to a throw, damage a tile.
fn run_out(player: &mut Player, who: PlayerId) {
    if anim_tick(player) == AnimStep::Ended {
        enter_anim(player, idle_anim(who));
        player.state_index = STATE_IDLE;
    }
}

/// State 12, knocked upward. ST `$1057c`, the mirror of [`struck_down`].
///
/// Both arms of its `cmpi.w #$19,$6ca6` add one to `$6ca6`; the `>= 25` arm
/// (`$105a4`) then does more that is not decoded, so this models the add and
/// stops. `// UNKNOWN: see bd discr-75o`.
///
/// Player 2's copy is `$be7a`, and it is the same mirror as state 11's:
/// `cmpi.w #$32,$6d26; ble; subq.w #1`. Both of ITS arms subtract, and the
/// `<= $32` arm (`$bea0`) is the one with the undecoded tail. Part 11i.
fn struck_up(player: &mut Player, who: PlayerId) {
    player.facing = STATE_STRUCK_UP;
    if anim_advanced(player) {
        // Unbounded on purpose: both arms of the compare step, and only the
        // bounded arm's extra work is undecoded.
        match who {
            PlayerId::One => player.world_y += 1, // $1059c / $105a4
            PlayerId::Two => player.world_y -= 1, // $be98 / $bea0
        }
    }
    if anim_tick(player) == AnimStep::Ended {
        player.state_index = STATE_IDLE;
    }
    player.grid_cell = grid_cell(player.world_x, player.world_y);
}

/// Player 1's hit test. ST `$10fd8`, called from the disc loop at `$a652`
/// **between the integration and the write-back**, which is why it takes the
/// three candidate coordinates and returns a possibly-modified `world_z`.
///
/// Only the strike -- the disc hitting the player's body -- is modelled here.
/// [`anticipate`] models a third path, the tail at `$112f4`-`$1147a` every
/// non-crossing and non-strike falls into, which is what actually installs
/// `$a71a`/`$a78e` (bd discr-ovl.1, CLOSED -- see [`anticipate`]'s doc).
/// Two paths through this routine remain genuinely unmodelled:
///
/// * **states 7..10 are the racket** (`$11030`-`$11096`): the player is
///   swinging, the disc is caught inside a second, wider box built from
///   `$6cc6`/`$6cc8`, and `$110a6` adds `$6cc4` to its `vel_x`. Nothing found
///   ties this path to a hook install; it is a separate, still-open swing
///   mechanic. `// UNKNOWN: see bd discr-b6x`.
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
    own_bank: &[Tile; TILE_CELLS],
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
        // $10fee/$10ff6/$11000/$11008 all branch to $112f4, the cascade.
        anticipate(player, PlayerId::One, disc, x_cand, z_cand, own_bank);
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
    let bottom = crate::disc::PLAYER_HEIGHT_REF + b2;
    // $11108/$11114/$11122 and the fourth: every miss goes to $112f4 too.
    if x_cand < left || x_cand > right || y_cand < bottom || y_cand > bottom + b3 {
        anticipate(player, PlayerId::One, disc, x_cand, z_cand, own_bank);
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

/// ST state 17: a four-byte stub, `$1089a bra $f1c4` / `$c192 bra $ac40`. Its
/// handler has no body at all.
pub const STATE_STUB: u8 = 0x11;

/// ST state 3: the running smash to the left (`$fa0c` / `$b3ee`).
pub const STATE_SMASH_LEFT: u8 = 3;

/// ST state 4: the running smash to the right (`$fabe` / `$b4a0`).
pub const STATE_SMASH_RIGHT: u8 = 4;

/// Does this state's handler stamp its own index into `player+$09`?
///
/// **Derived, not assumed.** Reading the first instruction of all 64 handlers --
/// 32 per player -- gives the same answer for both tables: 28 open with
/// `move.b #<their own index>,player+$09`, and exactly three do not.
///
/// | state | first instruction | why |
/// |---|---|---|
/// | 3 | `$fa0c` / `$b3ee` `cmpi.b #$3,player+$09` | it **reads** the byte as a latch: "have I already committed this smash?" |
/// | 4 | `$fabe` / `$b4a0` `cmpi.b #$4,player+$09` | the same, mirrored |
/// | 17 | `$1089a` / `$c192` `bra` | the stub, no body |
///
/// State 0 is not in either table at all -- its inline path *clears* the byte,
/// and only when the whole input byte is zero.
///
/// This was got wrong twice before it was measured: first as "every handler
/// stamps" (Part 10f), then as "every handler except 17" (Part 10g). Both were
/// right about most states and wrong about the ones the fixtures spend time in.
#[must_use]
pub const fn stamps_facing(state: u8) -> bool {
    !matches!(
        state,
        STATE_IDLE | STATE_SMASH_LEFT | STATE_SMASH_RIGHT | STATE_STUB
    )
}

/// The wind-up slide's per-frame step. ST `$b3f6 subq.w #1` / `$b4a8 addq.w #1`.
pub const SMASH_SLIDE: i16 = 1;

/// The jump it makes at one animation frame. ST `$b404 subi.w #$a` /
/// `$b4b6 addi.w #$a`.
pub const SMASH_LUNGE: i16 = 0x0a;

/// The animation cursor a left smash lunges on. ST `$b3fa cmpi.l #$4742`.
pub const SMASH_LUNGE_LEFT_AT: u32 = 0x4742;

/// And a right one. ST `$b4ac cmpi.l #$4708`.
pub const SMASH_LUNGE_RIGHT_AT: u32 = 0x4708;

/// How much room a running smash needs in the direction it is already walking.
/// ST `$ae94 subi.w #$26` / `$aef4 addi.w #$26`.
pub const SMASH_PROBE: i16 = 0x26;

/// The two animation cursor values player 2's state 18 commits on. ST
/// `$c19c` / `$c1a8`. Player 1's own pair is different -- see
/// [`intercept_release`].
pub const INTERCEPT_RELEASE_A: u32 = 0x4624;
/// The other one.
pub const INTERCEPT_RELEASE_B: u32 = 0x4634;

/// How far the intercept steps, in one move. ST `$c1d0`: `subq.w #6,$6d22`.
pub const INTERCEPT_STEP: i16 = 6;

/// The throw state that steps RIGHT. ST `$c1e2` and `$ae1c`.
pub const STATE_THROW_STANDING: u8 = 0x0f;

/// The throw state that steps LEFT. ST `$ae7e`.
pub const STATE_THROW_LEFT: u8 = 0x10;

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
        anticipate(player, PlayerId::Two, disc, x_cand, z_cand, own_bank);
        return z_cand;
    }

    // $c85a-$c87a: only one owner value gets the catch states, and a state that
    // is not one of the three falls through to $c87e like the other owner does.
    let catch_window = if disc.aim == PlayerId::One {
        match player.state_index {
            // $ca96, the intercept's catch.
            STATE_INTERCEPT => {
                let lo = player.world_x - CATCH_WIDTH_INTERCEPT;
                Some((lo, lo + CATCH_WIDTH_INTERCEPT))
            }
            // $cb06, the reach's -- an asymmetric window, $10 below and $20 above.
            STATE_REACH => {
                let lo = player.world_x - CATCH_LOW_REACH;
                Some((lo, lo + CATCH_HIGH_REACH))
            }
            // $cad0 is state 19's; no fixture reaches state 19.
            _ => None,
        }
    } else {
        None
    };

    if let Some((lo, hi)) = catch_window {
        let caught = (lo..=hi).contains(&x_cand);
        if caught {
            // $caae / $cb1e, then $cab2 / $cb22.
            disc.active = disc.active.wrapping_add(crate::disc::ACTIVE_RETIRE_STEP);
            player.discs_out = player.discs_out.saturating_sub(1);
        } else {
            // $cab8 / $cb28: a missed catch goes on to the strike, and state
            // 18's miss sets state 17 on the way ($cac6).
            if player.state_index == STATE_INTERCEPT {
                player.state_index = STATE_AFTER_CATCH;
            }
            return strike(player, disc, x_cand, z_cand, own_bank);
        }
        return z_cand;
    }

    // $c87e: states 7..10 are the racket path, not modelled. Everything else
    // falls through to the body box.
    if (7..=10).contains(&player.state_index) {
        return z_cand;
    }
    strike(player, disc, x_cand, z_cand, own_bank)
}

/// Player 2's strike, `$c934`-`$ca10`: the mirror of [`hit_test`]'s body box.
///
/// Same four comparisons against the same animation-derived hit box, and the
/// same bounce. Two asymmetries with player 1's, both real and both transcribed
/// rather than assumed:
///
/// * **the owner gate is inverted.** `$1116e tst.b ($11,a5); bne` skips player
///   1's energy when the owner byte is non-zero; `$c9a6 tst.b ($11,a5); beq`
///   skips player 2's when it is zero. So the owner says whose energy is at
///   risk -- 0 docks player 1, anything else docks player 2.
/// * **the energies are at different offsets**: `$11178` reads `player+$76` and
///   `$c9b0` reads `player+$74`. The records are otherwise mirrored, so this is
///   a trap; emitting `+$76` for both reported player 2's energy as a constant
///   0 for four parts of this project.
///
/// A miss falls through to `$cb2c`, the anticipation cascade -- so a disc that
/// crosses player 2's depth and neither is caught nor connects is one player 2
/// starts tracking instead.
fn strike(
    player: &mut Player,
    disc: &mut DiscSlot,
    x_cand: i16,
    z_cand: i16,
    own_bank: &[Tile; TILE_CELLS],
) -> i16 {
    // $c934-$c964: the body box, from the current animation cell.
    let [b0, b1, b2, b3] = player.hit_box;
    let left = player.world_x - 8 + b0;
    let bottom = crate::disc::PLAYER_HEIGHT_REF + b2;
    if x_cand < left
        || x_cand > left + 8 + b1
        || disc.world_y < bottom
        || disc.world_y > bottom + b3
    {
        // $c940 and the three after it all branch to $cb2c.
        anticipate(player, PlayerId::Two, disc, x_cand, z_cand, own_bank);
        return z_cand;
    }

    // $c968: state $1a is a case of its own. // UNKNOWN: see bd discr-b6x.
    if player.state_index == 0x1a {
        return z_cand;
    }

    // $c9a6-$ca02, the inverted owner gate.
    if disc.aim != PlayerId::One {
        player.energy -= disc.damage;
        if player.energy < 0 {
            player.energy = 0;
            player.down = true;
        }
    }

    // $ca06-$ca0e: the bounce, applied to the candidate the loop writes back.
    disc.dir_kind = disc.dir_kind.wrapping_neg();
    disc.hook = SteerHook::None;
    let z = z_cand + disc.dir_kind;

    // $ca12-$ca78: the knock-down cascade, and it tests the ALREADY-NEGATED
    // dir_kind. The mirror of `$111da`'s, with the polarity flipped on both
    // arms -- player 1 goes UP on a negative dir_kind and player 2 goes DOWN.
    match player.state_index {
        // $ca12-$ca40 -> $ca7a: interrupting a walk, a turn or a throw keeps
        // the state and touches two fields this crate does not model ($6d4e,
        // $6d52) -- except $ca8e, the mirror of $11256.
        1 | 2 | 3 | 4 | 0x15 | 0x16 => {
            if disc.dir_kind < 0 {
                disc.dir_kind = -1;
            }
        }
        // $ca42 bmi $ca5e: still travelling away from player 1 -> state 11,
        // and the disc leaves at exactly -1 ($ca72).
        _ if disc.dir_kind < 0 => {
            player.state_index = STATE_STRUCK_DOWN;
            enter_anim(player, ANIM_P2_STRUCK_DOWN);
            disc.dir_kind = -1;
        }
        // $ca48: knocked the other way, and nothing is forced.
        _ => {
            player.state_index = STATE_STRUCK_UP;
            enter_anim(player, ANIM_P2_STRUCK_UP);
        }
    }
    z
}

/// The state a missed intercept drops into. ST `$cac6`: `move.b #$11,$6d2e` --
/// the same state a completed throw goes to, which is [`STATE_STUB`].
pub const STATE_AFTER_CATCH: u8 = STATE_STUB;

/// ST state 18: stepping across to intercept (`$c6ec` entry 18, `$c196`).
pub const STATE_INTERCEPT: u8 = 0x12;

/// ST state 27: reaching for a disc without moving (`$c6ec` entry 27).
pub const STATE_REACH: u8 = 0x1b;

/// ST state 19: player 1's own third catch/commit state, `$108f4`. Structurally
/// identical to [`STATE_INTERCEPT`]'s commit (`$1089e`/`$c196`) -- same four
/// gates, same shape of tail-load -- but it commits to [`STATE_THROW_LEFT`]
/// (`$10940 move.b #$10,$6cae`) instead of [`STATE_THROW_STANDING`], and steps
/// `world_x` the OPPOSITE direction (`$1092e addq.w #6`, not `subq.w #6`).
///
/// What selects state 19 over 18 or 27 is not decoded: [`anticipate`]'s X
/// ladder only ever picks [`STATE_INTERCEPT`] or [`STATE_REACH`], so entering
/// state 19 comes from a third branch this crate has not found yet -- which
/// is exactly why `p2_hit_test`'s own note calls its mirror "not modelled, no
/// fixture reaches state 19" (see [`p2_hit_test`]). No fixture and no Hatari
/// probe has reached it on player 1 either.
/// `// UNKNOWN (what enters state 19): see bd discr-75o`.
pub const STATE_CATCH19: u8 = 0x13;

/// The two animation-cursor checkpoints state 19 commits on. ST
/// `$108fa`/`$10906`. Player 1's `STATE_INTERCEPT` checkpoints are `$2c10`/
/// `$2c20` -- a different pair again, confirming each committing state reads
/// its own two-cell window rather than sharing one.
pub const CATCH19_RELEASE_A: u32 = 0x2c3c;
/// The other one. ST `$10906`.
pub const CATCH19_RELEASE_B: u32 = 0x2c4c;

/// How far state 19 steps on commit, and which way. ST `$1092e addq.w #6,$6ca2`
/// -- the opposite sign from `INTERCEPT_STEP`'s `subq.w #6`.
pub const CATCH19_STEP: i16 = 6;

/// State 19, player 1's other catch/commit state. ST `$108f4`.
///
/// ```text
/// $108f4  move.b #$13,$6ca9
/// $108fa  if $6cda is $2c3c or $2c4c -> commit, else just run the animation
/// $10912  btst #$7,(a0) ; beq out        ; fire must be HELD
/// $1091a  btst #$1,(a0) ; bne out        ; down must not be
/// $10922  if $6d0a == $6d0c -> out       ; a disc must be available
/// $1092e  addq.w #6,$6ca2                ; six units RIGHT, in one step
/// $10932  animation $2bb4 (5 cells of 4)
/// $10940  move.b #$10,$6cae              ; and straight into state 16
/// ```
///
/// Gate-for-gate the same shape as [`intercept`], and fully determined by
/// fields this crate already carries, so it is modelled the same way. Player
/// 2's mirror is not decoded (see [`STATE_CATCH19`]'s doc), so this is a
/// no-op for [`PlayerId::Two`] rather than a guess.
/// `// UNKNOWN (what enters this state): see bd discr-75o`.
fn catch19(player: &mut Player, who: PlayerId, input: Input) {
    if who != PlayerId::One {
        return;
    }
    if player.anim_cursor != CATCH19_RELEASE_A && player.anim_cursor != CATCH19_RELEASE_B {
        return;
    }
    if !input.fire_held || input.dir.has(DirBits::DOWN) || player.discs_out == player.disc_cap {
        return;
    }
    player.world_x += CATCH19_STEP;
    player.grid_cell = grid_cell(player.world_x, player.world_y);
    player.state_index = STATE_THROW_LEFT;
    enter_anim(player, ANIM_P1_CATCH19_COMMIT);
    anim_tick(player);
}

/// The reach the bonus code 5 substitutes for [`Player::reach`]. ST `$cb5e`.
pub const BONUS_REACH: i16 = 0x32;

/// The anticipation cascade: `$cb2c`-`$cc9a` for player 2, `$112f4`-`$1147a`
/// for player 1. The tail of both hit tests, and **what installs all four
/// steering hooks** -- bd discr-ovl.1 from both sides.
///
/// Every non-crossing and every body-box miss branches here (`$10fee`, `$10ff6`,
/// `$11000`, `$11008`, `$11108`, `$11114`, `$11122`, ... all `-> $112f4`), so a
/// disc that fails to hit a player is a disc that player starts tracking.
///
/// ```text
/// player 2 ($cb2c)                      player 1 ($112f4)
/// -----------------------------------   -----------------------------------
/// only from state 0, facing != 7, and   the same two gates ($112f4, $112fc)
///   a disc owned by the other value
/// dir_kind > 0 (bmi/beq exit)           dir_kind < 0 (bpl exit)
/// owner byte == 0 (bne exit)            owner byte != 0 (beq exit)
/// d5 = depth - reach   ($cb52)          d5 = depth + reach   ($11316)
///   or - $32 under bonus code 5           or + $32, on $6d1c not $6d9a
/// exit if the disc is shallower         exit if the disc is DEEPER ($11330)
/// INSTALL $a7d8  ($cb70)                INSTALL $a78e  ($11334)
/// narrow window [depth-$c, depth-$a]    narrow window [depth+$a, depth+$c]
/// REACH:     $466a, state $1b ($cbae)   REACH:     $2c56, state $1b ($11372)
/// INTERCEPT: $4612, state $12 ($cc1e)   INTERCEPT: $2bfe, state $12 ($113e2)
///                                                  and hook $a71a
/// ```
///
/// So the **depth axis mirrors and the X axis does not**. The ladder on the
/// disc's X relative to `own_x - 3` is the same seven constants for both
/// players -- `$c`/`$18` to the right, `$f`/`$22` to the left, a `$c` probe
/// either way -- because X is the same direction for both of them and depth is
/// not. Reading player 1's half after transcribing player 2's, that was the only
/// surprise: three sign flips and a different bonus word, and everything else
/// byte for byte.
///
/// The choice between the two responses is a genuine little decision: **step
/// across only if the cell twelve units over is somewhere you could stand** --
/// either it is the cell you are already on, or its type is non-zero in your own
/// bank (`$cc02`/`$cc10`, `$113c6`/`$113d4`). Otherwise just reach.
///
/// `--watch` over `tests/fixtures/tile_damage.ndjson` counts player 2's three
/// outcomes: `$cb70` installs [`SteerHook::AtP2Wide`] 28 times, `$cbae` reaches
/// once, `$cc1e` steps across once. Player 1's first observed use is `p1_walk`
/// frame 272.
///
/// Not modelled: `$cc3a clr.b $6d28` and its player-1 counterpart.
/// `// UNKNOWN: see bd discr-b6x`.
pub fn anticipate(
    player: &mut Player,
    who: PlayerId,
    disc: &mut DiscSlot,
    x_cand: i16,
    z_cand: i16,
    own_bank: &[Tile; TILE_CELLS],
) {
    // $cb2c-$cb4e / $112f4-$11312: four gates, all of them exits.
    if player.state_index != STATE_IDLE || player.facing == RACKET_FACING {
        return;
    }
    // $cb3e/$cb46 vs $11306: the disc must be travelling AWAY from this player,
    // and "away" is the opposite sign for each of them.
    let away = match who {
        PlayerId::One => disc.dir_kind < 0,
        PlayerId::Two => disc.dir_kind > 0,
    };
    // $cb4a bne vs $1130e beq: and it must be the other player's disc.
    let theirs = match who {
        PlayerId::One => disc.aim != PlayerId::One,
        PlayerId::Two => disc.aim == PlayerId::One,
    };
    if !away || !theirs {
        return;
    }

    // $cb52-$cb6a / $11316-$11330: the tracking window's near edge. The bonus
    // arm reads a different word per player and neither fixture has it set.
    // // UNKNOWN ($6d1c, player 1's bonus word): see bd discr-ovl.4.
    let reach = player.reach;
    let (sign, near) = match who {
        PlayerId::One => (1, player.world_y + reach),
        PlayerId::Two => (-1, player.world_y - reach),
    };
    // The disc must be at least this deep, measured along each player's own
    // sense of deep: $cb6a exits when shallower, $11330 when deeper.
    if (z_cand - near) * sign > 0 {
        return;
    }

    // $cb70 / $11334: from here on the disc is tracked, whatever else happens.
    disc.hook = match who {
        PlayerId::One => SteerHook::AtP1Wide,
        PlayerId::Two => SteerHook::AtP2Wide,
    };

    // $cb78-$cb9a / $1133c-$1135e: and a narrow window inside that, two units
    // deep, on the near side of the player.
    let inner = near - sign * reach + sign * 0x0c;
    if (z_cand - inner) * sign > 0 {
        return;
    }
    if (z_cand - (inner - sign * 2)) * sign < 0 {
        return;
    }

    // $cb9e-$cc9a / $11362-$1147a: the X ladder, mirrored either side of
    // own_x - 3 and IDENTICAL for the two players.
    let pivot = player.world_x - 3;
    let step_across = if x_cand > pivot {
        // $cc40 / $11404: the right-hand half. Untested for player 2 -- neither
        // fixture puts a disc to its right at the moment it starts tracking.
        if x_cand <= pivot + 0x0c {
            false
        } else if x_cand > pivot + 0x0c + 0x18 {
            return;
        } else {
            let probe = player.world_x + 0x0c;
            probe <= 0x98 && can_stand(player, who, probe, own_bank)
        }
    } else if x_cand == pivot {
        // $cbaa / $1136e: dead on the pivot -- reach, do not step.
        false
    } else {
        // $cbcc / $11390: the left-hand half, which is the one every observed
        // case takes.
        if x_cand >= pivot - 0x0f {
            false
        } else if x_cand < pivot - 0x0f - 0x22 {
            return;
        } else {
            let probe = player.world_x - 0x0c;
            probe >= 8 && can_stand(player, who, probe, own_bank)
        }
    };

    if step_across {
        // $cc1e-$cc34 / $113e2-$113fe.
        disc.hook = match who {
            PlayerId::One => SteerHook::AtP1,
            PlayerId::Two => SteerHook::AtP2Deep,
        };
        player.state_index = STATE_INTERCEPT;
        enter_anim(player, intercept_anim(who));
    } else {
        // $cbae-$cbc4 / $11372-$11388: the wide hook stays installed.
        player.state_index = STATE_REACH;
        enter_anim(player, reach_anim(who));
    }
}

/// The `facing` value that locks a player out of anticipating. ST `$cb34
/// cmpi.b #$7,$6d29` / `$112fc cmpi.b #$7,$6ca9`.
pub const RACKET_FACING: u8 = 7;

/// The sequence [`STATE_INTERCEPT`] enters. ST `$cc26` / `$113ea`.
const fn intercept_anim(who: PlayerId) -> Anim {
    match who {
        PlayerId::One => ANIM_P1_INTERCEPT,
        PlayerId::Two => ANIM_INTERCEPT,
    }
}

/// The sequence [`STATE_REACH`] enters. ST `$cbb6` / `$1137a`.
const fn reach_anim(who: PlayerId) -> Anim {
    match who {
        PlayerId::One => ANIM_P1_REACH,
        PlayerId::Two => ANIM_REACH,
    }
}

/// May this player step onto the cell `probe_x` units along? ST `$f60a`-`$f64a`
/// for player 1 and `$afea`-`$b022` for player 2.
///
/// The two are the same shape and differ in exactly three constants, worth
/// tabulating because guessing at any of them gets it wrong:
///
/// | | player 1 (`$f60a`) | player 2 (`$afea`) |
/// |---|---|---|
/// | probe distance | `-$18` = 24 | the same |
/// | off-arena bail | `cmp.w #$8; blt` -> **blocked** | the same |
/// | far-row test | `+4` when `$6ca6` **>** `$e` (14) | `+4` when `$6d26` **<=** `$3a` (58) |
/// | own-cell shortcut | `cmp.w $6cb0; beq` -> walkable | `cmp.w $6d30` |
/// | bank | `$7616` | `$7596` |
///
/// The far-row test's **polarity is inverted** between them, and both happen to
/// add 4 for the depths the fixtures use -- player 1 at 18, player 2 at 54 -- so
/// it is invisible in the data and would only bite a player at an unusual depth.
///
/// Two things this crate used to get wrong, both of which mattered:
///
/// * it **clamped** the probe into the walkable range instead of bailing, so an
///   off-arena probe read a cell rather than blocking;
/// * it had **no own-cell shortcut**, so a player standing on a collapsed cell
///   could not step off it -- which is why switching to the right bank on its
///   own made two fixtures worse instead of better.
///
/// **Nothing calls this**, and that is the finding. `$f64e st d2` puts the
/// result in a flag, `$f650`'s direction compare gates the move instead, and the
/// `subq.w #3` at `$f658` runs whatever the probe said. What consumes `d2` is
/// further down the handler and is not decoded. This crate treated the probe as
/// a gate for eleven parts; both committed fixtures agreed with it, because in
/// neither does a walking player ever probe a destroyed cell.
/// `// UNKNOWN (what reads d2): see bd discr-75o`.
#[must_use]
pub fn walk_probe(
    player: &Player,
    who: PlayerId,
    probe_x: i16,
    own_bank: &[Tile; TILE_CELLS],
) -> bool {
    // $f612 / $aff2: off the arena is blocked, not clamped.
    if !(8..crate::COLUMN_TABLE_LEN).contains(&probe_x) {
        return false;
    }
    let mut cell = usize::from(column(probe_x)) + GRID_CELL_BASE as usize;
    let far = match who {
        // $f624: cmpi.w #$e,$6ca6 ; ble skip.
        PlayerId::One => player.world_y > FAR_ROW_Y,
        // $b004: cmpi.w #$3a,$6d26 ; bgt skip -- the other way round.
        PlayerId::Two => player.world_y <= 0x3a,
    };
    if far {
        cell += GRID_CELL_FAR_ROW as usize;
    }
    // $f630 / $b010: the cell you are already on is always steppable.
    if cell == player.grid_cell as usize {
        return true;
    }
    // $f63e / $b01e: tst.w on that player's OWN bank.
    own_bank.get(cell).is_some_and(|t| t.walkable())
}

/// `$cc02`/`$cc10`: is the cell over there one this player could stand on?
///
/// True when it is the cell they are already on, or when its type word is
/// non-zero in their own bank. `$cc16` (0) means reach, `$cc1a` (-1) step.
fn can_stand(player: &Player, who: PlayerId, probe_x: i16, own_bank: &[Tile; TILE_CELLS]) -> bool {
    // $add2 / $ae36: the probe is rejected outright outside the arena.
    if !(8..=0x98).contains(&probe_x) {
        return false;
    }
    let mut cell = usize::from(column(probe_x)) + GRID_CELL_BASE as usize;
    // The far-row threshold, and the two sites disagree on the number the way
    // the players' depths do: $cbf6/$cc6e test $6d26 against $3a (58) and
    // $113ba/$11432 test $6ca6 against $e (14). Both are `greater than`.
    let far = match who {
        PlayerId::One => player.world_y > FAR_ROW_Y,
        PlayerId::Two => player.world_y > 0x3a,
    };
    if far {
        cell += GRID_CELL_FAR_ROW as usize;
    }
    cell == player.grid_cell as usize
        || own_bank
            .get(cell)
            .is_some_and(|t| t.tile_type != TILE_TYPE_DESTROYED)
}

/// How far the idle-path throw steps before it throws. ST `$ae84` / `$ae22`.
pub const THROW_SIDESTEP: i16 = 4;

/// How far to either side it probes. ST `$adce` / `$ae32`.
pub const THROW_PROBE: i16 = 0x0d;

/// Player 2's idle-path throw decision. ST `$ad82`-`$ae2a`.
///
/// ```text
/// $ad82  cmpi.b #$80,(a0) ; beq out    ; fire ALONE does nothing
/// $ad8a  btst #$1,(a0) ; bne $af50     ; down+fire goes elsewhere
/// $ad92  if $6d8a >= $6d8c -> out      ; already at the disc cap
/// $ad9e  if $6d29 is 1 or 2 -> $ae90 / $aef0
/// $adb2  btst #$2,(a0) ; bne $adca     ; LEFT held  -> probe RIGHT
/// $adba  btst #$3,(a0) ; bne $ae2e     ; RIGHT held -> probe LEFT
/// $adc2  tst.b $6d28 ; bne $ae2e       ; neither: the last throw's side picks
/// $ae70  state 16: sequence $45c2, x -= 4, st  $6d28      ; step LEFT
/// $ae0e  state 15: sequence $45ea, x += 4, clr $6d28      ; step RIGHT
/// ```
///
/// The two probe arms reach state 16 from **opposite** outcomes, which reads as
/// one rule rather than two: probing right and finding nowhere to go means go
/// left, and probing left and finding somewhere to go also means go left.
///
/// The row threshold inside [`can_stand`] is `$3a` = 58 while a player's own
/// `world_y` is 54, so the probe lands in the near row where `grid_cell` puts
/// them in the far one -- which is what makes the cells differ and the bank
/// lookup decide. Reading that threshold as the movement code's 14 made an
/// earlier attempt at this disagree with the trace.
///
/// Not modelled: `$af50` (down+fire) and the `$6d29` 1-and-2 variants at
/// `$ae90`/`$aef0`. `// UNKNOWN: see bd discr-b6x`.
fn p2_throw_choice(player: &mut Player, input: Input, own_bank: &[Tile; TILE_CELLS]) {
    // $ad82: exactly $80 -- fire with no direction -- does nothing at all.
    if input.dir == DirBits(0)
        || input.dir.has(DirBits::DOWN)
        || player.discs_out >= player.disc_cap
        || player.facing == 1
        || player.facing == 2
    {
        return;
    }

    let probe_right = if input.dir.has(DirBits::LEFT) {
        true
    } else if input.dir.has(DirBits::RIGHT) {
        false
    } else {
        !player.threw_left
    };
    let offset = if probe_right {
        THROW_PROBE
    } else {
        -THROW_PROBE
    };
    let standable = can_stand(player, PlayerId::Two, player.world_x + offset, own_bank);

    if standable != probe_right {
        // $ae70: step left and throw.
        player.world_x -= THROW_SIDESTEP;
        player.state_index = STATE_THROW_LEFT;
        player.threw_left = true;
        enter_anim(player, ANIM_P2_THROW_LEFT);
    } else {
        // $ae0e: step right and throw.
        player.world_x += THROW_SIDESTEP;
        player.state_index = STATE_THROW_STANDING;
        player.threw_left = false;
        enter_anim(player, ANIM_P2_THROW_RIGHT);
    }
    // $ae2a / $ae8c: bra $ac40 -- the tail runs on the entering tick.
    anim_tick(player);
}

/// `$ae90` and `$aef0`: which running smash a walking player commits to.
///
/// One probe, `$26` = 38 units in the direction already being walked -- far
/// enough that it is asking whether there is room for the whole run. If there
/// is, commit (state 3 left, state 4 right); if there is not, fall back to the
/// standing throw's own probe, `$adca` or `$ae2e`.
fn smash_choice(player: &mut Player, walking: u8, own_bank: &[Tile; TILE_CELLS]) {
    let left = walking == FACING_LEFT;
    let probe = if left { -SMASH_PROBE } else { SMASH_PROBE };
    if can_stand(player, PlayerId::Two, player.world_x + probe, own_bank) {
        if left {
            // $aed4-$aee8.
            player.state_index = STATE_SMASH_LEFT;
            player.threw_left = false;
            enter_anim(player, ANIM_P2_SMASH_LEFT);
        } else {
            // $af34-$af48.
            player.state_index = STATE_SMASH_RIGHT;
            player.threw_left = true;
            enter_anim(player, ANIM_P2_SMASH_RIGHT);
        }
        anim_tick(player);
    }
    // $af30 / $aece: no room for the run falls through to the standing throw's
    // probe, which needs the input byte this function does not carry. Not
    // modelled. // UNKNOWN: see bd discr-b6x.
}

/// The running smash, states 3 and 4. ST `$b3ee` and `$b4a0`, which are the
/// same code mirrored.
///
/// ```text
/// $b4a0  cmpi.b #$4,$6d29 ; beq $b4c2   ; already committed -> skip the slide
/// $b4a8  addq.w #1,$6d22                ; otherwise slide one unit a frame
/// $b4ac  cmpi.l #$4708,$6d5a ; bne      ; at one animation frame:
/// $b4b6  addi.w #$a,$6d22               ;   lunge ten more
/// $b4bc  move.b #$4,$6d29               ;   and latch, which stops the slide
/// $b4c2  cmpi.l #$471a,$6d5a            ; the release frame -- see
///                                       ; crate::disc::THROW_STATES
/// ```
///
/// So `player+$09` is doing double duty here: for 28 of the 31 handlers it is a
/// stamp, and for these two it is the latch that says the lunge has happened.
/// That is why they are the exceptions in [`stamps_facing`].
///
/// The release itself lives in [`crate::GameState::tick`], because it builds a
/// disc record.
fn smashing(player: &mut Player, who: PlayerId, step: i16, lunge_at: u32) {
    if player.facing != player.state_index {
        // $b4a8 / $b3f6: the wind-up slide.
        player.world_x += step;
        if player.anim_cursor == lunge_at {
            // $b4b6 / $b404, then $b4bc / $b40a's latch.
            player.world_x += SMASH_LUNGE * step;
            player.facing = player.state_index;
        }
        player.grid_cell = grid_cell(player.world_x, player.world_y);
    }
    run_out(player, who);
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
    who: PlayerId,
    input: Input,
    own_bank: &[Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
    // See stamps_facing: 28 of the 31 handlers in either table open by stamping
    // their own index into player+$09, and the three that do not are 3, 4 and
    // 17. Historical note kept because it was got wrong twice:
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
    if stamps_facing(player.state_index) {
        player.facing = player.state_index;
    }

    // ST $f108: `tst.b $6cae; bne $f5d0` -- state 0 is the inline idle path and
    // everything else dispatches through the 32-entry table at $10e2c.
    // $b1e0-$b1f8 (and $f5ea-$f606 for player 1): a fire press inside a walk
    // goes to the throw decision at $ad82, which sees this walk's own stamp in
    // player+$09 and routes to the running-smash chooser for that direction.
    // Player 1 reaches the equivalent only when its two disc counters differ,
    // and both are 0, so it never does.
    if matches!(player.state_index, 1 | 2)
        && input.fire_held
        && !input.dir.has(DirBits::DOWN)
        && player.discs_out != player.disc_cap
    {
        smash_choice(player, player.state_index, own_bank);
        return;
    }

    match player.state_index {
        STATE_IDLE => idle(player, who, input, own_bank),
        STATE_TURN => turn(player),
        STATE_STRUCK_DOWN => struck_down(player, who),
        STATE_STRUCK_UP => struck_up(player, who),
        STATE_DEAD => dead(player),
        STATE_INTERCEPT => intercept(player, who, input),
        STATE_CATCH19 => catch19(player, who, input),
        STATE_STUB => stub(player, who),
        15 | 16 => throwing(player, who),
        STATE_SMASH_LEFT => smashing(player, who, -SMASH_SLIDE, SMASH_LUNGE_LEFT_AT),
        STATE_SMASH_RIGHT => smashing(player, who, SMASH_SLIDE, SMASH_LUNGE_RIGHT_AT),
        // $c6ec entry 27, the reach: run the $466a sequence out and fall back.
        // Whether its handler does anything else is not decoded, but the
        // fixture's twenty-three frames in it match the sequence exactly.
        // // UNKNOWN: see bd discr-b6x.
        STATE_REACH => run_out(player, who),
        1 => walk(player, input, FACING_LEFT, DirBits::LEFT, -WALK_STEP),
        2 => walk(player, input, FACING_RIGHT, DirBits::RIGHT, WALK_STEP),
        STATE_SLIDE_LEFT => slide_left(player, who),
        // Tier-1 states: the handler address is known, the behaviour is not.
        // Opaque pass-through -- moving a field we cannot justify would only
        // make a trace comparison diverge on the fields these do not touch.
        // Decoded in full (docs/disc-notes.md, Part 12) but not implemented:
        // each one either falls into a subroutine this crate does not carry
        // ($f306's aerial throw commit, or $aae8's parabola calculation) or
        // -- states 24 and 31 -- ends in a mechanism (installing the $1334a
        // hook, then state 31's unconditional round-reset) this crate has no
        // event for. Inventing one would be exactly the guess the house
        // rules forbid; see bd discr-75o.
        5 => {}  // $fb6e: Up from idle, rising. -> 24 at the $19 (25) clamp.
        14 => {} // $106b2: Right+Fire windup. -> 31 when it completes.
        24 => {} // $10ac4: the hover atop a rise. -> 31 when it completes.
        31 => {} // $10dda: sets $6d2d and $6cac every frame -- a round reset.
        // States 16 and 17 used to sit here under bd discr-rf9, "never observed
        // in Hatari"; player 2 spends much of both fixtures in them and they
        // are modelled above. Every other index is still unattested.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 16 cells walkable, as the floor is before any disc lands.
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
        step(player, PlayerId::One, input, tiles, &mut Vec::new());
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
    fn a_destroyed_destination_cell_does_not_block_the_step() {
        // This test used to assert the opposite, and both committed fixtures
        // agreed with it -- because in neither does a walking player ever probe
        // a destroyed cell. $f658's `subq.w #3` is unconditional once $f650's
        // direction compare passes; the probe's answer goes into d2 and what
        // reads it is further down the handler. p1_walk frame 100 is the frame
        // that tells the two models apart, and it says the player moves.
        //
        // The destination cell here is 16 -- since discr-ovl.5 that is one
        // PAST the 16-cell bank, so the probe reads it as blocked without any
        // cell having to be destroyed, and the step still happens.
        let mut p = walking(2, 117);
        run(&mut p, press(DirBits::RIGHT), &FLOOR);
        assert_eq!(p.world_x, 120, "the step is not gated on the probe");
        assert_eq!((p.facing, p.grid_cell), (FACING_RIGHT, 16));
    }

    /// The probe itself, which nothing gates on but which is decoded exactly.
    /// `$f60a`-`$f64a` against `$afea`-`$b022`: the same shape with the far-row
    /// test's polarity inverted between the players.
    #[test]
    fn walk_probe_reads_each_player_own_bank_and_threshold() {
        let mut bank = FLOOR;
        // From X = 93 probing right, 117 -> column 3 -> +8 -> 11, +4 -> 15.
        bank[15] = Tile {
            tile_type: 0,
            hp: 0,
        };
        let p1 = Player {
            world_x: 93,
            world_y: 18,
            grid_cell: 14,
            ..Player::default()
        };
        assert!(!walk_probe(&p1, PlayerId::One, 117, &bank));
        assert!(walk_probe(&p1, PlayerId::One, 117, &FLOOR));

        // $f630 / $b010: the cell you are standing on is always steppable, even
        // destroyed -- which is what lets a player leave a collapsing tile.
        let standing = Player {
            grid_cell: 15,
            ..p1
        };
        assert!(walk_probe(&standing, PlayerId::One, 117, &bank));

        // Off the arena is blocked, not clamped ($f612 / $aff2).
        assert!(!walk_probe(&p1, PlayerId::One, 7, &FLOOR));

        // Cell 16 -- from X = 117 probing right, 141 -> column 4 -> 12, +4 --
        // is one PAST the 16-cell bank (discr-ovl.5). The ST's tst.w there
        // reads $7696, the word past $7616's end, which happens to hold (1,1);
        // disc-core keeps the bank honest at 16 cells and reads past-the-bank
        // as blocked. Nothing consumes the probe's answer (discr-75o), so no
        // observable behaviour hangs on the difference.
        let at_the_edge = Player {
            world_x: 117,
            world_y: 18,
            grid_cell: 15,
            ..Player::default()
        };
        assert!(!walk_probe(&at_the_edge, PlayerId::One, 141, &FLOOR));
        // ...unless the player is already standing on 16 -- the own-cell
        // shortcut fires before the bank read, exactly as on the ST.
        let standing_past = Player {
            grid_cell: 16,
            ..at_the_edge
        };
        assert!(walk_probe(&standing_past, PlayerId::One, 141, &FLOOR));

        // Player 2 at its own depth takes the far row through the OTHER test:
        // $b004 adds 4 when world_y <= $3a, where $f624 adds it when > $e.
        let p2 = Player {
            world_x: 93,
            world_y: 54,
            grid_cell: 14,
            ..Player::default()
        };
        assert!(!walk_probe(&p2, PlayerId::Two, 117, &bank));
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
            PlayerId::One,
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
        // 0, 11, 12, 18, 19, 20, 21 and 23 left the list as Part 10/12
        // modelled them. What is left changes exactly one field: every
        // handler stamps $6ca9 with its own index as its first instruction,
        // modelled or not -- with 17 the single exception, a four-byte stub
        // that stamps nothing.
        for state in [5, 14, 16, 24, 31] {
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

    /// State 19, off its release frame: a pass-through exactly like the
    /// states `opaque_states_move_nothing` covers, tested separately because
    /// this file also proves the release frame itself commits. ST `$108fa`/
    /// `$10906` gate on `$2c3c`/`$2c4c`; `player.anim_cursor` starts at 0 via
    /// `Player::default`, which is neither.
    #[test]
    fn state19_is_a_pass_through_off_its_release_frame() {
        let before = Player {
            discs_out: 0,
            disc_cap: 1, // a disc IS available -- only the anim gate should block it
            ..walking(STATE_CATCH19, 117)
        };
        let mut p = before;
        let fire = Input {
            dir: DirBits::NONE,
            fire_edge: false,
            fire_held: true,
        };
        run(&mut p, fire, &FLOOR);
        assert_eq!(p.facing, STATE_CATCH19, "every handler stamps $6ca9");
        assert_eq!(
            Player { facing: 0, ..p },
            Player {
                facing: 0,
                ..before
            },
            "off the release frame, state 19 touches nothing else"
        );
    }

    /// State 19's commit, ST `$10922`-`$10940`: fire held, down not held, a
    /// disc available, and `anim_cursor` at one of its two checkpoints ends
    /// in six units RIGHT (the opposite sign from `STATE_INTERCEPT`'s left
    /// step) and state 16, not 15.
    #[test]
    fn state19_commits_on_its_release_frame_with_fire_and_a_disc() {
        for release in [CATCH19_RELEASE_A, CATCH19_RELEASE_B] {
            let mut p = Player {
                discs_out: 0,
                disc_cap: 1,
                anim_cursor: release,
                ..walking(STATE_CATCH19, 117)
            };
            let fire = Input {
                dir: DirBits::NONE,
                fire_edge: false,
                fire_held: true,
            };
            run(&mut p, fire, &FLOOR);
            assert_eq!(p.world_x, 117 + CATCH19_STEP, "checkpoint {release:#x}");
            assert_eq!(p.state_index, STATE_THROW_LEFT);
            assert_eq!(p.anim_base, ANIM_P1_CATCH19_COMMIT.start);
        }

        // The same frame, but with no disc available: the cap gate still
        // wins over the anim gate, exactly like STATE_INTERCEPT's.
        let mut p = Player {
            discs_out: 1,
            disc_cap: 1,
            anim_cursor: CATCH19_RELEASE_A,
            ..walking(STATE_CATCH19, 117)
        };
        let fire = Input {
            dir: DirBits::NONE,
            fire_edge: false,
            fire_held: true,
        };
        run(&mut p, fire, &FLOOR);
        assert_eq!(p.world_x, 117, "no disc available -> no commit");
        assert_eq!(p.state_index, STATE_CATCH19);
    }

    /// State 21, ST `$109aa`: `world_x` moves left by exactly
    /// [`SLIDE_STEP`] on every single call, whether or not any input is
    /// held, and the sequence loaded on entry ending lands directly on idle
    /// -- unlike state 20, which redirects through `pending_state`.
    #[test]
    fn state21_slides_unconditionally_and_ends_at_idle() {
        let mut p = walking(STATE_SLIDE_LEFT, 117);
        enter_anim(&mut p, ANIM_TURN); // one cell, hold 4 -- reused for its shape only
        let none = Input::default();

        for expected_x in [114, 111, 108] {
            run(&mut p, none, &FLOOR);
            assert_eq!(p.world_x, expected_x, "unconditional, no input needed");
            assert_eq!(p.state_index, STATE_SLIDE_LEFT);
        }
        run(&mut p, none, &FLOOR);
        assert_eq!(p.world_x, 117 - 4 * SLIDE_STEP);
        assert_eq!(
            p.state_index, STATE_IDLE,
            "the sequence ending lands on idle directly, not via pending_state"
        );
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
        step(
            &mut p,
            PlayerId::One,
            press(DirBits::RIGHT),
            &FLOOR,
            &mut events,
        );
        assert!(events.is_empty());
    }

    /// States 11 and 12 are mirrored between the two tables: `$1056a` bounds
    /// player 1 below at `$02` and subtracts, `$be6a` bounds player 2 above at
    /// `$45` and adds. Getting this shared was `p1_walk`'s frame-256 wall.
    #[test]
    fn the_knockdown_is_mirrored_between_the_players() {
        for (who, from, want, bound) in [
            (PlayerId::One, 18i16, 17i16, STRUCK_Y_FLOOR),
            (PlayerId::Two, 54, 55, STRUCK_Y_CEILING_P2),
        ] {
            let mut p = Player {
                world_y: from,
                state_index: STATE_STRUCK_DOWN,
                ..Player::default()
            };
            enter_anim(&mut p, ANIM_STRUCK_DOWN);
            p.anim_shown = NO_CELL; // force anim_advanced on this pass
            struck_down(&mut p, who);
            assert_eq!(p.world_y, want, "{who:?} moves one row on a cell change");

            // At the bound it stops, and the two bounds are on opposite sides.
            let mut q = Player {
                world_y: bound,
                state_index: STATE_STRUCK_DOWN,
                ..Player::default()
            };
            enter_anim(&mut q, ANIM_STRUCK_DOWN);
            q.anim_shown = NO_CELL;
            struck_down(&mut q, who);
            assert_eq!(q.world_y, bound, "{who:?} stops at its own bound");
        }
    }
}
