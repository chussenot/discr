//! Disc flight, steering and impact. Owned by bead `discr-leu` (I3).
//!
//! ST `$a4ea` is the update loop entry (`lea $6e3e,a5`): 8 records of stride
//! `$42`. Per slot the ST *can* nudge `vel_x` by +/-1 toward the aimed player,
//! clamped `[-2,+2]` (`$a722`), integrates `world_x` by `+$06` (`$6e44`) and
//! advances `world_z` by `dir_kind`, flips `+$0a` on turn-around (`$a606`), and
//! on a tile hit subtracts `+$16` from the struck cell's HP (`$a31c`).
//!
//! # The flight cycle
//!
//! `world_z` runs a four-phase cycle between [`Z_NEAR`] and [`Z_FAR`], read off
//! `tests/fixtures/tile_damage.ndjson` (disc 0, twice over; disc 1 repeats the
//! near turn at f208):
//!
//! ```text
//! f0  ..34    dir_kind +1   z +1    wz 20 -> 54     outbound
//! f35 ..51    dir_kind +1   z  0    wz 54           DWELL, whole record frozen
//! f52 ..52    dir_kind -3   z -1    wz 53           far turn
//! f53 ..69    dir_kind -3   z -3    wz 50 -> 2      return, three times faster
//! f70 ..70    dir_kind +1   z -2    wz 0            near turn (clamped)
//! f71 ..124   dir_kind +1   z +1    wz 1  -> 54     outbound again
//! f125..151   dir_kind +1   z  0    wz 54           dwell again
//! ```
//!
//! The magnitude of `dir_kind` *is* the per-frame z step and its sign is the
//! direction of travel. [`step`] models the near turn (it is in the loop) but
//! **not** what ends the dwell -- see [`Z_FAR`] and bd discr-0fm.
//!
//! "Can": the `$a71a`/`$a722` steering block **does fire, and it is aimed at
//! player TWO**. With the aim at `$6d22 - $13` the rule predicts 23 of 23
//! velocity transitions on frames 12-34 of `tests/fixtures/tile_damage.ndjson`,
//! the last of them being the at-target decay itself -- p2 X 63, aim 44, disc
//! at 44, `vel_x` 2 -> 1. So [`step`] calls [`steer`]. It does *not* fire on
//! the descent to the near bound (f1-f11), where the aim sits above the disc
//! and the ST holds `vel_x` at -2 until the floor flips it: there the bound
//! governs. The rest -- integrate, floor at 0, sign-flip on the floor -- is
//! read off `tests/fixtures/golden.ndjson`, and the floor's coupling to the
//! flip is **modelled, not mirrored**. See [`step`] and bd discr-217.
//!
//! There is no possession: a disc is always in flight and always homing on a
//! target player, so the target lives in [`crate::DiscSlot::aim`].
//!
//! # What this module does NOT decide
//!
//! Two triggers inside the ST loop are not decoded, so they are exposed as
//! explicit API calls rather than invented here:
//!
//! * *which* aim variant steers -- [`steer`] is the literal `$a722` rule and
//!   the evidence runs it, but the ST has **two live player-2 variants** in
//!   one round: `$a816` (`$6d22 - $13`) fits f12-f34 of the fixture exactly
//!   and `$a7d8` (`$6d22 - 4`) fits f99-f124 of the same run exactly.
//!   [`step`] uses the `$a816` form; what selects between them is
//!   `// UNKNOWN: see bd discr-217`. So is what exempts the descent to the
//!   near bound from steering at all.
//! * [`serve`] -- `$a9a0` stores the spawn record and `$a618` writes
//!   `dir_kind = +1`, but nothing says what *causes* a serve.
//!   `// UNKNOWN: see bd discr-m4x`.
//! * vertical motion -- `vel_y` (+$08) is 0 on all 84 frames of
//!   `dumps/disc_trace` and `world_y` still moves, so `world_y` is not
//!   integrated by `vel_y` and the `$a758` vertical steering block never
//!   fired. [`step`] leaves `world_y` and `vel_y` alone; what writes `world_y`
//!   and what gates `$a758` are both `// UNKNOWN: see bd discr-tan`.
//! * [`reflect`] and [`impact`] -- `$a606` negates `+$0a` when the disc "turns
//!   around" and `$a31c` damages cell `($00,a0,d5.w)`, but neither the
//!   turn-around condition nor the computation of `d5` is decoded, so [`step`]
//!   never touches `tiles` and never reflects on its own.
//!   `// UNKNOWN: see bd discr-5w5`.
//!
//! Screen X/Y (`+$0c` / `+$0e`) are projection, recomputed every frame at
//! `$a6b2`/`$a6b6` from world (x, y, z) through LUTs. They are not state and do
//! not appear here.

