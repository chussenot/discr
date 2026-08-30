//! Player 2's AI policy -- ST `$d2cc` / `$cea6` / `$efa8`.
//!
//! # Scope, stated before anything else
//!
//! `$d2cc` walks a 20-entry priority table at `$efa8` once a frame and
//! writes its decision to `$6da1`, the synthetic joystick byte `$abb2`
//! consumes for player 2 exactly where a human's `$6c59` would go
//! (`docs/state-schema.md`). `reports/part12-ai.md` has the full measured
//! decode -- table, dispatch loop, sensor pass, the "plan" mini-VM the
//! table's actions compile into -- with an ST address on every claim.
//!
//! **This module implements two of the twenty rows.** Every row's test
//! only runs after a reaction roll (`$d2f8`-`$d308`): `$6c5d += $6ab5`,
//! then fail if the running total exceeds the row's threshold. Eighteen
//! rows have a threshold under 255, so their outcome depends on `$6c5d`
//! -- a byte no fixture feeds and which (per the report) cannot be
//! reconstructed from one, because its own increments depend on which row
//! was latched on prior frames, which depends on earlier rolls, all the
//! way back to a reset this crate never observes. Two rows -- priority 50
//! (the escape, `$e0d8`/`$e214`) and priority 30 (the avoid, `$e158`/
//! `$e214`) -- carry threshold 255, and a `u8` reaction roll can never
//! exceed 255: their roll cannot fail. Those two are what this module
//! computes. The other eighteen are documented, not guessed at.
//!
//! Rows 0 and 1 share both their action (`$e214`) and identity (`$e290`)
//! routines, so the ST treats them as one latch: once either fires, the
//! *other* cannot preempt it (`$d2ec`'s identity-match check compares
//! function pointers, and theirs are equal) until the maneuver ends on its
//! own. This module mirrors that with one `motive` field rather than two.

use core::cmp::Ordering;

use crate::disc::PLAYER_HEIGHT_REF;
use crate::{
    COLUMN_TABLE_LEN, COLUMN_WIDTH, DISC_SLOTS, DirBits, DiscSlot, GameState, Player, TILE_CELLS,
    Tile,
};

/// `$e0d8` and `$e158` both refuse to fire while player 2's own
/// `state_index` (ST `player+$0e`) is one of these four values.
const TEST_EXCLUDED_STATES: [u8; 4] = [0x15, 0x16, 0x1d, 0x1e];

/// The step executor `$e30a` (run every frame a motive is latched, the
/// triggering frame included) refuses to output a direction -- and ends
/// the maneuver -- in a DIFFERENT four states, read at `$e33a`-`$e358`.
/// Kept as its own table: two byte lists at two addresses, not one rule
/// read twice.
const STEP_BUSY_STATES: [u8; 4] = [0x0d, 0x0e, 0x18, 0x19];

/// `$d062` and, independently, `$e2d0`: the same formula transcribed
/// twice at two addresses, `colTable[x] + (4 if y <= ROW_SPLIT else 0) -
/// 1`, landing in `0..=7` -- the floor bank's own index. `$3a` = 58, and
/// it is its own constant: distinct from `disc::DISC_FAR_ROW_Y` (70,
/// `$a25a`) and from `player+$06`'s row split (14, `$f838`).
const ROW_SPLIT: i16 = 0x3a;

/// `$1556`: one byte per floor cell (`0..=7`), bit `n` set means escape
/// direction code `n` is usable from that cell. Read directly from
/// `discram.bin` at `$1556` -- Ghidra disassembles code, not data, so this
/// table (and the two below) were pulled with a raw byte read, then
/// cross-checked against the bit test that consumes them at `$e13c`.
const ESCAPE_ALLOWED: [u8; 8] = [0x37, 0x7f, 0xef, 0xce, 0x73, 0xf7, 0xfe, 0xec];

