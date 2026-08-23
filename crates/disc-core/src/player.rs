//! Player movement and state dispatch. Owned by bead `discr-...` (I2).
//!
//! ST `$f5d0` reads `player+$0e` (`$6cae`) and jumps through the 32-entry
//! table at `$10e2c`. Validated handlers: 1 = walk left (`$f5e2`), 2 = walk
//! right (`$f7f6`). Walking moves `player+$02` by +/-3 (`$f658` / `$f86c`),
//! range-checked against 8 and `$98`; `$f838` tests Y > 14 for the far row.
//!
//! Stub: does nothing. The signature below is the contract that `lib.rs` calls.

use crate::{Event, Input, Player, TILE_CELLS, Tile};

/// Advance one player by one frame.
///
/// `tiles` is read-only here: the movement code only `tst.w`s `tile+$00` as a
/// walkability gate. Only the disc loop writes tiles.
pub fn step(
    _player: &mut Player,
    _input: Input,
    _tiles: &[Tile; TILE_CELLS],
    _events: &mut Vec<Event>,
) {
}
