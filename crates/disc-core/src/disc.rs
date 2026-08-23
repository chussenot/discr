//! Disc flight, steering and impact. Owned by bead `discr-...` (I3).
//!
//! ST `$a4ea` is the update loop entry (`lea $6e3e,a5`). Per slot it integrates
//! world X by `+$06` and advances `world_z`, steers `vel_x`/`vel_y` by +/-1
//! toward the aimed player's coordinates clamped to [-2,+2] (`$a722`-`$a860`),
//! flips `+$0a` with `neg.w` on turn-around (`$a606`), and on a tile hit
//! subtracts `+$16` from the cell's HP (`$a31c`).
//!
//! There is no possession: a disc is always in flight and always homing on a
//! target player, so the target lives in [`crate::DiscSlot::aim`].
//!
//! Stub: does nothing. The signature below is the contract that `lib.rs` calls.

use crate::{DiscSlot, Event, Player, TILE_CELLS, Tile};

/// Advance one disc slot by one frame.
///
/// `tiles` is mutable because tile damage happens inside this loop on the ST;
/// apply it through [`crate::tile::damage`] rather than writing cells directly.
pub fn step(
    _disc: &mut DiscSlot,
    _slot: usize,
    _players: &[Player; 2],
    _tiles: &mut [Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
}