/// `$155e`: per floor cell, the priority-ordered list of direction codes
/// to try, `$ff`-terminated (`$e134`-`$e13a`). 8 rows of 8 bytes.
const ESCAPE_ORDER: [[u8; 8]; 8] = [
    [0x01, 0x05, 0x02, 0x04, 0xff, 0xff, 0xff, 0xff],
    [0x02, 0x05, 0x06, 0x00, 0x04, 0x03, 0xff, 0xff],
    [0x01, 0x06, 0x05, 0x03, 0x07, 0x00, 0xff, 0xff],
    [0x02, 0x06, 0x01, 0x07, 0xff, 0xff, 0xff, 0xff],
    [0x05, 0x01, 0x06, 0x00, 0xff, 0xff, 0xff, 0xff],
    [0x06, 0x01, 0x02, 0x04, 0x00, 0x07, 0xff, 0xff],
    [0x05, 0x02, 0x01, 0x07, 0x03, 0x04, 0xff, 0xff],
    [0x06, 0x02, 0x05, 0x03, 0xff, 0xff, 0xff, 0xff],
];

/// `$15fe`: the `(world_x, world_y)` center of each of the 8 escape
/// direction codes -- four `COLUMN_WIDTH`-spaced columns (20, 60, 100,
/// 140) times two rows, 64 (far, `y > ROW_SPLIT`) and 54 (near).
const ESCAPE_TARGET: [(i16, i16); 8] = [
    (20, 64),
    (60, 64),
    (100, 64),
    (140, 64),
    (20, 54),
    (60, 54),
    (100, 54),
    (140, 54),
];

/// `$d062`/`$e2d0`'s shared formula. `None` where the column table
/// (`$7bfe`, `COLUMN_TABLE_LEN` long) would be read out of bounds, or the
/// result goes negative (`$e2ec`'s `bmi`).
fn floor_cell_index(x: i16, y: i16) -> Option<usize> {
    let column = if (0..COLUMN_TABLE_LEN).contains(&x) {
        1 + x / COLUMN_WIDTH
    } else {
        0
    };
    let idx = column + if y <= ROW_SPLIT { 4 } else { 0 } - 1;
    (idx >= 0).then_some(idx as usize)
}

/// Is the floor cell at `(x, y)` still there? ST reads `tiles_far`'s HP
/// word (`$759e+2`, masked `0x7f`) directly at `$e0c8`/`$e2f8`; this crate
/// uses [`Tile::walkable`] instead, which `tile.rs`'s HP-reaches-0-clears-
/// tile_type invariant makes equivalent.
fn floor_walkable(tiles_far: &[Tile; TILE_CELLS], x: i16, y: i16) -> bool {
    match floor_cell_index(x, y) {
        Some(i) if i < tiles_far.len() => tiles_far[i].walkable(),
        _ => false,
    }
}

/// `$e0d8`: is player 2 standing on a destroyed floor cell and, if so,
/// which `(world_x, world_y)` should it walk to? `None` is the test's
/// `$e154` (`moveq #-1,d0`).
fn test_escape(p2: &Player, tiles_far: &[Tile; TILE_CELLS]) -> Option<(i16, i16)> {
    if TEST_EXCLUDED_STATES.contains(&p2.state_index) {
        return None;
    }
    // $e0f8-$e110: player 2's own floor cell, ST `player+$10` minus 9.
    let cell = i16::try_from(p2.grid_cell).ok()? - 9;
    if !(0..8).contains(&cell) {
        return None;
    }
    let cell = cell as usize;
    if tiles_far[cell].walkable() {
        return None; // floor is fine -- nothing to escape ($e110 bne)
    }
    // $e11e-$e13e: this cell's direction codes, in priority order; the
    // first one the bitmask allows wins.
    let code = *ESCAPE_ORDER[cell]
        .iter()
        .take_while(|&&c| c != 0xff)
        .find(|&&c| ESCAPE_ALLOWED[cell] & (1 << c) != 0)?;
    Some(ESCAPE_TARGET[code as usize])
}

