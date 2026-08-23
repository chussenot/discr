//! `tracecheck` -- replay an Atari ST trace against `disc-core` and report the
//! **first** divergence.
//!
//! Bead `discr-3g6`.
//!
//! # What it does
//!
//! 1. Reads an NDJSON trace (one JSON object per line, one line per frame,
//!    sampled at `PC == $8198` -- i.e. *before* the VBL handler's
//!    `addq.w #1,$6ab4`, so a record is the state the tick is about to consume).
//! 2. Seeds a [`GameState`] from frame 0 of the trace.
//! 3. Drives [`GameState::tick`] forward, deriving each frame's [`Input`] from
//!    the trace's own `joy_6c58` history.
//! 4. Compares exactly the rows marked `compared` in `docs/state-schema.md`,
//!    reports the first mismatch as `(frame, field, expected, got)` and stops.
//!
//! Divergence-first, not a pass/fail tally: a tally over a diverged simulation
//! counts noise. The first mismatch is the only one with a cause you can chase,
//! which is the standard `reports/` sets for the oracle diffs.
//!
//! # Input decoding (ST `$6c58`)
//!
//! Direction bits are levels -- `$01` up, `$02` down, `$04` left, `$08` right.
//! Fire (`$80`) is **edge**-consumed: `bclr #7,(a0)` at `$f606` / `$f81a` /
//! `$fb90` clears it on use, so `fire_edge` is true only on the frame the bit
//! goes 0 -> 1, never while it is held.
//!
//! Player 2's input is waived (`discr-b6x`): `disc-core` takes it from its
//! caller and the trace carries no `$6c59`, so p2 is driven with no input and
//! a p2-only divergence is expected rather than a bug.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use disc_core::{DISC_SLOTS, DirBits, DiscSlot, GameState, Input, Player, TILE_CELLS, Tile};
use serde::Deserialize;

/// `docs/state-schema.md`, "Compared fields": 15 rows marked `compared`.
const SCHEMA_COMPARED: usize = 15;
/// `docs/state-schema.md`, "Waived and excluded": 7 rows marked `waived:`.
const SCHEMA_WAIVED: usize = 7;
/// `docs/state-schema.md`, "Waived and excluded": 6 rows marked `excluded:`.
const SCHEMA_EXCLUDED: usize = 6;
/// Compared rows this trace format carries no column for; see [`Frame`].
const NOT_IN_TRACE: [&str; 2] = ["discs[n].vel_y (disc+$08)", "discs[n].damage (disc+$16)"];

/// ST `$6c58` direction bits (`$01` up, `$02` down, `$04` left, `$08` right).
const JOY_DIR_MASK: u8 = 0x0f;
/// ST `$6c58` fire bit, cleared on use by `bclr #7`.
const JOY_FIRE_BIT: u8 = 0x80;

#[derive(Parser)]
#[command(
    name = "tracecheck",
    about = "Replay an ST trace against disc-core and report the first divergence."
)]
struct Cli {
    /// NDJSON trace to replay (e.g. tests/fixtures/golden.ndjson).
    trace: PathBuf,
    /// Stop after this many ticks instead of running the whole trace.
    #[arg(long, value_name = "N")]
    frames: Option<usize>,
}

// ---------------------------------------------------------------------------
// Trace records
//
// Unknown fields are ignored, which is how `sx`/`sy` (disc `+$0c`/`+$0e`) and
// `state_sha256` are dropped: screen X/Y are projection, `excluded:projection`
// in the schema, and must never be compared.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Frame {
    /// 0-based index within the trace, not an ST value.
    frame: u32,
    /// ST `$6ab4`, the VBL frame counter word.
    vbl_6ab4: u16,
    /// ST `$6c58`, player 1's decoded joystick byte.
    joy_6c58: u8,
    player: [TracePlayer; 2],
    disc: [TraceDisc; DISC_SLOTS],
    /// 17 cells of `[tile_type, hp]`.
    grid: [(u16, i16); TILE_CELLS],
}

#[derive(Deserialize)]
struct TracePlayer {
    /// `player+$02`.
    x: i16,
    /// `player+$06`.
    y: i16,
    /// `player+$09`.
    facing: u8,
    /// `player+$0e`.
    state: u8,
    /// `player+$10`.
    cell: u16,
}

