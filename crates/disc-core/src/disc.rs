//! Disc flight, steering and impact. Owned by bead `discr-leu` (I3).
//!
//! ST `$a4ea` is the update loop, disassembled in full in the Part 10 section
//! of `docs/disc-notes.md`. Per record of the 8, stride `$42` from `$6e3e`:
//!
//! ```text
//! $a4f0  tst.b ($10,a5) ; beq next      ; +$10 == 0 -> the slot is FREE
//! $a534  tst.b ($10,a5) ; bpl next      ; bit 7 clear -> occupied, NOT simulated
//! $a53c  d0 = +$00 world_x   d1 = +$02 world_y   d2 = +$04 world_z
//! $a546  tst.l ($12,a5) ; jsr (a0)      ; the STEERING HOOK, if installed
//! $a552  d0 += +$06 vel_x
//! $a556  d1 += +$08 vel_y
//! $a55a  d2 += +$0a dir_kind
//! $a58e  d0 > $9b  -> clear hook, neg vel_x,    clamp d0 = $9b
//! $a5a6  d0 < 0    -> clear hook, neg vel_x,    clamp d0 = 0
//! $a5ba  d2 > $4f  -> clear hook, neg dir_kind, clamp d2 = $4f, far  wall
//! $a5fe  d2 < 0    -> clear hook, neg dir_kind, clamp d2 = 0,   near wall
//! $a640  vel_y decays one step toward zero
//! $a652  bsr $10fd8  (player 1's hit test)   $a656  bsr $c826  (player 2's)
//! $a65e  write d0/d1/d2 back
//! ```
//!
//! So the whole of flight is: optionally steer, integrate three coordinates by
//! three velocities, clamp each against a bound that negates its velocity, and
//! bleed `vel_y` back toward zero.
//!
//! # What Part 10 settled
//!
//! * **`world_y` IS integrated by `vel_y`** (`$a556`). The Part 9 note saying
//!   otherwise is retracted in `docs/disc-notes.md`: `vel_y` is decayed at
//!   `$a640` *after* the integration and before the sampling point, so a
//!   one-frame impulse moves `world_y` by one and reads back as 0. It is
//!   structurally invisible in a trace, which is why it looked inert.
//! * **The steering "gate" is [`DiscSlot::hook`]** (`disc+$12`), not a flag.
//!   Nothing gates `$a722`; it runs while a hook is installed. Each bound
//!   clears the hook, and only the two players' hit tests install one -- which
//!   is why both player-2 aim variants are live inside one round.
//! * **The bounds are `world_x` in `0..=155` and `world_z` in `0..=79`.** The
//!   ceiling this module used to refuse to invent is `$9b` = 155, and
//!   `tests/fixtures/golden.ndjson` reaches it exactly.
//! * **The "dwell" is not a `world_z` phase.** It is `disc+$10` losing bit 7,
//!   after which the ST stops simulating the record entirely -- see
//!   [`crate::DiscSlot::active`].
//!
//! # What this module still does NOT decide
//!
//! * **what installs a hook** -- the two hit tests `$10fd8` and `$c826` do, off
//!   a cascade of range checks against each player's position and reach. Not
//!   decoded, so [`DiscSlot::hook`] is an input here, the way `state_index` is
//!   an input to [`crate::player::step`]. `// UNKNOWN: see bd discr-ovl.1`.
//! * **what retires a disc** -- what takes `disc+$10` off `$ff`.
//!   `// UNKNOWN: see bd discr-0fm`.
//! * **[`serve`]'s trigger** -- now *known* (`$c06e`: player 2's animation
//!   cursor `$6d5a` reaching `$4602`) but it lives in player 2's control
//!   routine, which this crate does not model. `// UNKNOWN: see bd discr-b6x`.
//! * **[`impact`]'s cell** -- `$a24c` computes it as
//!   `colTable[$7c02 + world_x] + (4 if world_y > $46)`, and the caller is the
//!   near wall at `$a618`. The column table is `$7bfe` read at +4, which this
//!   crate does not carry. `// UNKNOWN: see bd discr-5w5`.
//!
//! Screen X/Y (`+$0c` / `+$0e`) are projection, recomputed every frame at
//! `$a6b2`/`$a6b6` from world (x, y, z) through LUTs. They are not state and do
//! not appear here.

use core::cmp::Ordering;

use crate::{
    COLUMN_TABLE_LEN, COLUMN_WIDTH, DirBits, DiscSlot, Event, Input, Player, PlayerId, SteerHook,
    TILE_CELLS, Tile, VEL_CLAMP, tile,
};

/// Subtracted from the aimed player's world X to get the disc's X aim point.
///
/// ST `$a71a` `steer_at_p1_x`: the target is `$6ca2 - $13`. ST `$a816` is the
/// player-2 variant of the same offset against `$6d22`.
pub const AIM_X_OFFSET: i16 = 0x13;

/// The other X aim offset: ST `$a7dc` `subq.w #$04,d5`.
///
/// `$a7d8` aims 4 short of player 2 rather than `$13` short, and steers only
/// the X axis. Which of the two a disc gets is decided by player 2's hit test
/// (`// UNKNOWN: see bd discr-ovl.1`), not by anything in this module.
pub const AIM_X_WIDE_OFFSET: i16 = 0x04;

/// The player record's constant height reference, ST `player+$04` (`$6ca4`).
///
/// ST Part 9: `+$04` is a **constant 99**, not the player's Y. The player's Y
/// is `+$06` (`$6ca6`) and it is *not* what vertical homing reads -- a core
/// that steers `vel_y` at `+$06` diverges from the trace. Held as a constant
/// rather than a [`Player`] field because it never changes and
/// `docs/state-schema.md` (read-only) does not model it.
pub const PLAYER_HEIGHT_REF: i16 = 99;

/// Subtracted from [`PLAYER_HEIGHT_REF`] to get the disc's Y aim point.
///
/// ST `$a758` `steer_at_p1_y`: the target is `$6ca4 - $10`, i.e. `99 - 16` =
/// 83, and the observed disc `world_y` settles at 83. That block never fired
/// in any trace we have, so [`step`] does not use this -- see [`aim_y`].
pub const AIM_Y_OFFSET: i16 = 0x10;

