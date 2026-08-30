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
//! # Waived rows and `--skip-waived`
//!
//! `docs/state-schema.md` marks `players[1].*` `waived:discr-b6x`. Those rows
//! *cannot* match: `disc-core` takes player 2's `Input` from its caller and the
//! trace carries no `$6c59` column, so p2 stands still while the ST walks it.
//! By default they are still compared, and the run therefore always stops on
//! frame 1 -- which hides every number worth having.
//!
//! `--skip-waived` **resyncs** the waived rows from the trace after each tick
//! instead of comparing them, so a waived row can neither fail nor poison the
//! next tick, and the run continues to the first divergence among `compared`
//! rows. The default is unchanged and both modes say which one is in force.
//!
//! `--resync <FIELD>` extends that to any field path, for measuring past a
//! divergence that already has an owning bead. `--min-agree <N>` turns the
//! measured prefix into a regression gate -- exit 0 at or above `N` matched
//! ticks, still printing the divergence -- the same idiom
//! `scripts/oracle_diff.py` uses for the oracle's 275-frame boundary.
//!
//! # Input decoding (ST `$6c58`)
//!
//! Direction bits are levels -- `$01` up, `$02` down, `$04` left, `$08` right.
//! Fire (`$80`) is **edge**-consumed: `bclr #7,(a0)` at `$f606` / `$f81a` /
//! `$fb90` clears it on use, so `fire_edge` is true only on the frame the bit
//! goes 0 -> 1, never while it is held.
//!
//! Player 2 is waived (`discr-b6x`): `disc-core` takes both players' input from its
//! caller and the trace carries no `$6c59`, so p2 is driven with no input and
//! a p2-only divergence is expected rather than a bug.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use disc_core::{DISC_SLOTS, DirBits, DiscSlot, GameState, Input, Player, TILE_CELLS, Tile};
use serde::Deserialize;

/// `docs/state-schema.md`, "Compared fields": 18 rows marked `compared`.
const SCHEMA_COMPARED: usize = 18;
/// `docs/state-schema.md`, "Waived and excluded": 17 rows marked `waived:`.
const SCHEMA_WAIVED: usize = 17;
/// `docs/state-schema.md`, "Waived and excluded": 5 rows marked `excluded:`.
const SCHEMA_EXCLUDED: usize = 5;
/// serde default for [`Frame::updates`]: a trace recorded before Part 11f has no
/// column, and every such trace ran exactly one pass per frame.
const fn one() -> u16 {
    1
}

/// The trace a bare invocation replays: the committed golden fixture, which is
/// the one `mise run core-check` gates on first and the only one guaranteed to
/// be present in a clean clone.
const DEFAULT_TRACE: &str = "tests/fixtures/golden.ndjson";

/// Compared rows a trace may carry no column for; see [`Frame`].
///
/// Part 10 added `vy` and `dmg` to the oracle's emitter, so a freshly generated
/// trace has both and this list is empty for it. A trace recorded before that
/// still loads -- the columns default -- and is reported as missing them, which
/// is why this is computed per trace rather than being a constant.
fn not_in_trace(f: &Frame) -> Vec<&'static str> {
    let mut v = Vec::new();
    if f.disc.iter().all(|d| d.dmg.is_none()) {
        v.push("discs[n].damage (disc+$16)");
    }
    v
}

/// The `waived:` rows of `docs/state-schema.md` that name a field path this
/// tool builds a [`Check`] for, as `(field-path prefix, bead)`.
///
/// The other eleven waived rows are `--` rows: ST behaviour `disc-core` does
/// not model at all, with no field of its own to resync. They shorten the run
/// (see `reports/core-report.md`) but there is nothing here to skip.
const WAIVED: [(&str, &str); 1] = [("players[1].", "discr-b6x")];

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
    /// NDJSON trace to replay. Defaults to the golden fixture, so a bare
    /// `cargo run -p disc-tools` replays it rather than failing on a missing
    /// argument -- the run's own header always says which trace it read.
    #[arg(default_value = DEFAULT_TRACE)]
    trace: PathBuf,
    /// Stop after this many ticks instead of running the whole trace.
    #[arg(long, value_name = "N")]
    frames: Option<usize>,
    /// Resync the schema's waived rows from the trace each tick instead of
    /// comparing them, so the run reaches the first divergence among the
    /// `compared` rows. Off by default.
    #[arg(long)]
    skip_waived: bool,
    /// Also resync any field whose path starts with this, e.g.
    /// `players[0].state_index`. Repeatable. For measuring past a divergence
    /// that already has an owning bead.
    #[arg(long, value_name = "FIELD")]
    resync: Vec<String>,
    /// Exit 0 when at least N ticks matched, still printing the divergence.
    /// Without it any divergence exits 1.
    #[arg(long, value_name = "N")]
    min_agree: Option<usize>,
    /// Print every compared field whose path starts with this, ST value against
    /// `disc-core`'s, for each tick in the range given by `--from`/`--frames`.
    ///
    /// For localising a divergence rather than measuring one: a lag shows up as
    /// a column that is right but shifted, which a first-divergence report
    /// cannot distinguish from a wrong rule.
    #[arg(long, value_name = "FIELD")]
    dump: Option<String>,
    /// Start dumping at this tick. Only meaningful with `--dump`.
    #[arg(long, value_name = "N", default_value_t = 0)]
    from: usize,
}