use core::cmp::Ordering;

use crate::{DiscSlot, Event, Player, PlayerId, TILE_CELLS, Tile, VEL_CLAMP, tile};

/// Subtracted from the aimed player's world X to get the disc's X aim point.
///
/// ST `$a71a` `steer_at_p1_x`: the target is `$6ca2 - $13`. ST `$a816` is the
/// player-2 variant of the same offset against `$6d22`.
pub const AIM_X_OFFSET: i16 = 0x13;

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

/// The far end of the run, where the disc **dwells**.
///
/// While `world_z` is here and `dir_kind` is still outbound the entire record
/// is static -- `world_x` included, even with a nonzero `vel_x`. That is the
/// "freeze" of bd discr-0fm, and it is a phase of the cycle, not an anomaly.
///
/// **How long it lasts is not decoded**: 17 frames in the fixture's first
/// cycle (f35-51) and 27 in its second (f125-151), so there is no constant to
/// mirror. [`step`] therefore *enters* the dwell and never leaves it on its
/// own: the disc stays frozen until something writes a negative `dir_kind`.
/// `// UNKNOWN: see bd discr-0fm`.
///
/// The one lead, unmodelled and n=2: both exits coincide with the aimed
/// player entering state 17 (f52, f152), which is also the frame slot 1 is
/// served at f190 -- so state 17 looks like "the opponent plays the disc".
/// Not enough to build on; see the report on bd discr-0fm.
///
/// Leaving the dwell costs one z step, not three (54 -> 53 at f52 and f152).
pub const Z_FAR: i16 = 54;

/// Disc `world_y` at round init.
///
/// ST record layout: `disc+$02` is `$52` after `$aa50`. Offered as a default
/// for [`serve`]'s `world_y`; the rest of the `$a9a0` field table is not
/// recovered (`// UNKNOWN: see bd discr-st8`).
pub const SERVE_WORLD_Y: i16 = 0x52;

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

