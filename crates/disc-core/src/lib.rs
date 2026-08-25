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

pub use tile::Collapse;
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
    /// ST `$7616`, 16 cells of stride 8 (the near bank -- discr-ovl.5).
    /// Cells 1..8 are the records a disc's damage path writes and 9..16 the
    /// copies the movement code reads -- the same eight tiles twice, see
    /// [`tile`]. Index 16 is one past the bank (`$7696`, never a tile).
    pub tiles: [Tile; TILE_CELLS],
    /// ST `$7596`, the other 16-cell bank: player 2's floor, the one `$9f5e`
    /// damages and `$cc0a` tests for walkability. Held separately because
    /// `tiles` mirrors `$7616` and the two are independent boards.
    pub tiles_far: [Tile; TILE_CELLS],
    /// ST `$779e`: the one tile collapse the game can have in flight.
    pub collapse: Option<tile::Collapse>,
    /// **Retired in Part 11g**: the number of passes is `passes.len()`, and each
    /// pass carries its own input, because `$10ec6 bsr $d2cc` rewrites `$6da1`
    /// inside the repeat loop. Kept only as documentation of what a frame is.
    ///
    /// The main loop at `$96ba` pushes it and loops:
    ///
    /// ```text
    /// $96ba  move.w $6ab8,-(a7)     ; the repeat count
    /// $96be  bsr $a4ea              ; the disc loop
    /// $96c2  bsr $10eac             ; the player control dispatcher
    /// $96c6  bsr $9c52
    /// $96ca  subq.w #1,(a7)
    /// $96cc  bpl $96be              ; again while it is still >= 0
    /// ```
    ///
    /// so one pass of that loop is `$6ab8 + 1` updates -- but `$96ba` sits in the
    /// **main loop, not in the VBL handler**, and the sampling point is the VBL.
    /// Between two samples the main loop may therefore complete **0, 1 or 2**
    /// passes depending on what else the frame had to do. Measured:
    ///
    /// ```text
    /// golden       1 pass on every one of its 99 ticks
    /// tile_damage  1 pass on every one of its 214
    /// p1_walk      1 on 200 ticks, 2 on 37, and 0 on 37
    /// ```
    ///
    /// "One tick is one update" was a model of the *sampling*, not of the game,
    /// and it survived eleven parts because both clean fixtures happen to run
    /// exactly one pass per frame.
    ///
    /// What paces the main loop is not modelled.
    /// `// UNKNOWN: see bd discr-ovl.7`.
    pub updates: u16,
    /// ST `$6ab4` `vbl_frame_counter`, which is a *word* and wraps. Held here
    /// as a `u32` so the simulation has an unambiguous frame number; compare
    /// against the ST word as `frame as u16`.
    pub frame: u32,
}

