//! Floor grid damage and destruction. Owned by bead `discr-...` (I4).
//!
//! ST `$7616`, 17 cells of stride 8, `{+$00 type, +$02 hp}`.
//!
//! ```text
//! $a31c  sub.w  ($0016,a5),d6      ; HP -= the striking disc's damage (+$16)
//! $a34a  clr.w  d6                 ; clamped at 0, never negative
//! $a34c  move.w d6,($02,a0,d5.w)   ; the HP store
//! $a354  clr.w  ($00,a0,d5.w)      ; HP == 0 also clears the TYPE word
//! $a360  move.b #$03,$6c5c         ; and queues the destruction sample
//! ```
//!
//! Stub: does nothing. The signature below is the contract that
//! [`crate::disc::step`] calls.

use crate::{Event, TILE_CELLS, Tile};

/// Apply `damage` to one cell, clamping HP at 0 and destroying the cell when
/// it reaches 0.
///
/// Pushes [`Event::TileDamaged`] for a surviving cell, or
/// [`Event::TileDestroyed`] for a killing hit.
pub fn damage(
    _tiles: &mut [Tile; TILE_CELLS],
    _cell: usize,
    _damage: i16,
    _events: &mut Vec<Event>,
) {
}
