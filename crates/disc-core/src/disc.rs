//! Disc flight, steering and impact. Owned by bead `discr-leu` (I3).
//!
//! ST `$a4ea` is the update loop entry (`lea $6e3e,a5`): 8 records of stride
//! `$42`. Per slot the ST steers `vel_x`/`vel_y` by +/-1 toward the aimed
//! player, clamped `[-2,+2]` (`$a722`-`$a860`), integrates `world_x` by `+$06`
//! (`$6e44`) and advances `world_z`, flips `+$0a` with `neg.w` on turn-around
//! (`$a606`), and on a tile hit subtracts `+$16` from the struck cell's HP
//! (`$a31c`).
//!
//! There is no possession: a disc is always in flight and always homing on a
//! target player, so the target lives in [`crate::DiscSlot::aim`].
//!
//! # What this module does NOT decide
//!
//! Two triggers inside the ST loop are not decoded, so they are exposed as
//! explicit API calls rather than invented here:
//!
//! * [`serve`] -- `$a9a0` stores the spawn record and `$a618` writes
//!   `dir_kind = +1`, but nothing says what *causes* a serve.
//!   `// UNKNOWN: see bd discr-m4x`.
//! * [`reflect`] and [`impact`] -- `$a606` negates `+$0a` when the disc "turns
//!   around" and `$a31c` damages cell `($00,a0,d5.w)`, but neither the
//!   turn-around condition nor the computation of `d5` is decoded, so [`step`]
//!   never touches `tiles` and never reflects on its own.
//!   `// UNKNOWN: see bd discr-5w5`.
//!
//! Screen X/Y (`+$0c` / `+$0e`) are projection, recomputed every frame at
//! `$a6b2`/`$a6b6` from world (x, y, z) through LUTs. They are not state and do
//! not appear here.

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
/// 83, and the observed disc `world_y` converges 81 -> 82 -> 83.
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
#[must_use]
pub const fn aim_y() -> i16 {
    PLAYER_HEIGHT_REF - AIM_Y_OFFSET
}

/// One frame of velocity steering: nudge `vel` by +/-1 toward `target`.
///
/// ST `$a722`-`$a860`: `+/-1` per frame, clamped `[-2,+2]` (`cmp.w #$fffe` /
/// `cmp.w #$0002`). There is no angle table.
///
/// The result is additionally limited to the remaining gap so the disc cannot
/// step past its aim point. That limit is what reproduces the observed
/// `world_y` convergence 81 -> 82 -> 83 (Part 9); a bare `[-2,+2]` clamp
/// reaches 82 and then overshoots to 84 and oscillates. Which ST instructions
/// produce the damping is not identified.
/// `// UNKNOWN: see bd discr-g38`.
fn steer(vel: i16, pos: i16, target: i16) -> i16 {
    let gap = target - pos;
    let nudged = match gap.signum() {
        1 => vel.saturating_add(1),
        -1 => vel.saturating_sub(1),
        _ => 0,
    };
    nudged
        .clamp(-VEL_CLAMP, VEL_CLAMP)
        .clamp(gap.min(0), gap.max(0))
}

/// Advance one disc slot by one frame. ST `$a4ea`, one iteration.
///
/// Order matches the ST: steer first, then integrate, so the invariant the
/// notes verified (47/48 frames) holds -- while `world_z` advances,
/// `world_x[n+1] - world_x[n] == vel_x[n+1]`; when `world_z` is frozen so is
/// `world_x` (17/17).
///
/// `tiles` is mutable because tile damage happens inside this loop on the ST
/// (`$a31c`-`$a360`), but the collision test that selects the struck cell is
/// not decoded, so nothing here writes it -- see [`impact`].
/// `// UNKNOWN: see bd discr-5w5`.
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

    // $a722-$a860: steer both components toward the aimed player.
    disc.vel_x = steer(disc.vel_x, disc.world_x, aim_x(players, disc.aim));
    disc.vel_y = steer(disc.vel_y, disc.world_y, aim_y());

    // $6e44: world X integrates by vel_x. world_y follows vel_y the same way.
    disc.world_x = disc.world_x.saturating_add(disc.vel_x);
    disc.world_y = disc.world_y.saturating_add(disc.vel_y);

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
    /// == vel_x[n+1] (47/48 in the notes).
    #[test]
    fn world_x_integrates_by_vel_x_while_z_advances() {
        let players = players_at(152, 8);
        let mut disc = flying(0, aim_y());
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for _ in 0..80 {
            let (prev_x, prev_z) = (disc.world_x, disc.world_z);
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            assert_eq!(disc.world_z, prev_z + 1, "z advances +1 per frame");
            assert_eq!(disc.world_x - prev_x, disc.vel_x);
        }
        assert_eq!(disc.world_x, aim_x(&players, PlayerId::One));
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

    /// $a722-$a860: cmp.w #$fffe / cmp.w #$0002.
    #[test]
    fn steered_velocity_stays_within_the_clamp() {
        let players = players_at(152, 8);
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        for (start, aim) in [(0_i16, PlayerId::One), (153, PlayerId::Two)] {
            let mut disc = DiscSlot {
                aim,
                ..flying(start, aim_y())
            };
            for _ in 0..96 {
                step(&mut disc, 0, &players, &mut tiles, &mut events);
                assert!((-VEL_CLAMP..=VEL_CLAMP).contains(&disc.vel_x), "{disc:?}");
                assert!((-VEL_CLAMP..=VEL_CLAMP).contains(&disc.vel_y), "{disc:?}");
            }
        }
    }

    /// Part 9: vel_y homes on $6ca4 - $10 = 83, NOT on the player's Y at
    /// +$06, and world_y converges 81 -> 82 -> 83.
    #[test]
    fn vel_y_homes_on_the_height_reference() {
        assert_eq!(aim_y(), 83);
        let players = [
            Player {
                world_x: 117,
                world_y: 18, // $6ca6: the walkable row -- must NOT be the target
                ..Player::default()
            },
            Player::default(),
        ];
        let mut disc = flying(aim_x(&players, PlayerId::One), 81);
        let mut tiles = [Tile::default(); TILE_CELLS];
        let mut events = Vec::new();

        let mut ys = Vec::new();
        for _ in 0..4 {
            step(&mut disc, 0, &players, &mut tiles, &mut events);
            ys.push(disc.world_y);
        }
        assert_eq!(ys, vec![82, 83, 83, 83]);
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