impl Cli {
    /// The bead a field is waived under, or `None` if it is compared.
    ///
    /// `--resync` paths report as `--resync` rather than inventing a bead.
    fn waiver(&self, field: &str) -> Option<&str> {
        if self.skip_waived
            && let Some((_, bead)) = WAIVED.iter().find(|(p, _)| field.starts_with(p))
        {
            return Some(bead);
        }
        self.resync
            .iter()
            .any(|p| field.starts_with(p.as_str()))
            .then_some("--resync")
    }
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
    /// How many update passes ran between the previous sample and this one.
    /// Part 11f. Defaults to 1 so a pre-Part-11f trace replays as it used to.
    #[serde(default = "one")]
    updates: u16,
    /// `$6c58` as each pass consumed it, in order. Part 11g.
    #[serde(default)]
    pass_joy: Vec<u8>,
    /// `$6da1` as each pass consumed it. Part 11g.
    #[serde(default)]
    pass_ai: Vec<u8>,
    /// How many iterations of the OUTER main loop ran between the previous
    /// sample and this one -- `$96b6`, above the `$96be` repeat target, so it is
    /// not `updates` and it is not 1. Part 11h. Defaults to 1 so a pre-11h
    /// trace replays as it used to.
    #[serde(default = "one")]
    outer: u16,
    /// ST `$6da1`, the byte the one-player AI at `$d2cc` synthesises in place
    /// of player 2's joystick, consumed by `$abb2` at exactly the position
    /// `$6c59` occupies for a human. Part 10.
    ///
    /// Player 2's *policy* is waived (discr-b6x) and always will be until the
    /// 20-rule table at `$efa8` is decoded. Feeding the byte it produces is the
    /// same thing as feeding `$6c58` for player 1: an input, not state.
    #[serde(default)]
    ai_6da1: u8,
    player: [TracePlayer; 2],
    disc: [TraceDisc; DISC_SLOTS],
    /// 16 cells of `[tile_type, hp]` from `$7616` -- the near bank.
    ///
    /// The three committed fixtures predate Part 10e's discovery that a bank
    /// is 16 cells and carry a 17-pair column; the 17th pair is the first
    /// word past the bank's end (`$7696`, which reads `(1,1)` -- it is not a
    /// tile). That pair is parsed and DROPPED here so pre-discr-ovl.5 traces
    /// still load; a regenerated fixture emits exactly 16
    /// (`oracle/disc-oracle.c`). Only the 16 real cells are seeded, compared
    /// or resynced.
    #[serde(deserialize_with = "grid_column")]
    grid: [(u16, i16); TILE_CELLS],
    /// Both banks, 16 cells each: `$7596` then `$7616`. Part 10e.
    #[serde(default)]
    banks: Vec<(u16, i16)>,
}

