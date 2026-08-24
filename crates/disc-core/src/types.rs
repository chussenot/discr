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
    /// The same bit as a **level**, which some handlers want instead: `$c1b4
    /// btst #$7,(a0)` in state 18 commits to a throw only while fire is still
    /// held. The two differ because `$f606`/`$f81a` consume the bit inside the
    /// walk handlers, so an edge is what those see and a level is what a state
    /// reading the byte directly sees.
    pub fire_held: bool,
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
    /// ST `player+$09` (`$6ca9`): **the state whose handler last ran**, stamped
    /// at handler entry. Part 10 -- and this is bd discr-xfw's answer.
    ///
    /// Every handler opens by writing its own state number here: `$f5e2
    /// move.b #$01,$6ca9` for walk-left, `$f7f6 move.b #$02` for walk-right,
    /// `$1094a move.b #$14` for the turn transient, `$109aa move.b #$15`. The
    /// idle path clears it (`$f1c0`) when the joystick reads zero. Because a
    /// handler may then change `state_index` before the frame ends, the value
    /// sampled at the VBL is the state that ran *this* frame while
    /// `state_index` is the state that will run *next* -- which is exactly the
    /// one-frame lag the fixture shows, and why it reads like a previous-state
    /// field. It is not a facing flag; 1 and 2 are simply the two walk states.
    ///
    /// The name is kept for now because renaming it touches the schema, the
    /// fixture column and the differ together.
    pub facing: u8,
    /// ST `player+$0e` (`$6cae`): index into the 32-entry jump table at `$10e2c`.
    pub state_index: u8,
    /// ST `player+$10` (`$6cb0`): grid cell index, observed 9..16.
    pub grid_cell: u16,
    /// ST `player+$3a` (`$6cda` / `$6d5a`): the animation sequence cursor.
    ///
    /// Carried as the raw ST pointer because **the serve gates on its exact
    /// value**: `$c06e cmpi.l #$4602,$6d5a` for player 2's state 15 and
    /// `$c104 cmpi.l #$45da,$6d5a` for state 16. It is the one frame of the
    /// throw animation on which the disc leaves the hand.
    ///
    /// Driven by the animation engine (`$f1c4`), which this crate does not
    /// model, so it is a fed input. `// UNKNOWN: see bd discr-75o`.
    pub anim_cursor: u32,
    /// ST `player+$6e` (`$6d0e` / `$6d8e`): the `dir_kind` this player's throws
    /// carry, copied into `disc+$0a` at `$a9b4`.
    ///
    /// Reads `+1` for player 1 and `-3` for player 2 and never moves in any
    /// trace. Its sign is the direction of travel and its magnitude the
    /// per-frame `world_z` step, so player 2's disc comes back three times
    /// faster than player 1's goes out. `// UNKNOWN (its writer): see bd
    /// discr-qqt`.
    pub throw_dir_kind: i16,
    /// ST `player+$70` (`$6d10` / `$6d90`): the damage this player's throws do,
    /// copied into `disc+$16` at `$a9cc  move.w $6d90,($16,a1)`.
    ///
    /// Reads 1 for player 1 and 3 for player 2 -- the same magnitudes as
    /// [`Player::throw_dir_kind`]. Part 10b; before it, where `$a9a0` got the
    /// disc's damage from was unrecovered.
    pub throw_damage: i16,
    /// ST `player+$0a` (`$6caa`): the state to enter when the current
    /// animation sequence ends. Part 10.
    ///
    /// Written before entering the turn transient (`$f274` writes 1 for
    /// walk-left, `$f2c8` writes 2 for walk-right, `$f7b8`/`$f9ce` clear it on
    /// release) and read by state 20's handler at `$1099a`,
    /// `move.b $6caa,$6cae`.
    pub pending_state: u8,
    /// ST `player+$42` (`$6ce2`): frames left on the current animation cell.
    ///
    /// The state machine's clock. Every handler ends in the animation tail at
    /// `$f1c4`, which does `subq.w #1,$6ce2`; at zero it advances the sequence
    /// cursor `$6cda` by six bytes and reloads the count, and when the cursor
    /// reaches the sequence's zero terminator the state changes. `$6ce2` and
    /// `$6cda` were listed as `excluded:rendering` in `docs/state-schema.md`
    /// before Part 10; they are not rendering, they are the timer that decides
    /// when a state is over.
    ///
    /// Sequences are named in [`crate::player`]; a handler that runs one holds
    /// its cell index in [`Player::anim_cell`].
    pub anim_hold: u16,
    /// The ST address of the animation sequence this player is running -- the
    /// value a handler `lea`d into `$6cda` / `$6d5a` when it entered the state.
    ///
    /// Identifies the sequence for [`crate::player::anim_for`]. Distinct from
    /// [`Player::anim_cursor`], which is the *current* cell and advances.
    pub anim_base: u32,
    /// Which cell of the current animation sequence is showing. ST: the offset
    /// of `$6cda` from the sequence base, in six-byte steps.
    pub anim_cell: u8,
    /// The cell whose frame block was last copied into `$6ce4` by `$f1ca`.
    ///
    /// Handlers compare against it to detect "the sequence advanced since last
    /// frame": `$10560 move.l (A1),D0; $10562 cmp.l $6ce4,D0; beq` is state
    /// 11's test, and it is the whole of what paces the player's vertical
    /// movement while it is being knocked down. [`crate::player::NO_CELL`] is
    /// the value on entering a new sequence, where `$6ce4` still holds the
    /// *previous* sequence's block and so can never match.
    pub anim_shown: u8,
    /// ST `player+$1c`..`+$22` (`$6cbc`, `$6cbe`, `$6cc0`, `$6cc2`): the four
    /// words of this player's hit box, in that order.
    ///
    /// **Copied out of the current animation cell's frame block every frame**
    /// by `$f1ca`, so the box changes shape as the sprite does. This crate does
    /// not carry the frame blocks, so it is a fed input.
    /// `// UNKNOWN: see bd discr-75o`.
    ///
    /// The hit test reads them as `x in [px - 8 + b0, px - 8 + b0 + 8 + b1]`
    /// and `y in [99 + b2, 99 + b2 + b3]` (`$110fc`-`$1112c`).
    pub hit_box: [i16; 4],
    /// ST `player+$76` (`$6d16` / `$6d96`): the energy a strike subtracts from.
    ///
    /// `$11178`-`$111c6`: `d5 = $6d16`, minus the striking disc's `+$16` unless
    /// the bonus code is 4, stored back, and clamped to 0 -- at which point
    /// `$111ca st $6cac` marks the player down. Player 1 reads 5 at the start of
    /// the golden fixture, 2 after the first strike and 0 after the second.
    pub energy: i16,
    /// ST `player+$0d` (`$6cad` / `$6d2d`): set on the OTHER player when this
    /// one runs out of energy. ST `$f1b4`: `st $6d2d`, three instructions after
    /// player 1 enters state 23.
    ///
    /// The disc loop reads it: `$a564 tst.b $6d2d; bne $a570` retires every disc
    /// in play and drops the count, so **setting it is what ends a round**.
    /// `// UNKNOWN (what clears it): see bd discr-st8`.
    pub round_over: bool,
    /// ST `player+$6a` (`$6d0a` / `$6d8a`): how many discs this player has in
    /// play.
    ///
    /// `$a9aa addq.w #$01,$6d8a` on a serve, `$cab2`/`$cb22 subq.w #$01` when a
    /// catch retires one, and four more sites in the disc loop's possession
    /// paths that this crate does not model. Player 1's is never written in
    /// either fixture. Read by state 18's handler at `$c1c4`, which refuses to
    /// throw when it equals `player+$6c` -- a cap that reads 4 for player 2 and
    /// is never written at all. `// UNKNOWN: see bd discr-b6x`.
    pub discs_out: i16,
    /// ST `player+$1a` (`$6cba` / `$6d3a`): a per-frame X delta, **authored in
    /// the animation data**.
    ///
    /// `$f1ca` copies it out of the current animation cell like the hit box, and
    /// the idle path consumes it: `$f110 move.w $6cba,d0; $f114 clr.w $6cba;
    /// $f118 add.w d0,$6ca2`, mirrored at `$abbe`-`$abc6`. So some movement
    /// lives in the sprite tables rather than in code, and it is the only reason
    /// a standing player's `world_x` moves at all. A fed input, in the same
    /// category as the hit box. `// UNKNOWN: see bd discr-75o`.
    pub x_delta: i16,
    /// ST `player+$08` (`$6ca8` / `$6d28`): which way this player last threw.
    ///
    /// `$ae88 st` on a left throw, `$ae26 clr.b` on a right one, and `$cc3a
    /// clr.b` when an intercept commits. `$adc2 tst.b $6d28; bne` uses it to
    /// pick which side to probe when the stick is not pushed either way.
    pub threw_left: bool,
    /// ST `player+$6c` (`$6d0c` / `$6d8c`): the cap on [`Player::discs_out`].
    ///
    /// `$c1c4 cmp.w $6d8c,d0; beq` -- state 18's handler refuses to throw when
    /// the count has reached it. Reads 4 for player 2 and **0 for player 1**,
    /// whose count is also 0, so player 1 can never throw from that state --
    /// which is consistent with player 1 never throwing in either fixture.
    /// Never written anywhere in the analysed image.
    pub disc_cap: i16,
    /// ST `player+$12` (`$6cb2` / `$6d32`): how far ahead of itself this player
    /// can reach a disc. 12 for player 1, 26 for player 2.
    ///
    /// Read by both hit tests -- `$cb56`-`$cb66` subtracts it from the player's
    /// own depth to get the near edge of the window it starts tracking a disc
    /// in, and bonus code 5 replaces it with a flat `$32` = 50. Nothing in the
    /// analysed image writes it and it never moves in any trace.
    /// `// UNKNOWN (its writer): see bd discr-b6x`.
    pub reach: i16,
    /// ST `player+$0c` (`$6cac`): set by `$111ca` when the energy reaches 0.
    ///
    /// Gates the whole hit test (`$10fd8 tst.b $6cac; bne`) and the idle path
    /// (`$f11c`). No trace column: this crate produces it and nothing checks
    /// it. `// UNKNOWN (what clears it): see bd discr-75o`.
    pub down: bool,
}