/// `$e158`: does an active, simulated disc lie in the box `$e186`-`$e1c6`
/// builds from player 2's own hit box and position and, if so, which
/// `(world_x, world_y)` side-step clears it? Falls back to
/// [`test_escape`]'s table when the box test passes but neither side is
/// walkable, exactly as the ST falls through from `$e1e4`/`$e202` into
/// `$e112`.
fn test_avoid(
    p2: &Player,
    discs: &[DiscSlot; DISC_SLOTS],
    tiles_far: &[Tile; TILE_CELLS],
) -> Option<(i16, i16)> {
    if TEST_EXCLUDED_STATES.contains(&p2.state_index) {
        return None;
    }
    for d in discs {
        if !d.simulated() {
            continue; // $e182 bpl
        }
        // $e186-$e19e: the Y window, from player 2's own hit box.
        let y_lo = PLAYER_HEIGHT_REF + p2.hit_box[2];
        let y_hi = y_lo + p2.hit_box[3];
        if d.world_y < y_lo || d.world_y > y_hi {
            continue;
        }
        // $e1a2-$e1aa: the disc must still be in front of player 2.
        if d.world_z <= p2.world_y {
            continue;
        }
        // $e1ae-$e1c6: the X window, same construction.
        let x_lo = p2.world_x - 8 + p2.hit_box[0];
        let x_hi = p2.world_x + p2.hit_box[0] + p2.hit_box[1];
        if d.world_x <= x_lo || d.world_x >= x_hi {
            continue;
        }
        // Box test passed for this disc. $e1ca-$e206: try to side-step
        // it, preferring the side the disc is NOT already drifting
        // toward (`disc+$06`, vel_x).
        let left = p2.world_x - 8;
        let right = p2.world_x + 8;
        let step = if d.vel_x < 0 {
            side_step(tiles_far, right, p2.world_y)
                .or_else(|| side_step(tiles_far, left, p2.world_y))
        } else {
            side_step(tiles_far, left, p2.world_y)
                .or_else(|| side_step(tiles_far, right, p2.world_y))
        };
        // $e1e4/$e202: neither side clears it -- fall through to the
        // escape table, same as the ST.
        return step.or_else(|| test_escape(p2, tiles_far));
    }
    None
}

fn side_step(tiles_far: &[Tile; TILE_CELLS], x: i16, y: i16) -> Option<(i16, i16)> {
    floor_walkable(tiles_far, x, y).then_some((x, y))
}

/// Player 2's AI: the two deterministic rows of the table at `$efa8`.
///
/// Holds the one piece of state their shared latch needs -- `$6da6`/
/// `$6dac` collapsed to the `(world_x, world_y)` target `$e30a` walks
/// toward, since both rows write the same action/identity pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ai {
    motive: Option<(i16, i16)>,
}

impl Ai {
    /// `$d2cc`, entries 0 and 1 only -- see the module docs and
    /// `reports/part12-ai.md` for why the other 18 are out of reach.
    ///
    /// Returns the byte `$abb2` would read from `$6da1`: `DirBits` only
    /// (`$01`/`$02`/`$04`/`$08`) -- neither row ever sets the fire bit
    /// (`$80`), so this never does either.
    #[must_use]
    pub fn p2_policy(&mut self, state: &GameState) -> u8 {
        let p2 = &state.players[crate::PlayerId::Two.index()];
        if self.motive.is_some() {
            return self.step(p2, &state.tiles_far);
        }
        if let Some(target) = test_escape(p2, &state.tiles_far) {
            self.motive = Some(target);
            return self.step(p2, &state.tiles_far);
        }
        if let Some(target) = test_avoid(p2, &state.discs, &state.tiles_far) {
            self.motive = Some(target);
            return self.step(p2, &state.tiles_far);
        }
        0
    }

    /// `$e30a`: walk toward the latched motive, or end it. Called every
    /// frame the motive is set, including the frame it was just latched
    /// -- `$d332`'s post-loop identity call runs unconditionally once
    /// `$6da6` is nonzero, same frame or not.
    fn step(&mut self, p2: &Player, tiles_far: &[Tile; TILE_CELLS]) -> u8 {
        let Some((tx, ty)) = self.motive else {
            return 0;
        };
        // $e2d0: the target cell may have collapsed since the motive was
        // set (an escape target that itself later hits zero HP) --
        // re-validated every frame, not just when the motive is chosen.
        if !floor_walkable(tiles_far, tx, ty) {
            self.motive = None;
            return 0;
        }
        if STEP_BUSY_STATES.contains(&p2.state_index) {
            self.motive = None;
            return 0;
        }
        let mut out = DirBits::NONE;
        out = out.or(match tx.cmp(&p2.world_x) {
            Ordering::Greater => DirBits::RIGHT,
            Ordering::Less => DirBits::LEFT,
            Ordering::Equal => DirBits::NONE,
        });
        out = out.or(match ty.cmp(&p2.world_y) {
            Ordering::Greater => DirBits::UP,
            Ordering::Less => DirBits::DOWN,
            Ordering::Equal => DirBits::NONE,
        });
        // $e35a-$e370: within [-4,+4] x and [-2,+2] y of the target ->
        // arrived; clear the output and end the maneuver ($e372).
        let arrived =
            (tx - 4..=tx + 4).contains(&p2.world_x) && (ty - 2..=ty + 2).contains(&p2.world_y);
        if arrived {
            self.motive = None;
            return 0;
        }
        out.0
    }
}