/// Deserialize [`Frame::grid`]: exactly 16 pairs from a regenerated trace, or
/// 17 from a committed pre-discr-ovl.5 one, whose trailing non-tile pair is
/// dropped. Any other width is a malformed trace, not a tolerable variant.
fn grid_column<'de, D>(de: D) -> Result<[(u16, i16); TILE_CELLS], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let pairs = Vec::<(u16, i16)>::deserialize(de)?;
    if pairs.len() != TILE_CELLS && pairs.len() != TILE_CELLS + 1 {
        return Err(serde::de::Error::invalid_length(
            pairs.len(),
            &"16 tile cells (or 17 in a pre-discr-ovl.5 trace)",
        ));
    }
    let mut grid = [(0, 0); TILE_CELLS];
    grid.copy_from_slice(&pairs[..TILE_CELLS]);
    Ok(grid)
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
    /// `player+$3a`, the animation sequence cursor. Part 10b.
    #[serde(default)]
    anim: u32,
    /// `player+$6e`, the dir_kind this player's throws carry. Part 10b.
    #[serde(default)]
    throw_dk: i16,
    /// `player+$70`, the damage they do. Part 10b.
    #[serde(default)]
    throw_mag: i16,
    /// `player+$1c`..`+$22`, the hit box, copied out of the animation cell.
    /// Part 10d.
    #[serde(default)]
    #[serde(rename = "box")]
    hit_box: [i16; 4],
    /// `player+$76`, the energy a strike subtracts from. Part 10d.
    #[serde(default)]
    energy: i16,
    /// `player+$12`, how far ahead this player reaches for a disc. Part 10f.
    #[serde(default)]
    reach: i16,
    /// `player+$6a`, how many discs this player has in play. Part 10g.
    #[serde(default)]
    discs_out: i16,
    /// `player+$6c`, the cap on that count. Part 10g.
    #[serde(default)]
    disc_cap: i16,
    /// `player+$1a`, the animation-authored X delta. Part 10j.
    #[serde(default)]
    x_delta: i16,
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
    /// `disc+$08` `vel_y`. Part 10; absent from pre-Part-10 traces.
    #[serde(default)]
    vy: i16,
    /// `disc+$10`, the active byte: `$ff` live, 1..3 occupied-but-frozen, 0
    /// free. Part 10. Absent from pre-Part-10 traces, where `flag != 0` was
    /// the only liveness proxy available.
    #[serde(default)]
    act: Option<u8>,
    /// `disc+$11`, the owner byte. Part 10.
    #[serde(default)]
    own: Option<u8>,
    /// `disc+$12`, the steering hook, as a raw ST address. Part 10.
    #[serde(default)]
    hook: Option<u32>,
    /// `disc+$16`, the damage this disc does to a tile. Part 10.
    #[serde(default)]
    dmg: Option<i16>,
}

/// Map `disc+$12`'s raw ST pointer onto the enum.
///
/// Only three routines are ever installed (`docs/disc-notes.md`, Part 10); an
/// unrecognised pointer is a fact about the trace, not something to paper over,
/// so it panics rather than silently steering at nothing.
/// The inverse of [`steer_hook`], so the row can be compared as the ST word.
fn hook_raw(h: disc_core::SteerHook) -> u32 {
    match h {
        disc_core::SteerHook::None => 0,
        disc_core::SteerHook::AtP1 => 0xa71a,
        disc_core::SteerHook::AtP1Wide => 0xa78e,
        disc_core::SteerHook::AtP2Wide => 0xa7d8,
        disc_core::SteerHook::AtP2Deep => 0xa816,
    }
}

fn steer_hook(raw: u32) -> disc_core::SteerHook {
    match raw {
        0 => disc_core::SteerHook::None,
        0xa71a => disc_core::SteerHook::AtP1,
        0xa78e => disc_core::SteerHook::AtP1Wide,
        0xa7d8 => disc_core::SteerHook::AtP2Wide,
        0xa816 => disc_core::SteerHook::AtP2Deep,
        other => panic!("trace has an unknown disc+$12 hook: ${other:x}"),
    }
}

// ---------------------------------------------------------------------------
// Seeding and stepping
// ---------------------------------------------------------------------------

impl Frame {
    /// The inputs for each of this frame's update passes, in order.
    ///
    /// A frame holds `updates` passes and **each has its own pair of joystick
    /// bytes**, because `$d2cc` rewrites `$6da1` inside the repeat loop. The
    /// fire edge is computed across the flattened pass sequence, not per frame,
    /// since that is the sequence the ST saw.
    ///
    /// A trace recorded before Part 11g has no per-pass columns; it falls back to
    /// one pass carrying the frame's own bytes, which is what those traces did.
    fn passes(&self, prev_joy: u8, prev_ai: u8) -> Vec<[Input; 2]> {
        // `updates` is the authority on how many passes ran -- an EMPTY
        // pass array means "zero passes this frame" on a Part-11g trace and
        // "no such column" on an older one, and only `updates` tells them
        // apart. The bytes come from the arrays where they exist and from the
        // frame's own sample where they do not.
        let n = usize::from(self.updates);
        let mut out = Vec::with_capacity(n);
        let (mut pj, mut pa) = (prev_joy, prev_ai);
        for k in 0..n {
            let j = self.pass_joy.get(k).copied().unwrap_or(self.joy_6c58);
            let a = self.pass_ai.get(k).copied().unwrap_or(self.ai_6da1);
            out.push([
                Input {
                    dir: DirBits(j & JOY_DIR_MASK),
                    fire_edge: j & JOY_FIRE_BIT != 0 && pj & JOY_FIRE_BIT == 0,
                    fire_held: j & JOY_FIRE_BIT != 0,
                },
                Input {
                    dir: DirBits(a & JOY_DIR_MASK),
                    fire_edge: a & JOY_FIRE_BIT != 0 && pa & JOY_FIRE_BIT == 0,
                    fire_held: a & JOY_FIRE_BIT != 0,
                },
            ]);
            pj = j;
            pa = a;
        }
        out
    }