/// Advance one disc slot by one frame. ST `$a4ea`, one iteration.
///
/// Five things happen, and only these five:
///
/// * a **dwell** at [`Z_FAR`] -- while the disc is parked at the far end with
///   an outbound `dir_kind` the whole record is static and this returns early.
///   Nothing here ends it; see [`Z_FAR`] and bd discr-0fm;
/// * **steering** -- `$a722` nudges `vel_x` one step toward player 2's aim
///   point, *before* the integrate: fixture f33 has the disc at 44 with
///   `vel_x` 2 and f34 has it at 45 with `vel_x` 1, so the decay lands on the
///   same frame as the move it shortens. Suppressed while `vel_x` is
///   negative -- see below;
/// * `world_x += vel_x` -- the invariant the notes verified 47/48 frames while
///   `world_z` advances, with `world_x` frozen whenever `world_z` is (17/17);
/// * a **floor at `world_x == 0`** -- golden frame 10 -> 11 steps 1 -> 0, a
///   move of -1 under `vel_x` = -2, so the position clamps rather than passing
///   zero -- and `vel_x` **sign-flips on that clamp**, -2 -> +2 on the same
///   frame;
/// * `world_z += dir_kind`, bounded by [`Z_NEAR`] and [`Z_FAR`], with the near
///   bound clamping and turning the disc outbound.
///
/// `// MODELLED, not mirrored: trigger inferred from the fixture; ST guard
/// $a600 bpl undecoded -- see bd discr-217`. The `neg.w` at `$a606` is real;
/// what reaches it is a `bpl` on `d2` whose meaning is not decoded, so the
/// floor-to-flip coupling is an inference from the trace, not a transcription.
///
/// There is deliberately **no ceiling** on `world_x`. What earlier read as
/// "upper turnarounds at `world_x` 45 and 113" were the [`Z_FAR`] dwells: the
/// disc is not turning there, it is parked, and `world_x` simply stops
/// wherever it had got to -- which is why the two values differ. Nothing in
/// evidence bounds `world_x` from above. The asymmetry with the floor is the
/// evidence's, not a bug.
///
/// [`steer`] **is** called, at `players[1]` -- see the module docs. Two parts
/// of that call are inferred: the aim variant (`$a816`, `$6d22 - $13`) and the
/// exemption of the descent. `// UNKNOWN: see bd discr-217`.
///
/// `tiles` is mutable because tile damage happens inside this loop on the ST
/// (`$a31c`-`$a360`), but the collision test that selects the struck cell is
/// not decoded, so nothing here writes it -- see [`impact`].
/// `// UNKNOWN: see bd discr-5w5`.
///
/// `players` is read for the aim point only. [`DiscSlot::aim`] is *not*
/// consulted: both decoded variants read player 2's record (`$6d22`), nothing
/// decoded selects the aimed player, and the traces carry no column for it.
/// `// UNKNOWN: see bd discr-217`.
pub fn step(
    disc: &mut DiscSlot,
    _slot: usize,
    players: &[Player; 2],
    _tiles: &mut [Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
    if !disc.active {
        return;
    }

    // The dwell at the far end: the WHOLE record is static, world_x included,
    // even though vel_x is 1 throughout (fixture f35-51 and f125-151). Its
    // duration is not decoded -- 17 frames then 27 -- so nothing here ends it
    // and the disc waits for a negative dir_kind from outside.
    // // UNKNOWN: see bd discr-0fm.
    if disc.world_z >= Z_FAR && disc.dir_kind > 0 {
        return;
    }

    // $a71a/$a722: nudge vel_x one step toward the aim point, before the
    // integrate. The aim is PLAYER TWO's X, $6d22 - $13 (ST $a816): with it the
    // rule predicts 23 of 23 velocity transitions on f12-f34 of
    // tests/fixtures/tile_damage.ndjson, f34 being the at-target decay -- p2 X
    // 63, aim 44, disc at 44, vel_x 2 -> 1. The other live variant, $a7d8
    // ($6d22 - 4), fits f99-f124 of the same run; what selects between them is
    // // UNKNOWN: see bd discr-217.
    //
    // MODELLED, not mirrored: the descent to the near bound is exempt. On
    // f1-f11 the disc falls 21 -> 0 with the aim (44) above it the whole way,
    // so $a722 would raise vel_x on every one of those frames and the ST holds
    // it at -2 until the floor flips it -- the bound governs, not the
    // steering. `vel_x < 0` reproduces f1-f34, but it is the shape of the
    // stretch we can measure, not the ST's condition: the $a7d8 stretch does
    // steer with vel_x negative. // UNKNOWN: see bd discr-217.
    if disc.vel_x >= 0 {
        disc.vel_x = steer(disc.vel_x, disc.world_x, aim_x(players, PlayerId::Two));
    }

    // $6e44: world X integrates by vel_x, with a floor at 0 that sign-flips
    // the velocity ($a606 neg.w).
    // MODELLED, not mirrored: trigger inferred from the fixture; ST guard
    // $a600 bpl on d2 undecoded -- see bd discr-217.
    match disc.world_x.saturating_add(disc.vel_x) {
        next if next < 0 => {
            disc.world_x = 0;
            // dir_kind is NOT touched: the fixture holds it at +1 across the
            // flip, so whatever $a606 negates here, it is not that field.
            disc.vel_x = disc.vel_x.wrapping_neg();
        }
        next => disc.world_x = next,
    }

    // world_y is deliberately NOT advanced. In dumps/disc_trace (84 frames)
    // vel_y (+$08) is 0 on every frame while world_y (+$02) moves on three
    // frame pairs, so world_y is not integrated by vel_y and the $a758
    // vertical block never fired -- neither its gate nor world_y's writer is
    // decoded. // UNKNOWN: see bd discr-tan.

    // disc+$04: world Z advances by dir_kind -- the sign is the direction of
    // travel, the magnitude is the step, so the return leg (-3) comes back
    // three times faster than the outbound (+1) goes out.
    match disc.world_z.saturating_add(disc.dir_kind) {
        // The near bound clamps and turns the disc outbound: f70 steps 2 -> 0
        // under dir_kind -3 and writes dir_kind +1; slot 1 does the same at
        // f208. Both discs, so this one is mirrored.
        next if next < Z_NEAR => {
            disc.world_z = Z_NEAR;
            disc.dir_kind = SERVE_DIR_KIND;
        }
        // Leaving the dwell costs one step, not three: 54 -> 53 on f52 and on
        // f152. Mirrored -- why the far turn is short is not decoded.
        // // UNKNOWN: see bd discr-0fm.
        _ if disc.world_z >= Z_FAR && disc.dir_kind < 0 => disc.world_z = Z_FAR - 1,
        next => disc.world_z = next,
    }
}

/// Spawn a disc into a slot. ST `$a9a0`, with `$a618` writing `dir_kind = +1`.
///
/// What triggers a serve is not decoded, so this is an explicit call with no
/// trigger of its own. `// UNKNOWN: see bd discr-m4x`. The caller sets
/// [`DiscSlot::damage`] (`+$16`): the observed tier-1 value is 3, but where
/// `$a9a0` gets it from is not recovered.
pub fn serve(
    disc: &mut DiscSlot,
    slot: usize,
    aim: PlayerId,
    world_x: i16,
    world_y: i16,
    events: &mut Vec<Event>,
) {
    *disc = DiscSlot {
        active: true,
        aim,
        world_x,
        world_y,
        world_z: 0,
        vel_x: 0,
        vel_y: 0,
        // $a618: move.w #$0001,($000a,a5)
        dir_kind: SERVE_DIR_KIND,
        damage: 0,
    };
    events.push(Event::DiscServed { slot });
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

    /// $6e44 / disc+$04: while world_z advances, world_x[n+1] - world_x[n]
    /// == vel_x[n+1] (47/48 in the notes). Away from the floor, that is all
    /// step() does to x.
    #[test]
    fn world_x_integrates_by_vel_x_while_z_advances() {
        // p2 at 152 -> aim 133, above the whole climb, so $a722 holds vel_x at
        // the +2 clamp and the integrate is all that moves x.
        let players = players_at(8, 152);
        let mut disc = DiscSlot {
            vel_x: 2,
            ..flying(10, aim_y())
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        // Z_NEAR -> Z_FAR is the whole outbound leg; the 55th frame dwells.
        for _ in 0..Z_FAR {
            let (prev_x, prev_z) = (disc.world_x, disc.world_z);
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc.world_z, prev_z + 1, "z advances by dir_kind (+1)");
            assert_eq!(disc.world_x - prev_x, disc.vel_x);
        }
        assert_eq!((disc.world_x, disc.world_z), (118, Z_FAR));
    }

    /// tile_damage.ndjson f34 -> f35 and f124 -> f125: on reaching Z_FAR with
    /// an outbound dir_kind the entire record freezes -- world_x too, even
    /// with vel_x = 1. Its duration is bd discr-0fm, so step() enters the
    /// dwell and never leaves.
    #[test]
    fn the_far_end_is_a_dwell_and_the_whole_record_holds() {
        let players = players_at(117, 63);
        // f33: on the aim point (63 - $13 = 44) at vel_x 2; the step into the
        // far end decays it to 1 and moves x to 45.
        let mut disc = DiscSlot {
            vel_x: 2,
            world_z: Z_FAR - 1,
            ..flying(44, 83)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        // f33 -> f34: the last outbound frame still moves x.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x, disc.world_z), (45, 1, Z_FAR));

        // f34 -> f35 and every frame after it: nothing at all.
        let dwelling = disc;
        for _ in 0..64 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc, dwelling, "the dwell holds the whole record");
        }
    }

    /// tile_damage.ndjson f52 -> f71: given a negative dir_kind the disc
    /// leaves the dwell by one step (54 -> 53), returns at -3 per frame, and
    /// clamps at Z_NEAR while turning outbound again. Disc 1 repeats the near
    /// turn at f208.
    #[test]
    fn the_return_leg_steps_three_and_the_near_bound_turns_it_outbound() {
        let players = players_at(117, 57);
        let mut disc = DiscSlot {
            world_z: Z_FAR,
            dir_kind: RETURN_DIR_KIND,
            ..flying(48, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        // f52: the far turn is a single step, not three.
        for expected_z in [
            53, 50, 47, 44, 41, 38, 35, 32, 29, 26, 23, 20, 17, 14, 11, 8, 5, 2,
        ] {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_z, disc.dir_kind), (expected_z, RETURN_DIR_KIND));
        }

        // f70: 2 - 3 would be -1, so it clamps and flips outbound.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_z, disc.dir_kind), (Z_NEAR, SERVE_DIR_KIND));

        // f71: and away it goes, +1 at a time.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!(disc.world_z, 1);
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

    /// tile_damage.ndjson f11 -> f34 with the $a816 aim, `$6d22 - $13` = 44:
    /// the disc climbs from the floor pinned at the +2 clamp, and on the frame
    /// it lands on the aim point $a728 decays vel_x 2 -> 1 -- before the
    /// integrate, so that frame still moves x by 1, to 45. 23 of 23 velocity
    /// transitions.
    #[test]
    fn step_steers_at_player_two_and_decays_on_the_aim_point() {
        let players = players_at(117, 63);
        let mut disc = DiscSlot {
            vel_x: 2,
            world_z: 31,
            ..flying(0, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        // f12..f33: aim above, vel_x already at the clamp, so +2 a frame.
        for n in 1..=22 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_x, disc.vel_x), (2 * n, 2), "{disc:?}");
        }
        assert_eq!((disc.world_x, disc.world_z), (44, 53));

        // f34: on the aim point. The decay lands on the same frame as the move.
        step(&mut disc, 0, &players, &mut tiles, &mut events);
        assert_eq!((disc.world_x, disc.vel_x, disc.world_z), (45, 1, Z_FAR));
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

    /// $a9a0 + $a618: the spawn store writes dir_kind = +1.
    #[test]
    fn serve_activates_the_slot_with_dir_kind_plus_one() {
        let mut disc = DiscSlot::default();
        let mut events = Vec::new();
        serve(&mut disc, 3, PlayerId::Two, 135, 0, &mut events);
        assert!(disc.active);
        assert_eq!(disc.dir_kind, 1);
        assert_eq!((disc.world_x, disc.world_y, disc.world_z), (135, 0, 0));
        assert_eq!(disc.aim, PlayerId::Two);
        assert_eq!(events, vec![Event::DiscServed { slot: 3 }]);
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
