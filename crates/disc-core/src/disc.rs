//! Disc flight, steering and impact. Owned by bead `discr-leu` (I3).
//!
//! ST `$a4ea` is the update loop entry (`lea $6e3e,a5`): 8 records of stride
//! `$42`. Per slot the ST *can* nudge `vel_x` by +/-1 toward the aimed player,
//! clamped `[-2,+2]` (`$a722`), integrates `world_x` by `+$06` (`$6e44`) and
//! advances `world_z`, flips `+$0a` with `neg.w` on turn-around (`$a606`), and
//! on a tile hit subtracts `+$16` from the struck cell's HP (`$a31c`).
//!
//! "Can": the `$a71a`/`$a722` steering block is **gated off in every trace we
//! have**, so [`step`] does not call [`steer`]. What [`step`] models instead --
//! integrate, floor at 0, sign-flip on the floor -- is read off
//! `tests/fixtures/golden.ndjson`, and the floor's coupling to the flip is
//! **modelled, not mirrored**. See [`step`] and bd discr-217.
//!
//! There is no possession: a disc is always in flight and always homing on a
//! target player, so the target lives in [`crate::DiscSlot::aim`].
//!
//! # What this module does NOT decide
//!
//! Two triggers inside the ST loop are not decoded, so they are exposed as
//! explicit API calls rather than invented here:
//!
//! * horizontal steering -- [`steer`] is the literal `$a722` rule and it is
//!   right, but nothing in evidence runs it: in the golden fixture disc 0
//!   falls `world_x` 21 -> 0 with `vel_x` pinned at -2 for eleven frames while
//!   the aim point (98) sits above it the whole way, so the rule would have
//!   incremented `vel_x` on every one of them. [`step`] therefore never calls
//!   it. What gates `$a71a` is `// UNKNOWN: see bd discr-217`.
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

/// Disc `world_y` at round init.
///
/// ST record layout: `disc+$02` is `$52` after `$aa50`. Offered as a default
/// for [`serve`]'s `world_y`; the rest of the `$a9a0` field table is not
/// recovered (`// UNKNOWN: see bd discr-st8`).
pub const SERVE_WORLD_Y: i16 = 0x52;

/// The disc's X aim point for the player it is homing on.
///
/// ST `$a71a` reads `$6ca2 - $13` for player 1. Player 2 has *two* observed
/// variants -- `$a7d8` (`$6d22 - 4`) and `$a816` (`$6d22 - $13`) -- and which
/// one a given disc uses is selected by something that is not decoded, so this
/// uses the `$a816` form for both players.
/// `// UNKNOWN: see bd discr-b6x`.
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
/// **Not called by [`step`].** The rule is known exactly; its gate is not, and
/// in every trace we have the gate is off (bd discr-217). It stays public
/// because it is the decoded rule and the day `$a71a`'s condition is recovered
/// this is what goes behind it.
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
/// Three things happen, and only these three:
///
/// * `world_x += vel_x` -- the invariant the notes verified 47/48 frames while
///   `world_z` advances, with `world_x` frozen whenever `world_z` is (17/17);
/// * a **floor at `world_x == 0`** -- golden frame 10 -> 11 steps 1 -> 0, a
///   move of -1 under `vel_x` = -2, so the position clamps rather than passing
///   zero;
/// * `vel_x` **sign-flips on that clamp**, -2 -> +2 on the same frame.
///
/// `// MODELLED, not mirrored: trigger inferred from the fixture; ST guard
/// $a600 bpl undecoded -- see bd discr-217`. The `neg.w` at `$a606` is real;
/// what reaches it is a `bpl` on `d2` whose meaning is not decoded, so the
/// floor-to-flip coupling is an inference from the trace, not a transcription.
///
/// There is deliberately **no ceiling**. The fixture's two upper turnarounds
/// (`world_x` 45 and 113) are not clamps: both decay through +1 and 0 before
/// reversing, and they sit at different values, so nothing in evidence
/// supports a symmetric upper bound. The asymmetry is the evidence's, not a
/// bug.
///
/// [`steer`] is **not** called -- see the module docs and bd discr-217.
///
/// `tiles` is mutable because tile damage happens inside this loop on the ST
/// (`$a31c`-`$a360`), but the collision test that selects the struck cell is
/// not decoded, so nothing here writes it -- see [`impact`].
/// `// UNKNOWN: see bd discr-5w5`.
///
/// `players` is unused because the only thing that read it was the steering
/// call. It stays in the signature because the aim point is what goes back in
/// the day discr-217 recovers the gate.
pub fn step(
    disc: &mut DiscSlot,
    _slot: usize,
    _players: &[Player; 2],
    _tiles: &mut [Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
    if !disc.active {
        return;
    }

    // No steer() call here. $a722's rule is decoded, its gate is not, and in
    // every trace we have it is off -- calling it diverges on golden frame 1.
    // // UNKNOWN: see bd discr-217.

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

    // disc+$04: world Z advances +1 per frame while in flight.
    disc.world_z = disc.world_z.saturating_add(1);
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
        let players = players_at(152, 8);
        let mut disc = DiscSlot {
            vel_x: 2,
            ..flying(10, aim_y())
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for _ in 0..80 {
            let (prev_x, prev_z) = (disc.world_x, disc.world_z);
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc.world_z, prev_z + 1, "z advances +1 per frame");
            assert_eq!(disc.world_x - prev_x, disc.vel_x);
        }
        assert_eq!(disc.world_x, 170);
    }

    /// discr-217: the $a722 block is gated off in every trace we have, so
    /// step() must leave vel_x alone however far the aim point is. The
    /// fixture's first eleven frames are the witness: aim 98, disc falling
    /// from 21, vel_x pinned at -2.
    #[test]
    fn step_does_not_steer() {
        // $6ca2 = 117 -> aim 98, well above the disc the whole way down.
        let players = players_at(117, 8);
        assert_eq!(aim_x(&players, PlayerId::One), 98);

        let mut disc = DiscSlot {
            vel_x: -2,
            ..flying(21, 81)
        };
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for expected_x in [19, 17, 15, 13, 11, 9, 7, 5, 3, 1] {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!((disc.world_x, disc.vel_x), (expected_x, -2), "{disc:?}");
        }
    }

    /// tests/fixtures/golden.ndjson frames 10 -> 12: world_x 1 -> 0 -> 2 with
    /// vel_x -2 -> +2. The step to 0 is only -1, so the position clamps, and
    /// the velocity flips on the same frame.
    ///
    /// MODELLED, not mirrored: the trigger is inferred from the fixture; the
    /// ST guard $a600 bpl on d2 is undecoded -- see bd discr-217.
    #[test]
    fn world_x_clamps_at_zero_and_flips_the_velocity() {
        let players = players_at(117, 8);
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

    /// The floor has no mirror: nothing in the evidence bounds world_x from
    /// above, so step() lets it run past the fixture's turnaround values.
    #[test]
    fn there_is_no_ceiling() {
        let players = players_at(117, 8);
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
