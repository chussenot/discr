//! A serviceable stand-in for player 2's AI rows 2-17.
//!
//! `disc_core::ai::Ai::p2_policy` implements exactly the two rows that are
//! provably RNG-independent (priority 50 escape, priority 30 avoid -- see
//! that module's own docs for why the other eighteen need the undecoded
//! `$6c5d` byte). When both are silent it returns 0, which on real hardware
//! can mean either "nothing to react to" or "one of the eighteen undecoded
//! rows fired" -- this crate cannot tell the two apart.
//!
//! `// app-level stand-in, NOT decoded behaviour: bd discr-rxx.3` ("Implement
//! AI rows 2-17 on the verified RNG"). This module is exactly that stand-in,
//! kept intentionally simple: walk toward the column under the most relevant
//! incoming disc, and serve when able. It is never consulted while
//! `disc_core::ai::Ai::p2_policy` has an opinion -- see `main.rs`'s call
//! site -- so it never overrides decoded behaviour, only fills the silence.

use disc_core::{DISC_SLOTS, DirBits, DiscSlot, Input, Player, WALK_X_MAX, WALK_X_MIN};

/// Within this many world-X units of the target column, stop walking. Mirrors
/// the deadzone shape `disc_core::ai::Ai::step` uses for its own arrival
/// check (`[-4,+4]`), not a measured value for this fallback.
const COLUMN_DEADZONE: i16 = 4;

/// Which incoming disc the fallback should chase: the live, simulated disc
/// with the greatest `world_z` -- i.e. the one furthest along its flight and
/// so the most urgent to be under when it arrives. `None` when nothing is in
/// play.
#[must_use]
pub fn target_column(discs: &[DiscSlot; DISC_SLOTS]) -> Option<i16> {
    discs
        .iter()
        .filter(|d| d.simulated())
        .max_by_key(|d| d.world_z)
        .map(|d| d.world_x)
}

/// Walk toward `target`, clamped to the walkable range, with a deadzone so
/// the AI does not jitter once it arrives. `None` produces no input.
#[must_use]
pub fn fallback_dir(p2: &Player, target: Option<i16>) -> DirBits {
    let Some(target) = target else {
        return DirBits::NONE;
    };
    let target = target.clamp(WALK_X_MIN, WALK_X_MAX);
    if target > p2.world_x + COLUMN_DEADZONE {
        DirBits::RIGHT
    } else if target < p2.world_x - COLUMN_DEADZONE {
        DirBits::LEFT
    } else {
        DirBits::NONE
    }
}

/// Whether the fallback should hold fire to attempt a serve: simply "can it
/// -- is it under its own cap". `disc_core`'s own throw-state gate
/// (`disc::THROW_STATES`' exact `anim_cursor` match) is what actually decides
/// whether a held fire becomes a served disc on any given tick; this is only
/// the AI's intent.
#[must_use]
pub fn should_serve(p2: &Player) -> bool {
    p2.discs_out < p2.disc_cap
}

/// The full fallback decision as an [`Input`]: walk toward the incoming
/// disc's column. Never sets fire: `disc_core::player::idle`'s own dispatch
/// checks `fire_held && who == PlayerId::Two` BEFORE its walk-direction
/// branches, so a fire held every tick a serve is merely POSSIBLE (i.e. most
/// of the time) would keep player 2 out of the ordinary idle/walk path --
/// and, with it, out of `idle_tick`'s call to `anim_tick`, the thing that
/// actually populates `Player::hit_box` from the animation tables. A player 2
/// that never walks and never gets a real hit box can neither chase a disc
/// nor ever register a catch. [`crate::MatchState::serve_workaround`] reads
/// [`should_serve`] on its own, decoupled from this `Input`, for exactly this
/// reason -- see its own doc.
#[must_use]
pub fn fallback_input(p2: &Player, discs: &[DiscSlot; DISC_SLOTS]) -> Input {
    Input {
        dir: fallback_dir(p2, target_column(discs)),
        fire_edge: false,
        fire_held: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc_at(world_x: i16, world_z: i16) -> DiscSlot {
        DiscSlot {
            active: 0xff,
            world_x,
            world_z,
            ..DiscSlot::default()
        }
    }

    fn p2_at(world_x: i16) -> Player {
        Player {
            world_x,
            ..Player::default()
        }
    }

    #[test]
    fn no_discs_means_no_target() {
        let discs: [DiscSlot; DISC_SLOTS] = Default::default();
        assert_eq!(target_column(&discs), None);
        assert_eq!(fallback_dir(&p2_at(80), None), DirBits::NONE);
    }

    #[test]
    fn picks_the_disc_furthest_along_its_flight() {
        let mut discs: [DiscSlot; DISC_SLOTS] = Default::default();
        discs[0] = disc_at(20, 10);
        discs[1] = disc_at(140, 60);
        // Not simulated -- must be ignored even though it is "furthest".
        discs[2] = DiscSlot {
            world_x: 999,
            world_z: 999,
            ..DiscSlot::default()
        };
        assert_eq!(target_column(&discs), Some(140));
    }

    #[test]
    fn walks_toward_the_target_column_and_stops_in_the_deadzone() {
        let p2 = p2_at(50);
        assert_eq!(fallback_dir(&p2, Some(100)), DirBits::RIGHT);
        assert_eq!(fallback_dir(&p2, Some(0)), DirBits::LEFT);
        assert_eq!(fallback_dir(&p2, Some(51)), DirBits::NONE);
        assert_eq!(fallback_dir(&p2, Some(50 - COLUMN_DEADZONE)), DirBits::NONE);
    }

    #[test]
    fn target_is_clamped_to_the_walkable_range() {
        let p2 = p2_at(WALK_X_MAX);
        assert_eq!(fallback_dir(&p2, Some(9999)), DirBits::NONE);
    }

    #[test]
    fn serves_while_under_its_disc_cap() {
        let mut p2 = Player {
            discs_out: 1,
            disc_cap: 4,
            ..Player::default()
        };
        assert!(should_serve(&p2));
        p2.discs_out = 4;
        assert!(!should_serve(&p2));
    }

    #[test]
    fn fallback_input_walks_but_never_holds_fire() {
        // Fire is deliberately never set here -- see `fallback_input`'s own
        // doc for why holding it would break player 2's ordinary movement.
        let p2 = Player {
            world_x: 20,
            discs_out: 0,
            disc_cap: 4,
            ..Player::default()
        };
        let mut discs: [DiscSlot; DISC_SLOTS] = Default::default();
        discs[0] = disc_at(140, 10);
        let input = fallback_input(&p2, &discs);
        assert_eq!(input.dir, DirBits::RIGHT);
        assert!(!input.fire_edge);
        assert!(!input.fire_held);
    }
}