impl GameState {
    /// One pass of `$96be`-`$96c6`: the disc loop, the player dispatcher and
    /// player 2's throw. Run `$6ab8 + 1` times per frame by [`Self::tick`].
    fn update(&mut self, inputs: [Input; 2], events: &mut Vec<Event>) {
        // ST $a4ea: the disc update loop walks all 8 records.
        for slot in 0..DISC_SLOTS {
            disc::step(
                &mut self.discs[slot],
                slot,
                &mut self.players,
                &mut self.tiles,
                &mut self.collapse,
                &mut self.tiles_far,
                events,
            );
        }

        // ST $f5d0: the player state dispatch, per player.
        // A player's "own bank" is the one their movement code indexes: $7616
        // for player 1, $7596 for player 2. `tiles` is the walkability gate for
        // both, because only player 1's movement reads it.
        let (near, far) = (self.tiles, self.tiles_far);
        for (i, (p, input)) in self.players.iter_mut().zip(inputs).enumerate() {
            let (who, own) = if i == 0 {
                (PlayerId::One, &near)
            } else {
                (PlayerId::Two, &far)
            };
            player::step(p, who, input, own, events);
        }

        // ST $f1b4: `st $6d2d` -- entering the death state sets the flag on the
        // OTHER player, and the disc loop reads it to clear the board.
        for i in 0..2 {
            if self.players[i].state_index == player::STATE_DEAD {
                self.players[1 - i].round_over = true;
            }
        }

        // ST $012582: the render pass counts every retired slot down. After the
        // disc loop, so a disc caught this tick already reads one lower.
        disc::retire_tick(&mut self.discs);

        // ST $c068 / $c0fe, player 2's throw states 15 and 16. The release is
        // gated on the animation cursor reaching one exact value, which is why
        // Player::anim_cursor is carried as a raw ST pointer: it is a fed input
        // (the animation engine is not modelled), and it is the ST's own gate
        // rather than a synthesised trigger.
        //
        // Only PLAYER 2 throws here. Player 1's control routine $f104 has its
        // own $a972 call sites and its own parameter builds, none decoded.
        // // UNKNOWN: see bd discr-b6x.
        if let Some(&(_, _, x_offset, step)) =
            disc::THROW_STATES.iter().find(|&&(state, gate, _, _)| {
                self.players[1].state_index == state && self.players[1].anim_cursor == gate
            })
        {
            let thrower = self.players[1];
            if disc::serve(&mut self.discs, &thrower, inputs[1], x_offset, step, events).is_some() {
                // $c0c4 / $c158: the thrower goes to state 17 on this frame.
                self.players[1].state_index = disc::STATE_AFTER_THROW;
                // $a9aa: addq.w #$01,$6d8a.
                self.players[1].discs_out += 1;
            }
        }
    }

    /// Run one PAL VBL and return everything observable that happened.
    ///
    /// `inputs` is indexed by [`PlayerId::index`]. See the module docs for the
    /// frame order and the ST sites it mirrors.
    pub fn tick(&mut self, inputs: [Input; 2]) -> Vec<Event> {
        self.tick_passes(&[inputs])
    }

    /// Run one PAL VBL that contained `passes.len()` update passes.
    ///
    /// A frame is **not** one update: `$96ba`-`$96cc` sits in the main loop, the
    /// sampling point is the VBL, and between two samples the loop completes
    /// however many passes it got round to -- 0, 1 or 2 in the traces we have.
    ///
    /// Each pass carries **its own inputs**, because `$10ec6 bsr $d2cc` rewrites
    /// `$6da1` and `$10ece bsr $abb2` consumes it, both inside that loop. With
    /// two passes in one frame there are two different AI bytes, and a trace that
    /// samples once per frame only shows the last: `p1_walk` frame 224 used `$08`
    /// then `$00`, and driving both passes from `$00` loses the walk step the
    /// first one made.
    ///
    /// `tick` is the one-pass case.
    pub fn tick_passes(&mut self, passes: &[[Input; 2]]) -> Vec<Event> {
        let mut events = Vec::new();

        // ST $8198: addq.w #1,$6ab4 is the VBL handler's first instruction.
        self.frame = self.frame.wrapping_add(1);

        // ST $96be-$96c6, once per pass.
        for inputs in passes {
            self.update(*inputs, &mut events);
        }

        // ST $14ba4: the tile-collapse effect, LAST -- it lives in the render
        // pass, not the game update. Ordering it first (as Part 10e did) makes
        // the 49-tick delay come out right and then destroys the cell a player
        // is about to walk onto in the same tick, which is what p1_walk frame
        // 143 catches: player 2 walks off cell 15 on the frame the collapse
        // clears it, and the ST lets it.
        //
        // The delay still comes out right because collapse_step's own first
        // decrement now lands on the claiming tick instead of the one after.
        tile::collapse_step(&mut self.collapse, &mut self.tiles, &mut events);

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
        assert_eq!(st.tiles.len(), 16); // $7616, one 16 x 8 bank (discr-ovl.5)
        assert_eq!(st.tiles_far.len(), 16); // $7596, the other bank
        assert_eq!(st.players.len(), 2); // $6ca0 / $6d20
    }
}
