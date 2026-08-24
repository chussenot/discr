//! `disc-core` -- the game rules of Disc (Loriciel, 1990, Atari ST),
//! re-implemented from the evidence in `docs/disc-notes.md`.
//!
//! # Contract
//!
//! * State mirrors the ST records field for field, so a trace dumped from
//!   Hatari or the oracle compares directly. The authoritative field-by-field
//!   mapping is `docs/state-schema.md`.
//! * Integer arithmetic only. No fixed point, no floats, anywhere.
//! * No dependencies. `serde` is optional and exists only so the tracecheck
//!   tool can serialize these types.
//!
//! # Frame order
//!
//! [`GameState::tick`] runs one PAL VBL, in the order the ST does:
//!
//! 1. `$8198` -- the VBL handler's first instruction is `addq.w #1,$6ab4`,
//!    so the frame counter advances first.
//! 2. `$f5d0` -- the player state dispatch, once per player.
//! 3. `$a4ea` -- the disc update loop (`lea $6e3e,a5`), once per slot. Tile
//!    damage happens *inside* this loop at `$a31c`-`$a360`, so [`tile::damage`]
//!    is called from [`disc::step`] rather than from `tick`. There is no
//!    per-frame tile pass on the ST: cells change only when a disc hits one.

#![forbid(unsafe_code)]

pub mod disc;
pub mod player;
pub mod tile;

mod types;

pub use types::{
    COLUMN_TABLE_LEN, COLUMN_WIDTH, DISC_SLOTS, DirBits, DiscSlot, Event, FACING_LEFT,
    FACING_RIGHT, FAR_ROW_Y, GRID_CELL_BASE, GRID_CELL_FAR_ROW, Input, Player, PlayerId, SteerHook,
    TILE_CELLS, TILE_TYPE_DESTROYED, Tile, VEL_CLAMP, WALK_STEP, WALK_X_MAX, WALK_X_MIN,
};

/// The complete simulated state of one match.
///
/// `Default` is all zeroes. It is NOT the ST's round-init state: `$aa50`
/// initialises the 8 disc records and their sub-records with values this crate
/// does not yet model (see `docs/state-schema.md`, waived under `discr-st8`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameState {
    /// ST `$6ca0` and `$6d20` (stride `$80`).
    pub players: [Player; 2],
    /// ST `$6e3e`, 8 records of stride `$42`.
    pub discs: [DiscSlot; DISC_SLOTS],
    /// ST `$7616`, 17 cells of stride 8.
    pub tiles: [Tile; TILE_CELLS],
    /// ST `$6ab4` `vbl_frame_counter`, which is a *word* and wraps. Held here
    /// as a `u32` so the simulation has an unambiguous frame number; compare
    /// against the ST word as `frame as u16`.
    pub frame: u32,
}

impl GameState {
    /// Run one PAL VBL and return everything observable that happened.
    ///
    /// `inputs` is indexed by [`PlayerId::index`]. See the module docs for the
    /// frame order and the ST sites it mirrors.
    pub fn tick(&mut self, inputs: [Input; 2]) -> Vec<Event> {
        let mut events = Vec::new();

        // ST $8198: addq.w #1,$6ab4 is the VBL handler's first instruction.
        self.frame = self.frame.wrapping_add(1);

        // ST $a4ea: the disc update loop walks all 8 records.
        for slot in 0..DISC_SLOTS {
            disc::step(
                &mut self.discs[slot],
                slot,
                &mut self.players,
                &mut self.tiles,
                &mut events,
            );
        }

        // ST $f5d0: the player state dispatch, per player.
        for (i, p) in self.players.iter_mut().enumerate() {
            player::step(p, inputs[i], &self.tiles, &mut events);
        }

        // ST $c068 / $c0fe, player 2's throw states 15 and 16. The release is
        // gated on the animation cursor reaching one exact value, which is why
        // Player::anim_cursor is carried as a raw ST pointer: it is a fed input
        // (the animation engine is not modelled), and it is the ST's own gate
        // rather than a synthesised trigger.
        //
        // Only PLAYER 2 throws here. Player 1's control routine $f104 has its
        // own $a972 call sites and its own parameter builds, none decoded.
        // // UNKNOWN: see bd discr-b6x.
        if let Some(&(_, _, x_offset)) = disc::THROW_STATES.iter().find(|&&(state, gate, _)| {
            self.players[1].state_index == state && self.players[1].anim_cursor == gate
        }) {
            let thrower = self.players[1];
            if disc::serve(&mut self.discs, &thrower, inputs[1], x_offset, &mut events).is_some() {
                // $c0c4 / $c158: the thrower goes to state 17 on this frame.
                self.players[1].state_index = disc::STATE_AFTER_THROW;
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joystick_bits_match_st_6c58() {
        // $6c58: $01 up $02 down $04 left $08 right, ORed.
        assert_eq!(DirBits::UP.0 | DirBits::RIGHT.0, 0x09);
        let d = DirBits::UP.or(DirBits::RIGHT);
        assert!(d.has(DirBits::UP) && d.has(DirBits::RIGHT));
        assert!(!d.has(DirBits::DOWN) && !d.has(DirBits::LEFT));
    }

    #[test]
    fn tick_advances_the_frame_counter_by_one() {
        let mut st = GameState::default();
        assert!(st.tick([Input::default(); 2]).is_empty());
        assert_eq!(st.frame, 1);
        st.tick([Input::default(); 2]);
        assert_eq!(st.frame, 2);
    }

    #[test]
    fn tile_type_zero_is_unwalkable() {
        // $a354 clears the type word when HP hits 0; the movement code tst.w's it.
        assert!(
            !Tile {
                tile_type: 0,
                hp: 0
            }
            .walkable()
        );
        assert!(
            Tile {
                tile_type: 1,
                hp: 4
            }
            .walkable()
        );
        assert!(
            Tile {
                tile_type: 2,
                hp: 4
            }
            .walkable()
        );
    }

    #[test]
    fn record_shapes_match_the_st_arrays() {
        let st = GameState::default();
        assert_eq!(st.discs.len(), 8); // $6e3e, 8 x $42
        assert_eq!(st.tiles.len(), 17); // $7616, 17 x 8
        assert_eq!(st.players.len(), 2); // $6ca0 / $6d20
    }
}