/// `dir_kind` written when a disc is served.
///
/// ST `$a618`: `move.w #$0001,($000a,a5)`. Observed: a racked disc reads `-3`
/// at world (140, 53); on launch it reads `+1` at world (135, 0) and `world_z`
/// starts climbing.
pub const SERVE_DIR_KIND: i16 = 1;

/// `dir_kind` on the return leg: the disc comes back three times faster.
///
/// Observed on both discs of `tests/fixtures/tile_damage.ndjson` and in
/// `golden.ndjson`. Note this is **not** `neg.w` of [`SERVE_DIR_KIND`]: the
/// magnitude changes 1 -> 3 across the turn, so `$a606` alone does not account
/// for the flip and something else writes the kind.
/// `// UNKNOWN: see bd discr-5w5`.
pub const RETURN_DIR_KIND: i16 = -3;

/// The near end of the run, where the return leg turns outbound again.
///
/// `world_z` clamps here rather than overshooting: fixture f70 steps 2 -> 0
/// under `dir_kind` -3 (a move of -2) and `dir_kind` becomes
/// [`SERVE_DIR_KIND`]. Disc 1 does the same at f208, so the near turn is in
/// the loop and [`step`] models it.
pub const Z_NEAR: i16 = 0;

/// The far bound on `world_z`. ST `$a5ba`: `cmp.w #$4f,d2`.
///
/// Reaching it clears the hook, negates `dir_kind` and clamps `world_z` to 79.
/// **No trace reaches it** -- every disc we have recorded is retired or
/// returned by player 2 well before, the deepest observed `world_z` being 54 --
/// so this bound is a code read with no trace behind it.
///
/// It is emphatically *not* where the old "dwell at 54" came from. That was
/// `disc+$10` losing bit 7; see [`crate::DiscSlot::active`].
pub const Z_FAR: i16 = 0x4f;

/// The low bound on `world_x`. ST `$a5a6`: `tst.w d0; bpl`.
pub const X_MIN: i16 = 0;

/// The disc reads the `$7bfe` column table **4 bytes in**. ST `$a250`:
/// `move.b ($04,a0,d0.w),d5`, where the player's own lookup at `$f65c` is
/// `move.b (a0,d0.w)`. So a disc's column boundaries sit 4 world units left of
/// a player's: the table is 152 bytes of `1 + x / 40`, and reading it at
/// `x + 4` puts the steps at 36, 76 and 116 rather than 40, 80 and 120.
pub const DISC_COLUMN_OFFSET: i16 = 4;

/// The disc's near/far row threshold. ST `$a254`: `cmp.w #$46,d1; ble`.
///
/// 70, against the *player* code's 14 (`$f838`) -- because a disc's `world_y`
/// runs around 81 and a player's around 18. Both add 4 to the cell index for
/// the far row.
pub const DISC_FAR_ROW_Y: i16 = 0x46;

/// The high bound on `world_x`. ST `$a58e`: `cmp.w #$9b,d0`.
///
/// The ceiling this module used to refuse to invent, now read off the
/// instruction. `tests/fixtures/golden.ndjson` reaches `world_x` 155 exactly,
/// so unlike [`Z_FAR`] this one is confirmed by trace as well as by code.
pub const X_MAX: i16 = 0x9b;

/// The `world_y` every served disc carries. ST `$c084` / `$c118`:
/// `move.w #$0051,d0`, i.e. **81**, literal in both throw states.
///
/// Not to be confused with the `$52` a disc record holds after `$aa50`'s round
/// init, which is a different value in a different place.
pub const SERVE_WORLD_Y: i16 = 0x51;

/// Player 2's throw states and what each one serves.
///
/// ST `$c6ec`, player 2's own 32-entry state table (player 1's is `$10e2c`).
/// Entry 15 is `$c068` and entry 16 is `$c0fe`, and the two blocks are the same
/// code with two constants swapped: the animation cursor value the release
/// happens on, and the X offset from the thrower.
///
/// | state | gate `player+$3a` | `world_x` | ST |
/// |---|---|---|---|
/// | 15 | `$4602` | `p2.x - 9` | `$c06e`, `$c07e` |
/// | 16 | `$45da` | `p2.x + 3` | `$c104`, `$c114` |
///
/// Six more `bsr $a972` sites exist inside `$abb2` (`$b462`, `$b47a`, `$b492`,
/// `$b512`, `$b52a`, `$b542`), so there are other throw states with other
/// parameter builds. These are the two the fixtures exercise.
/// `// UNKNOWN: see bd discr-b6x`.
pub const THROW_STATES: [(u8, u32, i16); 2] = [(15, 0x4602, -9), (16, 0x45da, 3)];

/// The state a thrower enters on the frame the disc leaves.
/// ST `$c0c4` / `$c158`: `move.b #$11,$6d2e`.
pub const STATE_AFTER_THROW: u8 = 0x11;

/// The disc's X aim point for the player it is homing on.
///
/// ST `$a71a` reads `$6ca2 - $13` for player 1. Player 2 has *two* observed
/// variants -- `$a7d8` (`$6d22 - 4`) and `$a816` (`$6d22 - $13`) -- and both
/// are live within one round, so which one a given disc uses is selected by
/// something that is not decoded. This uses the `$a816` form for both players.
/// `// UNKNOWN: see bd discr-217`.
#[must_use]
pub fn aim_x(players: &[Player; 2], aim: PlayerId) -> i16 {
    players[aim.index()].world_x - AIM_X_OFFSET
}

/// The disc's Y aim point. ST `$a758`: `$6ca4 - $10`, the same for both
/// players because `player+$04` is a constant.
///
/// Exposed but unused by [`step`]: `$a758` never fired in the 84 recorded
/// frames, so what gates it is not decoded. `// UNKNOWN: see bd discr-tan`.
#[must_use]
pub const fn aim_y() -> i16 {
    PLAYER_HEIGHT_REF - AIM_Y_OFFSET
}