    /// The last pass's bytes, which is what the next frame's edges compare
    /// against.
    fn last_bytes(&self, prev_joy: u8, prev_ai: u8) -> (u8, u8) {
        // A frame with no passes consumed nothing, so the previous bytes stand.
        (
            self.pass_joy.last().copied().unwrap_or(prev_joy),
            self.pass_ai.last().copied().unwrap_or(prev_ai),
        )
    }

    /// Build the `GameState` this record describes.
    fn seed(&self) -> GameState {
        let mut st = GameState {
            frame: u32::from(self.vbl_6ab4),
            updates: self.updates,
            ..GameState::default()
        };
        for (i, (p, t)) in st.players.iter_mut().zip(&self.player).enumerate() {
            *p = Player {
                world_x: t.x,
                world_y: t.y,
                facing: t.facing,
                state_index: t.state,
                grid_cell: t.cell,
                // `player+$0a` and `player+$42` have no trace column. They are
                // carried across ticks by disc-core rather than reseeded, so
                // these only set the value at frame 0 -- correct for a trace
                // that starts with the player idle, which both fixtures do.
                // A trace seeded mid-turn would need the columns.
                pending_state: 0,
                anim_hold: 0,
                // `player+$3a`'s cell index has no trace column; both fixtures
                // start with the player idle, so cell 0 is right at frame 0.
                anim_cell: 0,
                anim_shown: 0,
                // Both fixtures start with each player idle, so the sequence
                // running at frame 0 is the idle one.
                anim_base: disc_core::player::idle_anim(if i == 0 {
                    disc_core::PlayerId::One
                } else {
                    disc_core::PlayerId::Two
                })
                .start,
                anim_cursor: t.anim,
                throw_dir_kind: t.throw_dk,
                throw_damage: t.throw_mag,
                hit_box: t.hit_box,
                energy: t.energy,
                reach: t.reach,
                discs_out: t.discs_out,
                disc_cap: t.disc_cap,
                x_delta: t.x_delta,
                // `player+$08` records which way the last throw stepped; both
                // fixtures start before either player has thrown.
                threw_left: false,
                round_over: false,
                down: false,
            };
        }
        for (d, t) in st.discs.iter_mut().zip(&self.disc) {
            *d = DiscSlot {
                // Part 10: `disc+$10` bit 7 is the ST's own liveness bit
                // ($a4f0 beq / $a534 bpl).  Pre-Part-10 traces have no `act`
                // column, and there `flag != 0` is the only proxy available.
                // `disc+$10` verbatim. A pre-Part-10 trace has no column, and
                // there a nonzero dir_kind is the only liveness proxy going.
                active: t.act.unwrap_or(if t.flag != 0 { 0xff } else { 0 }),
                // `disc+$11`. Part 12 (discr-ovl.2) settled which REAL player
                // owns which raw value: `$a9aa`/`$a9bc` (the serve routine,
                // called only from player 2's control routine `$abb2`) bump
                // `$6d8a` and clear the owner byte in the same instruction
                // pair, so a freshly served disc always reads owner 0 and is
                // charged to PLAYER 2's own throw-cap ledger. The wall
                // handlers confirm it dynamically: `tests/fixtures/
                // handover.ndjson` frame 259 (owner 0->255, the FAR wall)
                // moves `players[1]` (P2)'s `discs_out`/`disc_cap` DOWN and
                // `players[0]` (P1)'s UP in the same tick, and frame 339
                // (owner 255->0, the NEAR wall) reverses both -- exactly
                // `$6d8a--/$6d8c--/$6d0c++/$6d0a++` and its mirror, read live
                // off `$a5d0`-`$a63c`. So RAW owner 0 is PLAYER 2's disc and
                // raw 0xFF is PLAYER 1's. See reports/part12-owner.md for the
                // full chain (static + two independent trace confirmations).
                //
                // The mapping below is NOT flipped to match: `disc_core::
                // PlayerId::One`/`Two` as used for `aim` is an internal
                // boolean this crate's own wall/cascade logic (disc.rs,
                // player.rs) was written against under the OPPOSITE
                // convention (raw 0 <-> One), and `aim` is fed every tick,
                // never compared (see `feed_disc_inputs`) -- so the two
                // conventions never clash today. Flipping only this arm
                // measurably regresses `p1_walk` 274 -> 10 ticks (tried and
                // reverted; see reports/part12-owner.md), because it desyncs
                // from `disc.rs`'s and `player.rs`'s own `aim ==
                // PlayerId::One` checks, which encode the ST's raw-0 branch
                // under the CURRENT convention. A correct fix has to flip
                // this arm and every internal `PlayerId::One`/`Two` use for
                // `aim` in disc-core together; that is cross-crate and
                // tracked separately (message sent to disc.rs's and
                // player.rs's current owners; file a follow-up bead if one
                // does not already exist).
                aim: match t.own {
                    Some(0) | None => disc_core::PlayerId::One,
                    Some(_) => disc_core::PlayerId::Two,
                },
                // `disc+$12`, the steering hook. Parsed here purely as the
                // trace's OWN value for `want`/comparison -- `disc-core`
                // installs the hook itself now (Part 10f/11j, bd discr-ovl.1
                // CLOSED: both hit tests' anticipation cascades are modelled),
                // so this is no longer fed into `state`; see
                // `feed_disc_inputs` below.
                hook: t.hook.map_or(disc_core::SteerHook::None, steer_hook),
                world_x: t.wx,
                world_y: t.wy,
                world_z: t.wz,
                vel_x: t.vx,
                vel_y: t.vy,
                dir_kind: t.flag as i16,
                damage: t.dmg.unwrap_or(0),
            };
        }
        for (tile, &(tile_type, hp)) in st.tiles.iter_mut().zip(&self.grid) {
            *tile = Tile { tile_type, hp };
        }
        // The far bank, $7596's 16 cells, from the Part 10e column ($7596
        // comes first, so the zip against the 16-cell array takes exactly the
        // far bank). A trace recorded before it has none, and the array stays
        // all-zero, which makes every cell read as destroyed -- visible
        // rather than silent.
        for (tile, &(tile_type, hp)) in st.tiles_far.iter_mut().zip(&self.banks) {
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
        // Part 10d: player+$76, which the strike at $11178 subtracts from.
        push(
            format!("players[{n}].energy"),
            e.energy.into(),
            g.energy.into(),
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
        // Part 10 columns. `vy` defaults to 0 on a pre-Part-10 trace, which is
        // also what disc-core starts it at, so comparing it there is a no-op
        // rather than a false failure.
        push(format!("discs[{n}].vel_y"), e.vy.into(), g.vel_y.into());
        // Part 10g: disc+$10, whose whole life disc-core models now.
        if let Some(act) = e.act {
            push(format!("discs[{n}].active"), act.into(), g.active.into());
        }
        // Part 10f: disc+$12, which disc-core installs itself now.
        if let Some(raw) = e.hook {
            push(
                format!("discs[{n}].hook"),
                i64::from(raw),
                i64::from(hook_raw(g.hook)),
            );
        }
        if let Some(dmg) = e.dmg {
            push(format!("discs[{n}].damage"), dmg.into(), g.damage.into());
        }
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

/// Overwrite every compared field the predicate accepts with the trace's own
/// value for it, so a row nobody claims disc-core can produce neither fails
/// nor feeds the next tick.
///
/// One arm per row of the schema's "Compared fields" table, in that order.
/// `want` is `expected.seed()`: the trace record as a `GameState`.
/// Feed the ST fields that are **inputs** to the loops disc-core models rather
/// than state they produce.
///
/// `disc+$12`, the steering hook, came off this list in Part 10f: `disc-core`
/// installs it itself now, from `$c826`'s anticipation cascade.
///
/// Both are written by code outside `$a4ea` -- the two hit tests install and
/// clear hooks, and something not yet found retires a disc -- so `disc-core`
/// can no more produce them than it can produce `$6c58`. They are supplied
/// every tick for the same reason the joystick byte is, and the run says so in
/// its header. `// UNKNOWN: see bd discr-ovl.1` and `bd discr-0fm`.
fn feed_disc_inputs(state: &mut GameState, want: &GameState) {
    // `player+$3a` is driven by the animation engine ($f1c4), which this crate
    // does not model, and the serve gates on its exact value ($c06e). Feeding
    // it means the SERVE ITSELF is not fed: the trigger is the ST's own, and
    // what disc-core has to get right is the eight fields of the disc record it
    // then builds. `player+$6e` and `+$70` have no writer in the analysed image
    // at all (discr-qqt), so they come from the trace too.

    for (s, w) in state.players.iter_mut().zip(&want.players) {
        s.anim_cursor = w.anim_cursor;
        s.throw_dir_kind = w.throw_dir_kind;
        s.throw_damage = w.throw_damage;
        // `player+$1c`..`+$22` is copied out of the animation frame block every
        // frame by $f1ca, and this crate does not carry the frame blocks.
        s.hit_box = w.hit_box;
        // `player+$12` has no writer anywhere in the analysed image and never
        // moves; it is a parameter, not a decision. Part 10f traded a fed
        // `disc+$12` -- which changed 30 times across the two fixtures -- for
        // this constant.
        s.reach = w.reach;
        // `player+$6c` is never written anywhere in the analysed image either.
        s.disc_cap = w.disc_cap;
        // `player+$1a` is copied out of the animation cell, like the hit box.
        s.x_delta = w.x_delta;
    }

    // `disc+$11`, the owner byte -- the FIRST disc-side field this replay has
    // ever had to feed. p1_walk is the first trace where it moves: disc 0 reads
    // 255 from frame 268, and `disc-core` has no writer for it, so leaving it
    // at the frame-0 seed made player 1's anticipation cascade ($112f4, whose
    // third gate is `tst.b ($11,a5); beq`) unreachable for the whole trace.
    // Part 12 named which real player owns which raw value (see `seed()`
    // above and reports/part12-owner.md); disc-core still has no WRITER for
    // the four possession counters the ST moves alongside it, which is why
    // this stays fed rather than compared. See bd discr-ovl.2.
    for (s, w) in state.discs.iter_mut().zip(&want.discs) {
        s.aim = w.aim;
    }
}

fn resync(state: &mut GameState, want: &GameState, skip: &impl Fn(&str) -> bool) {
    if skip("frame") {
        state.frame = want.frame;
    }
    for n in 0..state.players.len() {
        let (s, w) = (&mut state.players[n], &want.players[n]);
        if skip(&format!("players[{n}].world_x")) {
            s.world_x = w.world_x;
        }
        if skip(&format!("players[{n}].world_y")) {
            s.world_y = w.world_y;
        }
        if skip(&format!("players[{n}].facing")) {
            s.facing = w.facing;
        }
        if skip(&format!("players[{n}].state_index")) {
            s.state_index = w.state_index;
        }
        if skip(&format!("players[{n}].grid_cell")) {
            s.grid_cell = w.grid_cell;
        }
        if skip(&format!("players[{n}].energy")) {
            s.energy = w.energy;
        }
    }
    for n in 0..DISC_SLOTS {
        let (s, w) = (&mut state.discs[n], &want.discs[n]);
        if skip(&format!("discs[{n}].world_x")) {
            s.world_x = w.world_x;
        }
        if skip(&format!("discs[{n}].world_y")) {
            s.world_y = w.world_y;
        }
        if skip(&format!("discs[{n}].world_z")) {
            s.world_z = w.world_z;
        }
        if skip(&format!("discs[{n}].vel_x")) {
            s.vel_x = w.vel_x;
        }
        if skip(&format!("discs[{n}].dir_kind")) {
            s.dir_kind = w.dir_kind;
        }
        if skip(&format!("discs[{n}].vel_y")) {
            s.vel_y = w.vel_y;
        }
        if skip(&format!("discs[{n}].hook")) {
            s.hook = w.hook;
        }
        if skip(&format!("discs[{n}].active")) {
            s.active = w.active;
        }
        if skip(&format!("discs[{n}].damage")) {
            s.damage = w.damage;
        }
    }
    for n in 0..TILE_CELLS {
        let (s, w) = (&mut state.tiles[n], &want.tiles[n]);
        if skip(&format!("tiles[{n}].tile_type")) {
            s.tile_type = w.tile_type;
        }
        if skip(&format!("tiles[{n}].hp")) {
            s.hp = w.hp;
        }
    }
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
            "  note      player 2 is waived (discr-b6x). Its INPUT is fed from the AI's own\n\
             \x20           byte at $6da1, so its rows do match for a while; what is not\n\
             \x20           modelled is the 28 states of its own table at $c6ec that this\n\
             \x20           crate has no handler for."
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
    {
        let missing = not_in_trace(&frames[0]);
        if missing.is_empty() {
            println!(
                "  comparing all {SCHEMA_COMPARED} compared rows -- this trace has every column"
            );
        } else {
            println!(
                "  comparing {} of the {SCHEMA_COMPARED} compared rows; \
                 this trace has no column for: {}",
                SCHEMA_COMPARED - missing.len(),
                missing.join(", ")
            );
        }
    }
    if cli.skip_waived {
        println!(
            "  waived rows: RESYNCED from the trace each tick, not compared -- {}",
            WAIVED
                .iter()
                .map(|(p, b)| format!("{p}* ({b})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    } else {
        println!(
            "  waived rows: COMPARED anyway -- the default, and since Part 10j it is\n               the interesting mode: {}* ({}) reproduces the whole golden fixture.\n               --skip-waived resyncs them from the trace instead, which is now a\n               weaker statement rather than a more permissive one.",
            WAIVED[0].0, WAIVED[0].1
        );
    }
    if !cli.resync.is_empty() {
        println!(
            "  also resynced (--resync, not a waiver): {}",
            cli.resync.join(", ")
        );
    }
    println!(
        "  ST inputs fed each tick, never modelled: player+$3a (the animation cursor\n         \x20              the serve gates on) and player+$1a/$1c..$22 (an X delta and the\n         \x20              hit box, both copied out of the animation cell), discr-75o;\n         \x20              player+$12/$6c/$6e/$70, four per-player constants nothing in the\n         \x20              image writes (discr-b6x, discr-qqt); updates and outer, the $96ba\n         \x20              pass count and the $96b6 outer-iteration count (Parts 11f, 11h);\n         \x20              and disc+$11, the owner byte -- polarity settled Part 12 (0 = player\n         \x20              2's disc, 0xFF = player 1's; reports/part12-owner.md), but disc-core\n         \x20              still has no WRITER for it or the four possession counters it\n         \x20              steers, so it stays fed rather than compared (discr-ovl.2)."
    );
    println!(
        "  seeded from frame {} (ST $6ab4 = {}), driving {ticks} tick(s)",
        first.frame, first.vbl_6ab4
    );

    let skip = |field: &str| cli.waiver(field).is_some();
    let mut state = first.seed();
    let mut prev_joy = first.joy_6c58;
    let mut prev_ai = first.ai_6da1;
    let mut prev = first;

    for (tick, expected) in rest[..ticks].iter().enumerate() {
        if let Some(prefix) = &cli.dump
            && tick >= cli.from
        {
            let cs = checks(prev, &state);
            let row: Vec<String> = cs
                .iter()
                .filter(|c| c.field.starts_with(prefix.as_str()))
                .map(|c| {
                    let mark = if c.expected == c.got { "" } else { " <-" };
                    format!("{}={}/{}{}", c.field, c.expected, c.got, mark)
                })
                .collect();
            println!("  tick {tick:3} in  {}", row.join("  "));
        }
        feed_disc_inputs(&mut state, &prev.seed());
        // The passes belong to the tick that PRODUCES the next sample, so they
        // come from the frame being predicted.
        let passes = expected.passes(prev_joy, prev_ai);
        // The divergence report quotes one input; the first pass's is the one a
        // reader wants, and the header says how many there were.
        let input = passes.first().map_or_else(Input::default, |p| p[0]);
        state.tick_frame(&passes, usize::from(expected.outer));
        resync(&mut state, &expected.seed(), &skip);

        let cs = checks(expected, &state);
        if let Some(r) = report(prev, expected, input, &cs) {
            let matched = expected.frame.saturating_sub(first.frame + 1) as usize;
            print!("\n{r}");
            println!("\n{matched} tick(s) matched before this one.");
            return Ok(gated(cli.min_agree, matched));
        }

        (prev_joy, prev_ai) = expected.last_bytes(prev_joy, prev_ai);
        prev = expected;
    }

    println!("\nOK: {ticks} tick(s) matched, no divergence.");
    Ok(true)
}

/// Whether a divergence after `matched` ticks still counts as a pass.
///
/// `--min-agree N` makes a known, bead-owned divergence a *boundary* instead of
/// a permanent red: the gate catches the prefix getting shorter and says so
/// when it gets longer. Same idiom, and the same reasoning, as
/// `scripts/oracle_diff.py --min-agree`.
fn gated(min_agree: Option<usize>, matched: usize) -> bool {
    let Some(n) = min_agree else { return false };
    if matched >= n {
        println!(
            "PASS: --min-agree {n} and {matched} tick(s) matched. The divergence above is\n      expected and owned by a bead; this gate fails when the prefix SHRINKS.\n      If it grew, raise the matching *_MIN_AGREE in mise.toml to {matched}."
        );
        true
    } else {
        println!("FAIL: --min-agree {n} but only {matched} tick(s) matched -- a regression.");
        false
    }
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
        // The committed fixture's grid column is 17 pairs wide; the 17th is
        // the non-tile word past the bank, parsed and dropped (discr-ovl.5).
        assert_eq!(st.tiles.len(), 16);
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
        f.updates = 1;
        f.pass_joy.clear();
        f.pass_ai.clear();
        let p1 = |prev| f.passes(prev, 0)[0][0];
        assert!(p1(0x00).fire_edge, "0 -> 1 on bit 7 is an edge");
        assert!(!p1(0x80).fire_edge, "held fire is consumed, not an edge");
        assert!(p1(0x80).fire_held, "...but it is still HELD");
        assert_eq!(p1(0x00).dir, DirBits::LEFT, "$04 is Left");
    }

    /// The edge is computed across the flattened pass sequence, because that is
    /// the sequence the ST saw: `$d2cc` rewrites `$6da1` once per pass.
    #[test]
    fn fire_edges_run_across_passes_not_frames() {
        let mut f: Frame = serde_json::from_str(F0.lines().next().unwrap()).unwrap();
        f.updates = 2;
        f.pass_ai = vec![0x80, 0x80];
        f.pass_joy = vec![0x00, 0x80];
        let p = f.passes(0x00, 0x00);
        assert_eq!(p.len(), 2, "two passes");
        assert!(p[0][1].fire_edge, "player 2's first pass is the edge");
        assert!(!p[1][1].fire_edge, "the second pass is the same press held");
        assert!(!p[0][0].fire_edge, "player 1 had not pressed yet");
        assert!(p[1][0].fire_edge, "and its edge lands on the second pass");
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
            // Part 10 added discs[n].vel_y and discs[n].damage, so 9 per disc
            // slot here (this golden trace has every column). The tile term is
            // 32 since discr-ovl.5: 16 real cells, two fields each -- the old
            // 17th pair compared a non-tile and is no longer a check at all.
            1 + 2 * 6 + DISC_SLOTS * 9 + TILE_CELLS * 2,
            "one check per compared field instance"
        );
    }

    /// The number `mise run core-check` gates on: with only the schema's waived
    /// rows resynced, `disc-core` reproduces **the whole golden fixture** --
    /// all 99 ticks, no divergence. Mirrors the loop in `run`.
    ///
    /// It was 10 before Part 10, 51 after Part 10b's state machine, 63 once the
    /// serve landed, and 99 once the hit test did.
    #[test]
    fn skip_waived_reproduces_the_whole_golden_fixture() {
        let f = golden();
        let skip = |field: &str| WAIVED.iter().any(|(p, _)| field.starts_with(p));
        let mut state = f[0].seed();
        let mut prev_joy = f[0].joy_6c58;
        let mut prev_ai = f[0].ai_6da1;

        for (matched, w) in f.windows(2).enumerate() {
            let (prev, expected) = (&w[0], &w[1]);
            feed_disc_inputs(&mut state, &prev.seed());
            state.tick_frame(
                &expected.passes(prev_joy, prev_ai),
                usize::from(expected.outer),
            );
            resync(&mut state, &expected.seed(), &skip);
            assert!(
                first_divergence(expected, &state).is_none(),
                "diverged after {matched} tick(s): {:?}",
                first_divergence(expected, &state).map(|d| d.field)
            );
            (prev_joy, prev_ai) = expected.last_bytes(prev_joy, prev_ai);
        }
        assert_eq!(f.len() - 1, 99, "the fixture is 100 frames");
    }

    /// **Resyncing buys nothing on either fixture any more.** The default run --
    /// nothing waived, nothing resynced, every compared row of *both* players --
    /// reproduces the whole of both committed traces.
    ///
    /// It stopped on frame 1 before Part 10c, 22 before 10f, 40 before state
    /// 18's handler, 59 once the animation tables landed, and 99 once player 2's
    /// idle-path throw decision did.
    #[test]
    fn nothing_waived_reproduces_the_whole_golden_fixture() {
        let f = golden();
        let mut state = f[0].seed();
        let mut prev_joy = f[0].joy_6c58;
        let mut prev_ai = f[0].ai_6da1;

        for (matched, w) in f.windows(2).enumerate() {
            let (prev, expected) = (&w[0], &w[1]);
            feed_disc_inputs(&mut state, &prev.seed());
            state.tick_frame(
                &expected.passes(prev_joy, prev_ai),
                usize::from(expected.outer),
            );
            assert!(
                first_divergence(expected, &state).is_none(),
                "diverged after {matched} tick(s): {:?}",
                first_divergence(expected, &state).map(|d| d.field)
            );
            (prev_joy, prev_ai) = expected.last_bytes(prev_joy, prev_ai);
        }
    }

    #[test]
    fn waiver_is_off_by_default_and_resync_is_reported_as_itself() {
        let bare = Cli::parse_from(["tracecheck", "t.ndjson"]);
        assert_eq!(bare.waiver("players[1].world_x"), None);

        let skipping = Cli::parse_from(["tracecheck", "t.ndjson", "--skip-waived"]);
        assert_eq!(skipping.waiver("players[1].world_x"), Some("discr-b6x"));
        assert_eq!(skipping.waiver("players[0].world_x"), None);

        let extra = Cli::parse_from([
            "tracecheck",
            "t.ndjson",
            "--resync",
            "players[0].state_index",
        ]);
        assert_eq!(extra.waiver("players[0].state_index"), Some("--resync"));
        assert_eq!(extra.waiver("players[0].world_x"), None);
    }

    /// --min-agree turns a bead-owned divergence into a boundary: it passes at
    /// or above the recorded prefix and fails when the prefix shrinks.
    #[test]
    fn min_agree_gates_on_the_prefix_not_on_being_clean() {
        assert!(gated(Some(10), 10), "at the boundary");
        assert!(gated(Some(10), 11), "past it");
        assert!(!gated(Some(10), 9), "a shorter prefix is a regression");
        assert!(!gated(None, 99), "without the flag any divergence fails");
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