/// Measures [`Ai::p2_policy`] against `ai_6da1`/`pass_ai` in the three
/// committed fixtures, reading them directly (a handful of field-scoped
/// structs, not `disc-tools`'s `Frame`, which `discr-b6x` does not own).
/// The numbers this measures are reported and pinned in
/// `reports/part12-ai.md`; nothing here gates `mise run core-check` --
/// that stays the five `tracecheck` invocations, which this module does
/// not touch.
#[cfg(test)]
mod agreement {
    use serde::Deserialize;

    use super::{Ai, DISC_SLOTS, DiscSlot, Player, TILE_CELLS, Tile};

    #[derive(Deserialize)]
    struct TPlayer {
        x: i16,
        y: i16,
        state: u8,
        cell: u16,
        #[serde(default, rename = "box")]
        hit_box: [i16; 4],
    }

    #[derive(Deserialize)]
    struct TDisc {
        wx: i16,
        wy: i16,
        wz: i16,
        vx: i16,
        #[serde(default)]
        act: Option<u8>,
    }

    #[derive(Deserialize)]
    struct TFrame {
        /// How many `$96be` passes produced this sample (Part 11f). A
        /// comparison only means one thing when this is exactly 1 -- see
        /// `measure`.
        #[serde(default = "one")]
        updates: u16,
        #[serde(default)]
        ai_6da1: u8,
        #[serde(default)]
        pass_ai: Vec<u8>,
        player: [TPlayer; 2],
        #[serde(default)]
        disc: Vec<TDisc>,
        #[serde(default)]
        banks: Vec<(u16, i16)>,
    }

    fn one() -> u16 {
        1
    }

    impl TFrame {
        /// `$6da1` as this frame's tick used it -- the schema's "one VBL"
        /// note: a frame's own `ai_6da1` is what the PRIOR tick consumed,
        /// sampled at this VBL, so the byte a policy call made from the
        /// PREVIOUS frame's state is checked against THIS frame's first
        /// pass (`docs/state-schema.md:183-200`).
        fn sampled_ai(&self) -> u8 {
            self.pass_ai.first().copied().unwrap_or(self.ai_6da1)
        }

        fn player2(&self) -> Player {
            let p = &self.player[1];
            Player {
                world_x: p.x,
                world_y: p.y,
                state_index: p.state,
                grid_cell: p.cell,
                hit_box: p.hit_box,
                ..Player::default()
            }
        }

        fn discs(&self) -> [DiscSlot; DISC_SLOTS] {
            let mut out: [DiscSlot; DISC_SLOTS] = Default::default();
            for (d, t) in out.iter_mut().zip(&self.disc) {
                *d = DiscSlot {
                    active: t.act.unwrap_or(0),
                    world_x: t.wx,
                    world_y: t.wy,
                    world_z: t.wz,
                    vel_x: t.vx,
                    ..DiscSlot::default()
                };
            }
            out
        }

        /// `banks`' first 16 pairs are `$7596`, player 2's floor -- the
        /// same slice `disc-tools`' `Frame::seed` takes (Part 10e).
        fn tiles_far(&self) -> [Tile; TILE_CELLS] {
            let mut out: [Tile; TILE_CELLS] = Default::default();
            for (tile, &(tile_type, hp)) in out.iter_mut().zip(self.banks.iter().take(16)) {
                *tile = Tile { tile_type, hp };
            }
            out
        }
    }

