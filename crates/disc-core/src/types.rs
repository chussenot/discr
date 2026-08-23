//! Public state types, mirroring the Atari ST records field for field.
//!
//! Every field carries the ST address it mirrors so a trace comparison can be
//! read against `docs/state-schema.md`. All arithmetic in this crate is plain
//! integer arithmetic: no fixed point, no floats.

/// Number of disc records in the array at ST `$6e3e` (stride `$42`).
///
/// ST `$aa50`: the round initialiser stores 8 disc records.
pub const DISC_SLOTS: usize = 8;

/// Number of floor cells in the tile grid at ST `$7616` (stride 8).
pub const TILE_CELLS: usize = 17;

/// Lowest walkable player world X.
///
/// ST `$f658` / `$f86c`: walkable X is 8..152, range-checked against 8 and `$98`.
pub const WALK_X_MIN: i16 = 8;

/// Highest walkable player world X (`$98`).
pub const WALK_X_MAX: i16 = 152;

/// Player world X delta per walking frame.
///
/// ST `$f658`: `subq.w #3,$6ca2`; ST `$f86c`: `addq.w #3,$6ca2`.
pub const WALK_STEP: i16 = 3;

/// Player world Y above which the far row of the floor grid is selected.
///
/// ST `$f838`: `cmp.w #$000e,$6ca6` -- Y > 14 selects the far row.
pub const FAR_ROW_Y: i16 = 14;

/// Base added to the column index to form the grid cell index.
///
/// ST `$7616` note: cell = column(X) + 8 + (4 if Y > 14); observed range 9..16.
pub const GRID_CELL_BASE: u16 = 8;

/// Added to the grid cell index when the player stands on the far row.
pub const GRID_CELL_FAR_ROW: u16 = 4;

/// Inclusive clamp on a disc's steered velocity components.
///
/// ST `$a722`-`$a860`: `vel_x` is nudged by +/-1 per frame and clamped [-2,+2].
pub const VEL_CLAMP: i16 = 2;

/// Facing value for "left".
///
/// ST `$6ca9` (`player+$09`), set at `$f5e2`: 1 = left, 2 = right.
pub const FACING_LEFT: u8 = 1;

/// Facing value for "right". ST `$6ca9`, set at `$f7f6`.
pub const FACING_RIGHT: u8 = 2;

/// Tile type value meaning "destroyed"; such a cell is not walkable.
///
/// ST `$a354`: `clr.w ($00,a0,d5.w)` -- HP reaching 0 also clears the type word.
/// The movement code `tst.w`s this word as its walkability gate.
pub const TILE_TYPE_DESTROYED: u16 = 0;

/// Which of the two players something refers to.
///
/// ST: player 1 is the entity record at `$6ca0`, player 2 the one at `$6d20`
/// (same layout, stride `$80`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlayerId {
    /// ST `$6ca0`.
    #[default]
    One,
    /// ST `$6d20`.
    Two,
}

impl PlayerId {
    /// Index into [`crate::GameState::players`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            PlayerId::One => 0,
            PlayerId::Two => 1,
        }
    }
}

/// Decoded joystick direction bits.
///
/// ST `$6c58` `joystick_decoded` (byte), ORed: `$01` up, `$02` down, `$04`
/// left, `$08` right, `$80` fire. The fire bit is deliberately NOT part of this
/// type -- see [`Input::fire_edge`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirBits(pub u8);

impl DirBits {
    /// ST `$6c58` bit `$01`.
    pub const UP: DirBits = DirBits(0x01);
    /// ST `$6c58` bit `$02`.
    pub const DOWN: DirBits = DirBits(0x02);
    /// ST `$6c58` bit `$04`.
    pub const LEFT: DirBits = DirBits(0x04);
    /// ST `$6c58` bit `$08`.
    pub const RIGHT: DirBits = DirBits(0x08);
    /// No direction held.
    pub const NONE: DirBits = DirBits(0x00);

    /// True when every bit of `other` is set.
    #[must_use]
    pub const fn has(self, other: DirBits) -> bool {
        self.0 & other.0 == other.0
    }

    /// Union of two direction sets, as the ST code ORs them into `$6c58`.
    #[must_use]
    pub const fn or(self, other: DirBits) -> DirBits {
        DirBits(self.0 | other.0)
    }
}

/// One frame of input for one player.
///
/// ST `$6c58`: fire is bit `$80`, and it is *consumed*: `bclr #7,(a0)` at
/// `$f606` / `$f81a` / `$fb90` clears it on use. So fire is an EDGE, not a
/// level, and the caller is responsible for presenting it as one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Input {
    /// ST `$6c58` bits `$01`/`$02`/`$04`/`$08`.
    pub dir: DirBits,
    /// ST `$6c58` bit `$80`, as a one-frame edge (cleared by `bclr #7`).
    pub fire_edge: bool,
}