/// The width of one arena column in world-X units. ST `$7bfe` is 152 bytes of
/// `1 + x / 40`, so the boundaries sit at 40, 80 and 120.
pub const COLUMN_WIDTH: i16 = 40;

/// How many bytes of `$7bfe` are the column table: **160**, four blocks of 40
/// giving 1, 2, 3, 4, and then zeros from index 160.
///
/// An earlier revision said 152 and was simply a short dump. It mattered: the
/// disc reads the table at `x + 4` (`$a250`), so a disc at `world_x` 151 --
/// which the `$9b` ceiling allows -- indexes 155, and the 152-byte reading made
/// [`crate::disc::disc_cell`] give up exactly where `tile_damage.ndjson` frame
/// 208 has it destroy a cell.
pub const COLUMN_TABLE_LEN: i16 = 160;

/// Which steering routine is installed in a disc's `+$12` hook.
///
/// ST Part 10. `scan` over the analysed image finds every site that writes the
/// field, and there are only three routines and one clear:
///
/// | value | ST | aim | installed by |
/// |---|---|---|---|
/// | [`SteerHook::None`] | `clr.l ($12,a5)` | -- | every bound, and `$a276` |
/// | [`SteerHook::AtP1`] | `$a71a` | `$6ca2 - $13`, then `$a758`'s `$6ca4 - $10` | `$113e2`, in player 1's cascade |
/// | [`SteerHook::AtP1Wide`] | `$a78e` | `$6ca2 - $04`, **X only** | `$11334` and `$11372`, same cascade |
/// | [`SteerHook::AtP2Wide`] | `$a7d8` | `$6d22 - $04`, **X only** -- it `rts`es at `$a814` | `$cb70` and `$cbae`, in player 2's hit test `$c826` |
/// | [`SteerHook::AtP2Deep`] | `$a816` | `$6d22 - $13`, then falls through to `$a854`'s `$6d24 - $10` | `$cc1e`, same routine |
///
/// The one structural difference between the three is that `$a7d8` returns
/// before the vertical block and the other two fall into it, which is why a
/// disc under `$a7d8` holds its `world_y` and one under `$a816` climbs to 83.
/// `tests/fixtures/golden.ndjson` frames 11-28 show exactly that: `$a7d8` from
/// frame 11 with `world_y` pinned at 81, `$a816` from frame 22, and `world_y`
/// 81 -> 82 -> 83 on frames 23-24.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SteerHook {
    /// No hook: the disc is not steered this frame.
    #[default]
    None,
    /// ST `$a71a` (+ `$a758`): home on player 1 with the deep X offset, both axes.
    AtP1,
    /// ST `$a78e`: home on player 1 with the shallow X offset, X only -- the
    /// exact mirror of [`SteerHook::AtP2Wide`].
    ///
    /// Found in Part 11 by a fixture that walks player 1 into a disc's path.
    /// **No earlier trace had ever installed it**, so the enum was missing a
    /// variant and `tracecheck` would have panicked on it rather than quietly
    /// mis-steering -- which is why that mapping panics.
    AtP1Wide,
    /// ST `$a7d8`: home on player 2 with the shallow X offset, X only.
    AtP2Wide,
    /// ST `$a816` (+ `$a854`): home on player 2 with the deep X offset, both axes.
    AtP2Deep,
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
    /// **ST `disc+$10`**, verbatim: the byte that says whether the slot is in
    /// play, and its whole three-state life is modelled as of Part 10g.
    ///
    /// ```text
    /// $ff   live -- $a4f0 tst.b beq skips a free slot, $a534 tst.b bpl a
    ///       retired one, so bit 7 is "simulate this record"
    /// 3..1  retired but not yet free: the record is frozen and $012588
    ///       counts the byte down one per frame from the render pass
    /// 0     free -- $a9a2's slot search will fill it
    /// ```
    ///
    /// Every writer, from `--watch 0x6e4e 0x6e4f` over 215 frames:
    ///
    /// | PC | what |
    /// |---|---|
    /// | `$a9b8` | `st` -- the serve claims the slot |
    /// | `$caae` | `addq.b #4` -- player 2 catches it from state 18 |
    /// | `$cb1e` | `addq.b #4` -- ...or from state 27 |
    /// | `$012588` | `subq.b #1` -- the render pass counts a retired slot down |
    ///
    /// `$ff + 4` is `$03`, and the countdown's first step lands in the same tick
    /// as the catch, so a caught disc reads 2, 1, 0 on the next three frames and
    /// its record does not move again. **That is bd discr-0fm's "dwell":** not a
    /// `world_z` phase and not an anomaly, but a disc that has been caught.
    pub active: u8,
    /// Which player "has" this disc. **ST `disc+$11`** (Part 10).
    ///
    /// `$a55e`, `$a5d0` and `$a612` branch on this byte, and the wall handlers
    /// flip it (`st ($11,a5)` at the far wall, `clr.b ($11,a5)` at the near
    /// one) while moving four counters -- `$6d8a`, `$6d8c`, `$6d0a`, `$6d0c` --
    /// in opposite directions. So the ST field is real and mirrored, not
    /// modelled. **Which byte value names which player is not settled**: every
    /// trace we have reads 0 on every live slot, so no trace has ever seen a
    /// disc change hands. `// UNKNOWN: see bd discr-ovl.2`.
    ///
    /// It is *not* what selects the steering aim point -- that is
    /// [`DiscSlot::hook`].
    pub aim: PlayerId,
    /// The per-disc steering hook. **ST `disc+$12`** (Part 10).
    ///
    /// A longword function pointer, called at `$a54c` *before* the integration
    /// and cleared by every one of the four bounds. It is what the Part 9
    /// section of `docs/disc-notes.md` was looking for when it asked "what
    /// gates the `$a71a` steering block": nothing gates it -- it runs exactly
    /// while a hook is installed, and only the two players' hit tests install
    /// one.
    pub hook: SteerHook,
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

impl DiscSlot {
    /// Whether `$a4ea` simulates this record. ST `$a534`: `tst.b ($10,a5); bpl`.
    #[must_use]
    pub const fn simulated(self) -> bool {
        self.active & 0x80 != 0
    }

    /// Whether the slot is taken at all. ST `$a4f0` and `$a9a2`:
    /// `tst.b ($10,a5); beq` -- **not** the same test as [`Self::simulated`].
    #[must_use]
    pub const fn occupied(self) -> bool {
        self.active != 0
    }
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