/// One frame of velocity steering: the literal `$a722`-`$a758` rule.
///
/// Three cases and no others -- `d5` is the aim point, `d0` the coordinate,
/// `($0006,a5)` the velocity:
///
/// ```text
/// $a722  cmp.w d0,d5
/// $a724  bgt -> $a74c   aim > pos:  if vel < +2 then vel += 1   (clamp +2)
/// $a726  blt -> $a73c   aim < pos:  if vel > -2 then vel -= 1   (clamp -2)
///        else  $a728    aim == pos: vel decays TOWARD ZERO by 1
///                         $a72c bmi -> $a736 addq  (vel < 0: += 1)
///                         $a72e beq -> done        (vel == 0: nothing)
///                         $a730      subq          (vel > 0: -= 1)
/// ```
///
/// The at-target decay is the whole of the damping: no gap limiting, no
/// proportional term, no angle table.
///
/// [`step`] calls this with the **player-2** aim point ([`aim_x`] of
/// [`PlayerId::Two`], the `$a816` variant), which reproduces every velocity
/// transition on frames 12-34 of `tests/fixtures/tile_damage.ndjson`. Two
/// things about the call site are inferred rather than decoded: which aim
/// variant applies, and why the descent to the near bound is exempt. Both are
/// `// UNKNOWN: see bd discr-217`.
#[must_use]
pub fn steer(vel: i16, pos: i16, target: i16) -> i16 {
    match target.cmp(&pos) {
        // $a74c: bgt -- clamp +2.
        Ordering::Greater if vel < VEL_CLAMP => vel + 1,
        // $a73c: blt -- clamp -2.
        Ordering::Less if vel > -VEL_CLAMP => vel - 1,
        // $a728-$a736: at target, unwind one step toward zero.
        Ordering::Equal => vel - vel.signum(),
        _ => vel,
    }
}

/// One frame of *vertical* velocity steering: ST `$a758` / `$a854`.
///
/// The same three cases as [`steer`] with **one asymmetry, which is real**: the
/// downward branch has no clamp.
///
/// ```text
/// $a760  cmp.w d1,d5
/// $a762  bgt -> $a780   aim > pos:  if vel < +2 then vel += 1     (clamp +2)
/// $a764  beq -> $a76c   aim == pos: vel decays toward zero by 1
///        else  $a766    aim < pos:  vel -= 1                      (NO clamp)
/// ```
///
/// `$a7f8`, the X rule's `blt` arm, guards with `cmpi.w #-2,($06,a5); ble`.
/// `$a766` has no such guard, so a disc held above its aim point can build up
/// an unbounded downward `vel_y`. Transcribed rather than tidied: no trace
/// reaches a `vel_y` below -1, so this is a code read, and inventing a
/// symmetric clamp would be inventing a rule.
#[must_use]
pub fn steer_y(vel: i16, pos: i16, target: i16) -> i16 {
    match target.cmp(&pos) {
        // $a780: bgt -- clamp +2.
        Ordering::Greater if vel < VEL_CLAMP => vel + 1,
        Ordering::Greater => vel,
        // $a76c-$a77a: at target, unwind one step toward zero.
        Ordering::Equal => vel - vel.signum(),
        // $a766: subq.w #1 with no clamp.
        Ordering::Less => vel - 1,
    }
}

/// The `(x, y)` aim point a hook steers toward, or `None` for an axis the hook
/// does not touch.
///
/// The only difference between the three routines: `$a7d8` `rts`es at `$a814`
/// before the vertical block, so it never writes `vel_y`.
#[must_use]
pub fn aim_for(hook: SteerHook, players: &[Player; 2]) -> (Option<i16>, Option<i16>) {
    match hook {
        SteerHook::None => (None, None),
        // $a71a: $6ca2 - $13, falling through to $a758's $6ca4 - $10.
        SteerHook::AtP1 => (
            Some(players[PlayerId::One.index()].world_x - AIM_X_OFFSET),
            Some(aim_y()),
        ),
        // $a7d8: $6d22 - 4, X only.
        SteerHook::AtP2Wide => (
            Some(players[PlayerId::Two.index()].world_x - AIM_X_WIDE_OFFSET),
            None,
        ),
        // $a816: $6d22 - $13, falling through to $a854's $6d24 - $10.
        SteerHook::AtP2Deep => (
            Some(players[PlayerId::Two.index()].world_x - AIM_X_OFFSET),
            Some(aim_y()),
        ),
    }
}

/// Which grid cell a disc at `(world_x, world_y)` strikes. ST `$a24c`, the
/// first six instructions -- bd discr-5w5's `d5`.
///
/// ```text
/// $a24c  lea $7bfe,a0
/// $a250  move.b ($04,a0,d0.w),d5     ; the column, read 4 bytes in
/// $a254  cmp.w #$46,d1 ; ble $a25c
/// $a25a  addq.b #4,d5                ; world_y > 70 -> the far row
/// $a25c  ext.w d5 ; lsl.w #3,d5      ; x8, the tile stride
/// $a260  lea $7616,a0                ; the NEAR grid
/// ```
///
/// So the cell index is `column(world_x + 4) + (4 if world_y > 70)`, which
/// lands in **1..=8** -- and the player's `grid_cell` rule
/// (`8 + column(x) + (4 if y > 14)`) lands in 9..=16. The 17 cells at `$7616`
/// are therefore two banks of eight: **1..8 is the wall a disc hits and 9..16
/// is the floor the players walk on**, with index 0 unused. That is consistent
/// with the observed tile events -- cells 6, 7 and 8 changed under disc impact
/// and cell 14, in the floor bank, changed under something else (bd discr-b4q).
///
/// `None` when `world_x + 4` falls off the end of the 152-byte table, which the
/// ST would read as whatever byte follows it. `step` treats that as no impact
/// rather than guessing.
#[must_use]
pub fn disc_cell(world_x: i16, world_y: i16) -> Option<usize> {
    let x = world_x + DISC_COLUMN_OFFSET;
    if !(0..COLUMN_TABLE_LEN).contains(&x) {
        return None;
    }
    let column = 1 + (x / COLUMN_WIDTH) as usize;
    Some(column + if world_y > DISC_FAR_ROW_Y { 4 } else { 0 })
}