/// A player entity record.
///
/// ST `$6ca0` (player 1) / `$6d20` (player 2), stride `$80`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Player {
    /// ST `player+$02` (`$6ca2`): world X, walkable 8..152, +/-3 per frame.
    pub world_x: i16,
    /// ST `player+$06` (`$6ca6`): world Y (row); > 14 selects the far row.
    pub world_y: i16,
    /// ST `player+$09` (`$6ca9`): 1 = left, 2 = right.
    pub facing: u8,
    /// ST `player+$0e` (`$6cae`): index into the 32-entry jump table at `$10e2c`.
    pub state_index: u8,
    /// ST `player+$10` (`$6cb0`): grid cell index, observed 9..16.
    pub grid_cell: u16,
}

/// One of the 8 disc records.
///
/// ST `$6e3e`, 8 records, stride `$42`.
///
/// Screen X/Y (`+$0c` / `+$0e`) are deliberately absent: ST `$a6b2`/`$a6b6`
/// project them from world (x, y, z) through LUTs every frame, so they are
/// rendering output, not game state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiscSlot {
    /// Whether this slot is in play. Modelled, not an ST field: `+$0a` is a
    /// signed direction/kind word, not a live flag (Part 7), and the ST
    /// encoding of an unused slot is not yet known. Up to 3 of the 8 records
    /// were seen live at once.
    pub active: bool,
    /// Which player this disc is homing on.
    ///
    /// Modelled, not an ST field: Part 8 records that there is no possession --
    /// a disc is always in flight and always homing on a target player's
    /// coordinates -- and the target is implied by which steering routine runs
    /// (`$a71a` reads `$6ca2`, `$a7d8` reads `$6d22`).
    pub aim: PlayerId,
    /// ST `disc+$00` (`$6e3e`): world X, signed; spans 0..153.
    pub world_x: i16,
    /// ST `disc+$02`: world Y / height.
    pub world_y: i16,
    /// ST `disc+$04`: world Z (depth), +1 per frame while in flight.
    pub world_z: i16,
    /// ST `disc+$06`: X velocity, signed, steered +/-1 per frame toward
    /// `$6ca2`, clamped [-2,+2] at `$a722`.
    pub vel_x: i16,
    /// ST `disc+$08`: Y velocity, steered the same way toward `$6ca4`.
    pub vel_y: i16,
    /// ST `disc+$0a`: sign is the travel direction (flipped by `neg.w` at
    /// `$a606`, not by a comparison); magnitude is the kind of disc. Observed
    /// values +1, -1 and -3.
    pub dir_kind: i16,
    /// ST `disc+$16`: damage subtracted from a tile's HP on impact
    /// (`$a31c  sub.w ($0016,a5),d6`).
    pub damage: i16,
}

/// One floor grid cell.
///
/// ST `$7616`, 17 cells, stride 8.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tile {
    /// ST `tile+$00` (word), {0, 1, 2}. 0 = destroyed = unwalkable; the
    /// movement code `tst.w`s this as its walkability gate. Cleared by
    /// `$a354` when HP reaches 0.
    pub tile_type: u16,
    /// ST `tile+$02` (word): hit points. `$a31c` subtracts the striking disc's
    /// `+$16`; `$a34a  clr.w d6` clamps at 0, so it is never negative.
    pub hp: i16,
}

impl Tile {
    /// Whether a player may stand on this cell.
    ///
    /// ST: the movement code `tst.w`s `tile+$00`; 0 = destroyed = unwalkable.
    #[must_use]
    pub const fn walkable(self) -> bool {
        self.tile_type != TILE_TYPE_DESTROYED
    }
}

/// Something observable that happened during one [`crate::GameState::tick`].
///
/// Events exist so a trace comparison can align on discrete moments rather than
/// only on end-of-frame state. Every variant names the ST site that produces it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Event {
    /// A disc struck a cell and its HP was reduced but did not reach 0.
    ///
    /// ST `$a34c`: `move.w d6,($02,a0,d5.w)` -- the HP store.
    TileDamaged {
        /// Grid cell index into [`crate::GameState::tiles`].
        cell: usize,
        /// The HP value written, after the clamp at `$a34a`.
        hp: i16,
    },
    /// A cell's HP reached 0 and its type word was cleared.
    ///
    /// ST `$a354`: `clr.w ($00,a0,d5.w)`; `$a360` then queues the
    /// destruction sample.
    TileDestroyed {
        /// Grid cell index into [`crate::GameState::tiles`].
        cell: usize,
    },
    /// A disc was spawned into play.
    ///
    /// ST `$a9a0`: the spawn/serve routine stores the record; `$a9aa`
    /// (`addq.w #$01,$6d8a`) bumps the serve counter.
    DiscServed {
        /// Index into [`crate::GameState::discs`].
        slot: usize,
    },
    /// A disc turned around.
    ///
    /// ST `$a606`: `neg.w ($000a,a5)` -- the sign of `disc+$0a` is flipped.
    DiscReflected {
        /// Index into [`crate::GameState::discs`].
        slot: usize,
    },
}