#[derive(Deserialize)]
struct TraceDisc {
    /// `disc+$00`.
    wx: i16,
    /// `disc+$02`.
    wy: i16,
    /// `disc+$04`.
    wz: i16,
    /// `disc+$06`.
    vx: i16,
    /// `disc+$0a` `dir_kind`, emitted **unsigned**: `65533` is `-3`. The sign
    /// is the travel direction and the magnitude is the kind of disc; it is
    /// not a live flag, whatever the trace calls the column.
    flag: u16,
}

// ---------------------------------------------------------------------------
// Seeding and stepping
// ---------------------------------------------------------------------------

impl Frame {
    /// Player-1 input for the tick this record is about to be consumed by.
    ///
    /// `prev` is the previous record's `$6c58`, which is what makes the fire
    /// bit an edge rather than a level.
    fn input(&self, prev: u8) -> Input {
        Input {
            dir: DirBits(self.joy_6c58 & JOY_DIR_MASK),
            fire_edge: self.joy_6c58 & JOY_FIRE_BIT != 0 && prev & JOY_FIRE_BIT == 0,
        }
    }

    /// Build the `GameState` this record describes.
    fn seed(&self) -> GameState {
        let mut st = GameState {
            frame: u32::from(self.vbl_6ab4),
            ..GameState::default()
        };
        for (p, t) in st.players.iter_mut().zip(&self.player) {
            *p = Player {
                world_x: t.x,
                world_y: t.y,
                facing: t.facing,
                state_index: t.state,
                grid_cell: t.cell,
            };
        }
        for (d, t) in st.discs.iter_mut().zip(&self.disc) {
            *d = DiscSlot {
                // `active` and `aim` are waived (discr-m4x) and the trace has
                // no column for either. A nonzero dir_kind is the only signal
                // the trace offers for "this slot is in play"; it matches the
                // one live disc in the golden fixture.
                // ponytail: dir_kind != 0 as the liveness proxy -- revisit when
                // discr-m4x pins down how the ST marks an unused slot.
                active: t.flag != 0,
                aim: disc_core::PlayerId::One,
                world_x: t.wx,
                world_y: t.wy,
                world_z: t.wz,
                vel_x: t.vx,
                vel_y: 0,
                dir_kind: t.flag as i16,
                damage: 0,
            };
        }
        for (tile, &(tile_type, hp)) in st.tiles.iter_mut().zip(&self.grid) {
            *tile = Tile { tile_type, hp };
        }
        st
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// One compared value: schema field path, what the ST had, what `disc-core` got.
struct Check {
    field: String,
    expected: i64,
    got: i64,
}

/// Every `compared` row of `docs/state-schema.md` that this trace carries, in
/// the order the schema table lists them. Frame first, then players, then
/// discs, then tiles -- so a mismatch in the frame counter is never masked by
/// the state it indexes.
fn checks(expected: &Frame, got: &GameState) -> Vec<Check> {
    let mut v = Vec::new();
    let mut push = |field: String, e: i64, g: i64| {
        v.push(Check {
            field,
            expected: e,
            got: g,
        });
    };

    // `$6ab4` is a word and wraps; disc-core widens it, so compare as u16.
    push(
        "frame".into(),
        i64::from(expected.vbl_6ab4),
        i64::from(got.frame as u16),
    );

    for (n, (e, g)) in expected.player.iter().zip(&got.players).enumerate() {
        push(
            format!("players[{n}].world_x"),
            e.x.into(),
            g.world_x.into(),
        );
        push(
            format!("players[{n}].world_y"),
            e.y.into(),
            g.world_y.into(),
        );
        push(
            format!("players[{n}].facing"),
            e.facing.into(),
            g.facing.into(),
        );
        push(
            format!("players[{n}].state_index"),
            e.state.into(),
            g.state_index.into(),
        );
        push(
            format!("players[{n}].grid_cell"),
            e.cell.into(),
            g.grid_cell.into(),
        );
    }

    for (n, (e, g)) in expected.disc.iter().zip(&got.discs).enumerate() {
        push(format!("discs[{n}].world_x"), e.wx.into(), g.world_x.into());
        push(format!("discs[{n}].world_y"), e.wy.into(), g.world_y.into());
        push(format!("discs[{n}].world_z"), e.wz.into(), g.world_z.into());
        push(format!("discs[{n}].vel_x"), e.vx.into(), g.vel_x.into());
        // Signed compare: the trace emits dir_kind unsigned.
        push(
            format!("discs[{n}].dir_kind"),
            i64::from(e.flag as i16),
            g.dir_kind.into(),
        );
        // vel_y and damage: no column in this trace, see NOT_IN_TRACE.
    }

    for (n, (&(tile_type, hp), g)) in expected.grid.iter().zip(&got.tiles).enumerate() {
        push(
            format!("tiles[{n}].tile_type"),
            tile_type.into(),
            g.tile_type.into(),
        );
        push(format!("tiles[{n}].hp"), hp.into(), g.hp.into());
    }

    v
}

/// The first mismatch in schema order, or `None` if the frame agrees.
///
/// `report` needs the whole check list, so this exists for the tests.
#[cfg(test)]
fn first_divergence(expected: &Frame, got: &GameState) -> Option<Check> {
    checks(expected, got)
        .into_iter()
        .find(|c| c.expected != c.got)
}

/// Whether a field path belongs to player 2, whose input is waived.
fn is_player_two(field: &str) -> bool {
    field.starts_with("players[1]")
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/// One `field / expected / got / delta / before` block.
fn block(s: &mut String, prev: &Frame, d: &Check) {
    let _ = writeln!(s, "  field     {}", d.field);
    let _ = writeln!(s, "  expected  {:<8} (the ST, from the trace)", d.expected);
    let _ = writeln!(s, "  got       {:<8} (disc-core)", d.got);
    let _ = writeln!(s, "  delta     {:+}", d.got - d.expected);
    if let Some(before) = field_at(prev, &d.field) {
        let _ = writeln!(
            s,
            "  before    frame {} had {before}; the ST moved it to {}, disc-core {}",
            prev.frame,
            d.expected,
            if d.got == before {
                "left it unchanged".to_string()
            } else {
                format!("moved it to {}", d.got)
            }
        );
    }
}

/// Render the divergence report. Separate from `main` so a test can read it.
///
/// `cs` is the whole check list for this frame, already computed. When the
/// first divergence is a `players[1]` row the report also names the first row
/// that is not, because p2 is driven with no input by design (`discr-b6x`) and
/// a p2 mismatch is an artefact of that waiver, not something anyone can fix.
fn report(prev: &Frame, expected: &Frame, input: Input, cs: &[Check]) -> Option<String> {
    let d = cs.iter().find(|c| c.expected != c.got)?;

    let mut s = String::new();
    let _ = writeln!(
        s,
        "DIVERGENCE at trace frame {} (ST $6ab4 = {} / ${:04x})",
        expected.frame, expected.vbl_6ab4, expected.vbl_6ab4
    );
    block(&mut s, prev, d);
    let _ = writeln!(
        s,
        "  tick      frame {} -> {}, driven with p1 dir=${:02x} fire_edge={}",
        prev.frame, expected.frame, input.dir.0, input.fire_edge
    );

    if is_player_two(&d.field) {
        let _ = writeln!(
            s,
            "  note      player-2 input is waived (discr-b6x): disc-core takes p2's Input from\n\
             \x20           its caller and this trace carries no $6c59, so tracecheck drives p2\n\
             \x20           with nothing. A players[1] row cannot match and is not a bug."
        );
        match cs
            .iter()
            .find(|c| c.expected != c.got && !is_player_two(&c.field))
        {
            Some(n) => {
                let _ = writeln!(s, "\nFirst divergence outside player 2, same frame:");
                block(&mut s, prev, n);
            }
            None => {
                let _ = writeln!(s, "\nEverything outside player 2 matches on this frame.");
            }
        }
    }
    Some(s)
}

/// The value a compared field held in a trace record, for the "unchanged?" line.
fn field_at(frame: &Frame, field: &str) -> Option<i64> {
    checks(frame, &frame.seed())
        .into_iter()
        .find(|c| c.field == field)
        .map(|c| c.expected)
}

// ---------------------------------------------------------------------------

fn run(cli: &Cli) -> Result<bool, String> {
    let text =
        std::fs::read_to_string(&cli.trace).map_err(|e| format!("{}: {e}", cli.trace.display()))?;
    let frames: Vec<Frame> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", cli.trace.display(), i + 1))
        })
        .collect::<Result<_, _>>()?;

    let [first, rest @ ..] = frames.as_slice() else {
        return Err(format!("{}: empty trace", cli.trace.display()));
    };
    let ticks = cli.frames.unwrap_or(rest.len()).min(rest.len());

    println!(
        "tracecheck {} -- {} frames",
        cli.trace.display(),
        frames.len()
    );
    println!(
        "docs/state-schema.md: {SCHEMA_COMPARED} compared, {SCHEMA_WAIVED} waived, \
         {SCHEMA_EXCLUDED} excluded"
    );
    println!(
        "  comparing {} of the {SCHEMA_COMPARED} compared rows; this trace has no column for: {}",
        SCHEMA_COMPARED - NOT_IN_TRACE.len(),
        NOT_IN_TRACE.join(", ")
    );
    println!(
        "  seeded from frame {} (ST $6ab4 = {}), driving {ticks} tick(s)",
        first.frame, first.vbl_6ab4
    );

    let mut state = first.seed();
    let mut prev_joy = first.joy_6c58;
    let mut prev = first;

    for expected in &rest[..ticks] {
        let input = prev.input(prev_joy);
        state.tick([input, Input::default()]);

        let cs = checks(expected, &state);
        if let Some(r) = report(prev, expected, input, &cs) {
            print!("\n{r}");
            println!(
                "\n{} tick(s) matched before this one.",
                expected.frame.saturating_sub(first.frame + 1)
            );
            return Ok(false);
        }

        prev_joy = prev.joy_6c58;
        prev = expected;
    }

    println!("\nOK: {ticks} tick(s), no divergence.");
    Ok(true)
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("tracecheck: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F0: &str = include_str!("../../../tests/fixtures/golden.ndjson");

    fn golden() -> Vec<Frame> {
        F0.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("golden fixture parses"))
            .collect()
    }

    #[test]
    fn golden_fixture_parses_and_seeds() {
        let f = golden();
        assert_eq!(f.len(), 100);
        let st = f[0].seed();
        assert_eq!(st.frame, 6949);
        assert_eq!(st.players[0].world_x, 117);
        assert_eq!(st.discs[0].world_z, 20);
        assert_eq!(st.tiles.len(), 17);
    }

    #[test]
    fn dir_kind_is_signed_even_though_the_trace_emits_it_unsigned() {
        // 65533 in the trace is -3 on the ST, not a large positive number.
        let d: TraceDisc =
            serde_json::from_str(r#"{"wx":0,"wy":0,"wz":0,"vx":0,"flag":65533}"#).unwrap();
        assert_eq!(d.flag as i16, -3);
    }

    #[test]
    fn fire_is_an_edge_not_a_level() {
        let mut f: Frame = serde_json::from_str(F0.lines().next().unwrap()).unwrap();
        f.joy_6c58 = 0x84;
        assert!(f.input(0x00).fire_edge, "0 -> 1 on bit 7 is an edge");
        assert!(
            !f.input(0x80).fire_edge,
            "held fire is consumed, not an edge"
        );
        assert_eq!(f.input(0x00).dir, DirBits::LEFT, "$04 is Left");
    }

    #[test]
    fn a_frame_compared_against_its_own_seed_never_diverges() {
        for f in &golden() {
            let st = f.seed();
            assert!(
                first_divergence(f, &st).is_none(),
                "frame {} disagrees with its own seed",
                f.frame
            );
        }
    }

    #[test]
    fn projection_is_never_compared() {
        // sx/sy are excluded:projection. No check may mention them.
        let f = golden();
        let names: Vec<_> = checks(&f[0], &f[0].seed())
            .into_iter()
            .map(|c| c.field)
            .collect();
        assert!(!names.iter().any(|n| n.contains("screen")));
        assert_eq!(
            names.len(),
            1 + 2 * 5 + DISC_SLOTS * 5 + TILE_CELLS * 2,
            "one check per compared field instance"
        );
    }

    #[test]
    fn a_seeded_state_that_is_perturbed_is_caught_in_schema_order() {
        let f = golden();
        let mut st = f[0].seed();
        st.tiles[3].hp += 1;
        st.players[0].world_x += 1;
        let d = first_divergence(&f[0], &st).expect("perturbation is caught");
        // players come before tiles in the schema table.
        assert_eq!(d.field, "players[0].world_x");
        assert_eq!(d.expected, 117);
        assert_eq!(d.got, 118);
    }
}