/// Advance one disc slot by one frame. ST `$a4ea`, one iteration.
///
/// Transcribed from the disassembly in the module docs, in the ST's order,
/// which matters in two places: the hook runs *before* the integration, and
/// `vel_y` decays *after* it.
///
/// 1. `$a4f0`/`$a534` -- an inactive slot is not touched at all. That is the
///    "dwell": see [`crate::DiscSlot::active`].
/// 2. `$a546` -- if a hook is installed, steer `vel_x` toward its X aim point
///    and, for the two hooks that do not `rts` early, `vel_y` toward its Y aim
///    point. [`aim_for`] has the table.
/// 3. `$a552`/`$a556`/`$a55a` -- integrate all three coordinates.
/// 4. `$a58e`..`$a5fe` -- four bounds. Each **clears the hook**, negates the
///    velocity that carried the disc into it, and clamps the coordinate:
///    `world_x` against [`X_MIN`]/[`X_MAX`] flipping `vel_x`, `world_z` against
///    [`Z_NEAR`]/[`Z_FAR`] flipping `dir_kind`.
/// 5. `$a640` -- `vel_y` decays one step toward zero.
///
/// Two ST behaviours inside this loop are deliberately **not** reproduced,
/// because their triggers are outside it:
///
/// * the near wall additionally forces `dir_kind` to `+1` and calls the
///   tile-damage routine (`$a618`), and the far wall forces `-1` and calls the
///   far grid's (`$a5d6`) -- but *only* for one value of `disc+$11`, whose
///   polarity is not settled (`// UNKNOWN: see bd discr-ovl.2`), and the
///   tile-damage call needs the `$7bfe` column table this crate does not carry
///   (`// UNKNOWN: see bd discr-5w5`). So the bounds here negate `dir_kind` and
///   stop, and `tiles` is untouched.
/// * the two hit tests at `$a652`/`$a656` are what install hooks and retire
///   discs. Not modelled: `// UNKNOWN: see bd discr-ovl.1`.
///
/// Because a bound clears the hook and the hit tests then re-install one within
/// the same ST frame, [`DiscSlot::hook`] at the sampling point is what the hit
/// tests decided, not what the bound left. A replay therefore has to reseed it
/// each tick, exactly as it reseeds `state_index`.
pub fn step(
    disc: &mut DiscSlot,
    _slot: usize,
    players: &[Player; 2],
    _tiles: &mut [Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
    // $a4f0 tst.b ($10,a5) beq / $a534 tst.b bpl: a slot the ST is not
    // simulating is not touched -- no integration, no decay, nothing. This is
    // the whole of the "dwell". // UNKNOWN (what retires it): see bd discr-0fm.
    if !disc.active {
        return;
    }

    // $a546-$a550: the hook, before the integrate.
    let (aim_x, aim_y) = aim_for(disc.hook, players);
    if let Some(target) = aim_x {
        disc.vel_x = steer(disc.vel_x, disc.world_x, target);
    }
    if let Some(target) = aim_y {
        disc.vel_y = steer_y(disc.vel_y, disc.world_y, target);
    }

    // $a552: d0 = world_x + vel_x, then $a58e/$a5a6 bound it. Both bounds do
    // the same three things: clear the hook, negate vel_x, clamp.
    match disc.world_x.saturating_add(disc.vel_x) {
        next if next > X_MAX => {
            disc.hook = SteerHook::None;
            disc.vel_x = disc.vel_x.wrapping_neg();
            disc.world_x = X_MAX;
        }
        next if next < X_MIN => {
            disc.hook = SteerHook::None;
            disc.vel_x = disc.vel_x.wrapping_neg();
            disc.world_x = X_MIN;
        }
        next => disc.world_x = next,
    }

    // $a556: add.w ($08,a5),d1. Unconditional, no bound of its own.
    disc.world_y = disc.world_y.saturating_add(disc.vel_y);

    // $a55a: d2 = world_z + dir_kind, then $a5ba/$a5fe bound it. The magnitude
    // of dir_kind is the step and its sign the direction, so the return leg
    // (-3) comes back three times faster than the outbound (+1) goes out.
    match disc.world_z.saturating_add(disc.dir_kind) {
        next if next > Z_FAR => {
            disc.hook = SteerHook::None;
            disc.dir_kind = disc.dir_kind.wrapping_neg();
            disc.world_z = Z_FAR;
            // $a5d0-$a5e0: for the OTHER owner value the ST forces dir_kind to
            // -1 and damages the far grid at $7596, a second 8-cell bank this
            // crate does not carry. // UNKNOWN: see bd discr-ovl.3.
        }
        next if next < Z_NEAR => {
            disc.hook = SteerHook::None;
            disc.world_z = Z_NEAR;
            if disc.aim == PlayerId::One {
                // $a618: this owner value forces dir_kind to +1 -- NOT the
                // neg.w, which would give +3 off the -3 return leg -- and then
                // calls the near grid's damage routine $a24c. tile_damage.ndjson
                // f70 is exactly this: dir_kind -3 -> +1 and cell 6 destroyed
                // on the same frame.
                disc.dir_kind = SERVE_DIR_KIND;
                if let Some(cell) = disc_cell(disc.world_x, disc.world_y) {
                    impact(disc, cell, _tiles, _events);
                }
            } else {
                // $a624: the other owner value transfers possession instead,
                // moving four counters this crate does not model. The neg.w
                // from $a602 stands. // UNKNOWN: see bd discr-ovl.2.
                disc.dir_kind = disc.dir_kind.wrapping_neg();
            }
        }
        next => disc.world_z = next,
    }

    // $a640-$a650: vel_y bleeds one step toward zero, AFTER the integration.
    // This is why vel_y reads 0 at every sampling point while world_y moves.
    disc.vel_y -= disc.vel_y.signum();
}

/// Serve a disc from `thrower` into the first free slot. ST `$c07a`-`$c0fa`
/// (player 2's state 15) or `$c110`-`$c18e` (state 16), then `$a972`'s slot
/// fill at `$a9a2`-`$a9cc`.
///
/// Returns the slot filled, or `None` when all eight are taken.
///
/// The parameter build, which is where every field of a served disc comes from:
///
/// ```text
/// $c07a  d0.high = $6d22 + x_offset      -> +$00 world_x
/// $c084  d0.low  = $0051                 -> +$02 world_y
/// $c088  d1.high = $6d26 - 1             -> +$04 world_z
/// $c0ae  d1.low  = 0, then +/-1 or +/-2  -> +$06 vel_x
/// $c090  d2.high = 0, or -5 if input bit 0  -> +$08 vel_y
/// $c090  d2.low  = $6d8e, or -5 if $6d9a == 2  -> +$0a dir_kind
/// $a9b8  st  ($10,a1)                    -> active
/// $a9bc  clr.b ($11,a1)                  -> owner 0
/// $a9c8  move.l a2,($12,a1)              -> the hook, from $6d4a
/// $a9cc  move.w $6d90,($16,a1)           -> +$16 damage
/// ```
///
/// Two details are easy to get backwards and both are transcribed from the
/// branch, not from intuition:
///
/// * **the sideways step doubles unless `dir_kind` is exactly -1.** `$c0e8
///   cmp.w #-1,d2; beq $c0f0` *skips* the second `addq`, so the -1 disc gets
///   the single step and every other kind gets two. An earlier note in
///   `docs/disc-notes.md` had this the wrong way round.
/// * `vel_y` and `dir_kind` come out of one register through **two** swaps.
///   Stopping at the first inverts them.
///
/// Not reproduced: the `$6d9a == 2` bonus, which serves `dir_kind` -5 instead
/// of the thrower's own. No trace has ever carried a non-zero bonus code, so
/// there is nothing to test it against. `// UNKNOWN: see bd discr-z8m`.
///
/// The free-slot search is `$a9a2 tst.b ($10,a1); bne next` -- `+$10 == 0`,
/// which is *not* the same test as [`DiscSlot::active`] (bit 7). A retired disc
/// counting down through 3, 2, 1 is neither active nor free, and this crate
/// cannot express that; in both fixtures the count reaches 0 before the slot is
/// reused. `// UNKNOWN: see bd discr-0fm`.
pub fn serve(
    discs: &mut [DiscSlot],
    thrower: &Player,
    input: Input,
    x_offset: i16,
    events: &mut Vec<Event>,
) -> Option<usize> {
    // $a9a2: the first record whose +$10 is zero.
    let slot = discs.iter().position(|d| !d.active)?;

    let dir_kind = thrower.throw_dir_kind;

    // $c0ae-$c0f0: nothing, left or right, and the step doubles unless the
    // kind is exactly -1.
    let step = if dir_kind == -1 { 1 } else { 2 };
    let vel_x = if input.dir.has(DirBits::LEFT) {
        -step
    } else if input.dir.has(DirBits::RIGHT) {
        step
    } else {
        0
    };

    discs[slot] = DiscSlot {
        active: true,
        aim: PlayerId::One,
        hook: SteerHook::None,
        world_x: thrower.world_x + x_offset,
        world_y: SERVE_WORLD_Y,
        world_z: thrower.world_y - 1,
        vel_x,
        // $c0a4/$c0aa: joystick bit 0 is UP.
        vel_y: if input.dir.has(DirBits::UP) { -5 } else { 0 },
        dir_kind,
        damage: thrower.throw_damage,
    };
    events.push(Event::DiscServed { slot });
    Some(slot)
}

/// Turn a disc around. ST `$a606`: `neg.w ($000a,a5)`.
///
/// A sign flip, not a comparison: the sign of `dir_kind` is the travel
/// direction and its magnitude is the kind of disc, so negating preserves the
/// kind. Nothing is known to happen to `vel_x`/`vel_y` here, so nothing does.
/// The condition that reaches `$a606` is not decoded.
/// `// UNKNOWN: see bd discr-5w5`.
pub fn reflect(disc: &mut DiscSlot, slot: usize, events: &mut Vec<Event>) {
    disc.dir_kind = disc.dir_kind.wrapping_neg();
    events.push(Event::DiscReflected { slot });
}

/// Apply this disc's damage to one floor cell. ST `$a310`:
/// `sub.w ($0016,a5),d6`, inside the disc loop.
///
/// A destroyed cell is skipped entirely -- the guard is on this side of the
/// call, not inside [`tile::damage`]:
///
/// ```text
/// $a2ec  tst.w ($00,a0,d5.w)   ; the cell's TYPE word (+$00)
/// $a2f0  beq.w $a3ea           ; type == 0 -> never reaches the damage
/// $a300  tst.w ($00,a0,d5.w)   ; the same test on the other path
/// $a304  beq.w $a3ea
/// ```
///
/// Which cell a flying disc strikes is not decoded (`d5`), so the caller names
/// it. `// UNKNOWN: see bd discr-5w5`.
///
/// The damage is applied **once**. `$a314  cmp.w #$0001,$00006d9a` /
/// `$a31c  sub.w ($0016,a5),d6` subtracts it a second time when `$6d9a` is 1,
/// and `$a32e` tests the same word against 3 for a further path; what `$6d9a`
/// means is not decoded, so the multiplier is not modelled and single
/// application is the base case. `// UNKNOWN: see bd discr-z8m`.
pub fn impact(
    disc: &DiscSlot,
    cell: usize,
    tiles: &mut [Tile; TILE_CELLS],
    events: &mut Vec<Event>,
) {
    // $a2ec/$a2f0: type == 0 skips the whole damage path.
    if !tiles[cell].walkable() {
        return;
    }
    tile::damage(tiles, cell, disc.damage, events);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players_at(x1: i16, x2: i16) -> [Player; 2] {
        [
            Player {
                world_x: x1,
                ..Player::default()
            },
            Player {
                world_x: x2,
                ..Player::default()
            },
        ]
    }

    fn flying(world_x: i16, world_y: i16) -> DiscSlot {
        DiscSlot {
            active: true,
            world_x,
            world_y,
            dir_kind: SERVE_DIR_KIND,
            ..DiscSlot::default()
        }
    }

    /// $a552/$a55a with no hook installed: world_x integrates by vel_x and
    /// world_z by dir_kind, and nothing else touches either. golden.ndjson
    /// frames 6-10 are this, at vel_x -2.
    #[test]
    fn world_x_integrates_by_vel_x_while_z_advances() {
        let players = players_at(117, 63);
        let mut disc = DiscSlot {
            vel_x: 2,
            ..flying(10, aim_y())
        };
        assert_eq!(disc.hook, SteerHook::None, "no hook, so no steering");
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        let mut clamped = false;
        for _ in 0..Z_FAR {
            let (prev_x, prev_z) = (disc.world_x, disc.world_z);
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc.world_z, prev_z + 1, "z advances by dir_kind (+1)");
            if disc.world_x == X_MAX {
                // $a58e: the only frame that does not integrate cleanly.
                assert_eq!(disc.vel_x, -2, "the ceiling negates vel_x");
                clamped = true;
            } else {
                assert_eq!(disc.world_x - prev_x, disc.vel_x, "{disc:?}");
            }
        }
        // 10 + 2*79 would be 168, so $a58e's ceiling is reached and bounces.
        assert!(clamped, "the run should reach the $9b ceiling");
        assert_eq!(disc.world_z, Z_FAR);
    }

    /// The "dwell" is `disc+$10` losing bit 7, not a `world_z` phase.
    /// golden.ndjson frames 124-126: `act` goes 255 -> 2 -> 1 -> 0 with
    /// world_z stuck at 54 -- nowhere near [`Z_FAR`] -- and the record then
    /// never moves again. So an inactive slot is simply not stepped, and
    /// nothing in this crate reactivates it (bd discr-0fm).
    #[test]
    fn the_dwell_is_an_inactive_slot_not_a_z_phase() {
        let players = players_at(117, 63);
        let frozen = DiscSlot {
            active: false,
            vel_x: 2,
            world_z: 54,
            hook: SteerHook::AtP2Deep,
            ..flying(40, 83)
        };
        let mut disc = frozen;
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for _ in 0..64 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc, frozen, "an inactive record is not touched at all");
        }
        assert!(
            disc.world_z < Z_FAR,
            "and it froze well short of the z bound"
        );
    }

    /// tile_damage.ndjson f53 -> f71: the return leg steps -3 a frame and
    /// $a5fe clamps at Z_NEAR while negating dir_kind. Disc 1 repeats the near
    /// turn at f208.
    #[test]
    fn the_return_leg_steps_three_and_the_near_bound_turns_it_outbound() {
        let players = players_at(117, 57);
        let mut disc = DiscSlot {
            world_z: 53,
            dir_kind: RETURN_DIR_KIND,
            ..flying(48, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for expected_z in [
            50, 47, 44, 41, 38, 35, 32, 29, 26, 23, 20, 17, 14, 11, 8, 5, 2,
        ] {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_z, disc.dir_kind), (expected_z, RETURN_DIR_KIND));
        }

        // f70: 2 - 3 would be -1, so $a5fe clamps -- and $a618 writes +1
        // outright rather than letting the neg.w stand, which off a -3 return
        // leg would have given +3.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_z, disc.dir_kind), (Z_NEAR, SERVE_DIR_KIND));
    }

    /// tile_damage.ndjson f0 -> f11, the descent to the near bound: the aim
    /// (p2 X 63 - $13 = 44) is above the disc the whole way down, so $a722
    /// would raise vel_x on every frame -- and the ST holds it at -2 until the
    /// floor flips it. The bound governs, not the steering.
    ///
    /// MODELLED, not mirrored: `vel_x < 0` is the exemption's shape here, not
    /// the ST's condition -- see step() and bd discr-217.
    #[test]
    fn the_near_bound_governs_the_descent() {
        let players = players_at(117, 63);
        assert_eq!(aim_x(&players, PlayerId::Two), 44);

        let mut disc = DiscSlot {
            vel_x: -2,
            world_z: 20,
            ..flying(21, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for expected_x in [19, 17, 15, 13, 11, 9, 7, 5, 3, 1] {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_x, disc.vel_x), (expected_x, -2), "{disc:?}");
        }

        // f11: the floor clamps and flips, and only then is the disc steered.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x), (0, 2));
    }

    /// $a250/$a25a: the disc's cell is the column table read at `x + 4`, plus 4
    /// for the far row. tile_damage.ndjson f70 destroys cell 6 with the disc at
    /// world (67, 81); f170 and f208 damage cells 7 and 8.
    #[test]
    fn disc_cell_matches_the_observed_impacts() {
        // f70: (67 + 4) / 40 = 1 -> column 2; y 81 > 70 -> +4 = cell 6.
        assert_eq!(disc_cell(67, 81), Some(6));
        // The 4-unit offset is real: 36 is already the second column for a
        // disc, where a player at 36 is still in the first.
        assert_eq!(disc_cell(35, 0), Some(1));
        assert_eq!(disc_cell(36, 0), Some(2));
        // Two banks of four, never the players' 9..16.
        for x in 0..148 {
            let near = disc_cell(x, DISC_FAR_ROW_Y).unwrap();
            let far = disc_cell(x, DISC_FAR_ROW_Y + 1).unwrap();
            assert!((1..=4).contains(&near), "x={x} -> {near}");
            assert_eq!(far, near + 4);
        }
        // Past the end of the 152-byte table there is no answer to give.
        assert_eq!(disc_cell(148, 81), None);
    }

    /// $a5fe/$a618: reaching the near bound damages the cell the disc is over
    /// and forces dir_kind to +1. tile_damage.ndjson f70: disc 0 arrives at
    /// world (67, 81) on a -3 return leg and cell 6 goes (1,1) -> (0,0).
    #[test]
    fn the_near_bound_damages_the_cell_the_disc_is_over() {
        let players = players_at(117, 63);
        let mut disc = DiscSlot {
            world_z: 2,
            dir_kind: RETURN_DIR_KIND,
            damage: 3,
            ..flying(67, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        tiles[6] = Tile {
            tile_type: 1,
            hp: 1,
        };
        let mut events = Vec::new();

        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_z, disc.dir_kind), (Z_NEAR, SERVE_DIR_KIND));
        assert_eq!(
            tiles[6],
            Tile {
                tile_type: 0,
                hp: 0
            },
            "hp 1 - 3 clamps to 0 and $a354 clears the type"
        );
        assert!(!events.is_empty(), "the impact is reported");
    }

    /// golden.ndjson frames 11-28, the two player-2 hooks back to back. This
    /// is the test the old `vel_x >= 0` invention was standing in for.
    ///
    /// Frames 11-21 run under `$a7d8` (aim `63 - 4` = 59, X only): the disc
    /// climbs from the floor at the +2 clamp and `world_y` does **not** move,
    /// because `$a7d8` returns before the vertical block. Frame 22 switches to
    /// `$a816` (aim `63 - $13` = 44, both axes) and `world_y` climbs 81 -> 82
    /// -> 83 on frames 23-24 and then stops dead on the aim point -- which is
    /// the whole of the retracted "vel_y is inert" story.
    #[test]
    fn the_two_player_two_hooks_differ_only_in_the_vertical_axis() {
        let players = players_at(117, 63);
        assert_eq!(aim_for(SteerHook::AtP2Wide, &players), (Some(59), None));
        assert_eq!(aim_for(SteerHook::AtP2Deep, &players), (Some(44), Some(83)));

        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        // f11 -> f21 under $a7d8: x climbs by 2, world_y pinned at 81.
        let mut disc = DiscSlot {
            vel_x: 2,
            world_z: 31,
            hook: SteerHook::AtP2Wide,
            ..flying(0, 81)
        };
        for n in 1..=10 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_x, disc.vel_x), (2 * n, 2), "{disc:?}");
            assert_eq!((disc.world_y, disc.vel_y), (81, 0), "$a7d8 is X only");
        }

        // f22 -> f24 under $a816: world_y climbs to the aim and stays.
        disc.hook = SteerHook::AtP2Deep;
        for expected_y in [82, 83, 83, 83] {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc.world_y, expected_y, "{disc:?}");
            assert_eq!(disc.vel_y, 0, "$a640 decays vel_y before the sample");
        }
    }

    /// $a722 lands the at-target decay on the same frame as the move it
    /// shortens, because the hook runs before the integrate: f33 has the disc
    /// on the aim point at vel_x 2 and f34 has it one further on at vel_x 1.
    #[test]
    fn step_steers_at_player_two_and_decays_on_the_aim_point() {
        let players = players_at(117, 63);
        let mut disc = DiscSlot {
            vel_x: 2,
            world_z: 31,
            hook: SteerHook::AtP2Deep,
            ..flying(44, 83)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x), (45, 1));
    }

    /// tests/fixtures/golden.ndjson frames 10 -> 12: world_x 1 -> 0 -> 2 with
    /// vel_x -2 -> +2. The step to 0 is only -1, so the position clamps, and
    /// the velocity flips on the same frame.
    ///
    /// MODELLED, not mirrored: the trigger is inferred from the fixture; the
    /// ST guard $a600 bpl on d2 is undecoded -- see bd discr-217.
    #[test]
    fn world_x_clamps_at_zero_and_flips_the_velocity() {
        let players = players_at(117, 63);
        let mut disc = DiscSlot {
            vel_x: -2,
            ..flying(1, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x), (0, 2), "clamped, then negated");
        // dir_kind rides through untouched: the fixture holds flag = 1.
        assert_eq!(disc.dir_kind, SERVE_DIR_KIND);

        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x), (2, 2), "and away it goes");
    }

    /// The floor has no mirror. What looked like upper turnarounds at world_x
    /// 45 and 113 were the Z_FAR dwells -- the disc parked, world_x stopped
    /// where it was -- so nothing in the evidence bounds world_x from above
    /// and step() lets it run past both values.
    #[test]
    fn there_is_no_ceiling() {
        // aim 133, above the whole run: the clamp holds vel_x at +2.
        let players = players_at(117, 152);
        let mut disc = DiscSlot {
            vel_x: 2,
            ..flying(110, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for _ in 0..8 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
        }
        assert_eq!((disc.world_x, disc.vel_x), (126, 2));
    }

    /// The other half of the invariant: z frozen => x frozen (17/17).
    #[test]
    fn an_inactive_slot_is_frozen() {
        let players = players_at(152, 8);
        let mut disc = DiscSlot {
            active: false,
            world_x: 40,
            vel_x: 2,
            ..DiscSlot::default()
        };
        let before = disc;
        let mut tiles = [Tile::default(); TILE_CELLS];
        step(&mut disc, 0, &players, &mut tiles, &mut Vec::new());
        assert_eq!(disc, before);
    }

    /// $a722-$a860: cmp.w #$fffe / cmp.w #$0002. Iterating the rule from any
    /// start converges inside the clamp and stays there.
    #[test]
    fn steered_velocity_stays_within_the_clamp() {
        for (mut pos, target) in [(0_i16, 133_i16), (153, -11)] {
            let mut vel = 0;
            for _ in 0..96 {
                vel = steer(vel, pos, target);
                assert!((-VEL_CLAMP..=VEL_CLAMP).contains(&vel), "{vel} @ {pos}");
                pos += vel;
            }
            // It arrives, then hunts within a step or two of the aim point:
            // the +/-1 nudge and the at-target decay cannot settle exactly
            // when the approach is at full speed. That hunt is the rule's,
            // not ours.
            assert!((pos - target).abs() <= VEL_CLAMP, "{pos} vs {target}");
        }
    }

    /// $a728-$a736: at the aim point the velocity unwinds toward zero one
    /// step per frame. That decay is the whole of the damping -- the only
    /// part of the steering rule the ST actually shows.
    #[test]
    fn at_target_the_velocity_decays_toward_zero() {
        assert_eq!(steer(2, 50, 50), 1);
        assert_eq!(steer(1, 50, 50), 0);
        assert_eq!(steer(0, 50, 50), 0);
        assert_eq!(steer(-1, 50, 50), 0);
        assert_eq!(steer(-2, 50, 50), -1);
        // $a724 / $a726: away from target, +/-1 up to the clamp and no further.
        assert_eq!(steer(1, 50, 60), 2);
        assert_eq!(steer(2, 50, 60), 2);
        assert_eq!(steer(-1, 50, 40), -2);
        assert_eq!(steer(-2, 50, 40), -2);
    }

    /// Part 9: vel_y is 0 on all 84 recorded frames and world_y still moves,
    /// so step() must not touch either. What advances world_y is bd discr-tan.
    #[test]
    fn step_leaves_world_y_and_vel_y_alone() {
        assert_eq!(aim_y(), 83);
        let players = players_at(117, 8);
        let mut disc = flying(0, 81);
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for _ in 0..96 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_y, disc.vel_y), (81, 0), "{disc:?}");
        }
    }

    /// $a606: neg.w -- a sign flip that preserves the kind magnitude.
    #[test]
    fn reflect_negates_dir_kind() {
        let mut disc = flying(140, 53);
        disc.dir_kind = -3;
        let mut events = Vec::new();
        reflect(&mut disc, 5, &mut events);
        assert_eq!(disc.dir_kind, 3);
        reflect(&mut disc, 5, &mut events);
        assert_eq!(disc.dir_kind, -3);
        assert_eq!(
            events,
            vec![
                Event::DiscReflected { slot: 5 },
                Event::DiscReflected { slot: 5 }
            ]
        );
    }

    /// golden.ndjson f51 -> f52, the re-serve of disc 0 from player 2's state
    /// 15. Player 2 at (57, 54) throwing right with dir_kind -3 puts the disc
    /// at world (57 - 9, 81, 54 - 1) with vel_x +2 -- doubled, because the
    /// doubling is skipped only for dir_kind -1.
    #[test]
    fn serve_reproduces_the_state_15_throw() {
        let thrower = Player {
            world_x: 57,
            world_y: 54,
            throw_dir_kind: -3,
            throw_damage: 3,
            ..Player::default()
        };
        let mut discs = [DiscSlot::default(); 8];
        let mut events = Vec::new();
        let input = Input {
            dir: DirBits::RIGHT,
            fire_edge: false,
        };

        assert_eq!(serve(&mut discs, &thrower, input, -9, &mut events), Some(0));
        let d = discs[0];
        assert!(d.active);
        assert_eq!((d.world_x, d.world_y, d.world_z), (48, 81, 53));
        assert_eq!((d.vel_x, d.vel_y, d.dir_kind), (2, 0, -3));
        assert_eq!(d.damage, 3, "$a9cc copies the thrower's +$70");
        assert_eq!(events, vec![Event::DiscServed { slot: 0 }]);
    }

    /// golden.ndjson f75 -> f76: state 16 is the same code with +3 instead of
    /// -9, and it fills slot 1 because slot 0 is still live.
    #[test]
    fn serve_takes_the_first_free_slot_and_state_16_offsets_the_other_way() {
        let thrower = Player {
            world_x: 49,
            world_y: 54,
            throw_dir_kind: -3,
            throw_damage: 3,
            ..Player::default()
        };
        let mut discs = [DiscSlot::default(); 8];
        discs[0].active = true;
        let mut events = Vec::new();
        let input = Input {
            dir: DirBits::RIGHT,
            fire_edge: false,
        };

        assert_eq!(serve(&mut discs, &thrower, input, 3, &mut events), Some(1));
        assert_eq!(
            (discs[1].world_x, discs[1].world_z, discs[1].vel_x),
            (52, 53, 2)
        );
    }

    /// $c0e8: the second addq is skipped only when dir_kind is exactly -1, so
    /// the -1 disc is the one served with the SINGLE sideways step.
    #[test]
    fn only_dir_kind_minus_one_gets_the_single_sideways_step() {
        let mut events = Vec::new();
        for (dk, expect) in [(-1, 1), (-3, 2), (1, 2), (-5, 2)] {
            let thrower = Player {
                throw_dir_kind: dk,
                ..Player::default()
            };
            let mut discs = [DiscSlot::default(); 8];
            serve(
                &mut discs,
                &thrower,
                Input {
                    dir: DirBits::LEFT,
                    fire_edge: false,
                },
                0,
                &mut events,
            );
            assert_eq!(discs[0].vel_x, -expect, "dir_kind {dk}");
        }
    }

    /// $c0a4/$c0aa: joystick bit 0 (up) serves vel_y -5 instead of 0.
    #[test]
    fn up_serves_a_negative_vel_y() {
        let thrower = Player {
            throw_dir_kind: -3,
            ..Player::default()
        };
        let mut discs = [DiscSlot::default(); 8];
        serve(
            &mut discs,
            &thrower,
            Input {
                dir: DirBits::UP,
                fire_edge: false,
            },
            0,
            &mut events_vec(),
        );
        assert_eq!(discs[0].vel_y, -5);
    }

    fn events_vec() -> Vec<Event> {
        Vec::new()
    }

    /// $a2ec/$a2f0: a hit on a destroyed cell (type 0) never reaches the
    /// damage path, so no second TileDestroyed event is emitted.
    #[test]
    fn impact_skips_a_destroyed_cell() {
        let mut tiles = [Tile::default(); TILE_CELLS];
        tiles[9] = Tile {
            tile_type: 0,
            hp: 0,
        };
        let mut disc = flying(80, aim_y());
        disc.damage = 3;
        let mut events = Vec::new();
        impact(&disc, 9, &mut tiles, &mut events);
        assert_eq!(tiles[9], Tile::default());
        assert!(events.is_empty());
    }
}