    /// `predicted[k] == fixture[k+1].sampled_ai()` for every frame this
    /// module's two rows can possibly have produced; returns
    /// `(agreeing, total_compared)`.
    ///
    /// Only a transition with exactly one `$96be` pass (`next.updates ==
    /// 1`) is comparable at all: `$d2cc` runs once per pass, not once per
    /// sample (Part 11g), so a 0-pass tick ran no AI this crate could have
    /// predicted and a 2-pass tick ran it against an intermediate disc
    /// position no fixture records. Both are skipped -- not counted as
    /// disagreements -- and neither advances [`Ai`]'s latched motive,
    /// since the real `$d2cc` did not evaluate rows 0/1 against a state
    /// this harness can reconstruct either.
    fn measure(path: &str) -> (usize, usize) {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let frames: Vec<TFrame> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{path}: {e}")))
            .collect();
        let mut ai = Ai::default();
        let (mut agree, mut total) = (0, 0);
        for (idx, w) in frames.windows(2).enumerate() {
            let [cur, next] = w else { unreachable!() };
            if next.updates != 1 {
                continue;
            }
            let state = crate::GameState {
                players: [Player::default(), cur.player2()],
                discs: cur.discs(),
                tiles_far: cur.tiles_far(),
                ..crate::GameState::default()
            };
            let predicted = ai.p2_policy(&state);
            total += 1;
            let actual = next.sampled_ai();
            if predicted == actual {
                agree += 1;
            } else if std::env::var("AI_DIAG").is_ok() {
                eprintln!(
                    "frame {idx} tick {total}: predicted={predicted:#04x} actual={actual:#04x} p2=({},{}) state={} cell={}",
                    cur.player2().world_x,
                    cur.player2().world_y,
                    cur.player2().state_index,
                    cur.player2().grid_cell,
                );
            }
        }
        (agree, total)
    }

    /// `env!("CARGO_MANIFEST_DIR")` is `crates/disc-core`; the fixtures
    /// live at the repo root.
    fn fixture(name: &str) -> String {
        format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    /// Measured 2026-08-30, entries 0/1 only (`$e0d8` escape, `$e158`
    /// avoid; the other 18 rows need `$6c5d`, undocumented -- see the
    /// module docs). Never destroys player 2's own floor cell or puts a
    /// disc in its avoid box in this fixture, so both rows stay silent
    /// and every predicted byte is 0; the number below is exactly the
    /// fraction of frames whose own sampled `ai_6da1` is already 0 (an
    /// RNG-gated row of the other eighteen produced every nonzero byte
    /// here). Pinned so a regression is visible; see
    /// `reports/part12-ai.md` for the fixture-by-fixture count.
    #[test]
    fn golden_agreement() {
        let (agree, total) = measure(&fixture("golden.ndjson"));
        println!("golden: {agree}/{total}");
        assert_eq!((agree, total), (18, 99));
    }

    /// Same as [`golden_agreement`] -- entry 1's box never catches a disc
    /// in this fixture either.
    #[test]
    fn tile_damage_agreement() {
        let (agree, total) = measure(&fixture("tile_damage.ndjson"));
        println!("tile_damage: {agree}/{total}");
        assert_eq!((agree, total), (61, 214));
    }

    /// Almost every one of the 178 disagreements below is the same shape
    /// as the other two fixtures' -- an RNG-gated row of the other 18
    /// produced a nonzero byte this module cannot predict, so it predicts
    /// 0 and misses. **Four are different, and worth reading precisely
    /// because they are NOT that**: at frame 256 player 2 enters state 11
    /// (knocked down, mirroring `$ca12`), and with a disc still sitting in
    /// its avoid box (`$e186`-`$e1c6`) this module latches entry 1 and
    /// steers -- `predicted = $04`. The ST's own byte there is `$06`, not
    /// `$00`: whatever it is doing, it is not silent either. Neither
    /// `$e158`'s own exclusion list (states `$15`/`$16`/`$1d`/`$1e`) nor
    /// the step executor `$e30a`'s (`$d`/`$e`/`$18`/`$19`) names state 11,
    /// so nothing in the code this module transcribes says a knockdown
    /// should suppress or change row 1's output -- and guessing a fourth
    /// exclusion state to make the number match is exactly what the house
    /// rules ask not to do. Left open: whatever governs `$6da1` during a
    /// knockdown is a different bead's find.
    #[test]
    fn p1_walk_agreement() {
        let (agree, total) = measure(&fixture("p1_walk.ndjson"));
        println!("p1_walk: {agree}/{total}");
        assert_eq!((agree, total), (22, 200));
    }
}
