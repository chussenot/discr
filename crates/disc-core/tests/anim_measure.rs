//! Standalone measurement for bead `discr-rxx.1`.
//!
//! Answers one question: if `player+$3a` (`anim_cursor`), `player+$1a`
//! (`x_delta`) and `player+$1c`..`+$22` (`hit_box`) were *not* fed every tick
//! -- if `disc-core` reconstructed them purely from the animation-table decode
//! in `player.rs` and the state transitions it already computes -- would the
//! six committed fixtures still hold?
//!
//! This is a **copy** of `disc-tools/src/main.rs`'s trace-replay machinery
//! (`Frame`/`TracePlayer`/`TraceDisc`/`seed`/`passes`/`feed_disc_inputs`), not
//! a call into it: `main.rs` is owned by another agent for the duration of
//! this measurement, and copying its (already-proven) replay logic verbatim
//! is safer than reimplementing it from scratch. The only two changes from
//! the original are (1) `feed_disc_inputs` here does **not** feed the three
//! target fields, and (2) the comparison additionally tracks them, which is
//! exactly the two edits `main.rs`/`docs/state-schema.md` need once this
//! measures 100%. Everything else -- seeding, per-pass input, the frame
//! order, what stays fed -- is unchanged, so a divergence here is a fact
//! about the reconstruction, not an artefact of a shortcut in this harness.
//!
//! Run with `ANIM_DIAG=1` for a per-mismatch dump.

use disc_core::player::{NO_CELL, idle_anim};
use disc_core::{
    DISC_SLOTS, DirBits, DiscSlot, GameState, Input, Player, PlayerId, SteerHook, TILE_CELLS, Tile,
};
use serde::Deserialize;

/// `disc-tools`' real `seed()` hardcodes `anim_cell`/`anim_hold` to `0` at
/// frame 0 because those two fields have no trace column of their own -- a
/// harmless simplification while `anim_cursor` was fed every tick regardless,
/// but it silently assumes frame 0 catches the idle sequence at its FIRST
/// cell, which `golden.ndjson`'s own frame 0 (`anim` = 11408 = `$2c90`, cell 4
/// of `ANIM_P1_IDLE`, one of the 48-hold pauses) shows is not true here.
/// `anim_cursor` (`t.anim`) HAS a trace column at frame 0, and every cursor
/// this crate's tables produce is `base + 6*cell`, so the starting cell (and
/// a first-cell hold estimate) is recoverable from it instead of assumed.
/// This is the one seeding fix this measurement needs beyond the two
/// deliberate diffs from `main.rs` the module doc names.
fn seed_anim_cell(idle: disc_core::player::Anim, cursor: u32) -> (u8, u16, u8) {
    if cursor >= idle.start {
        let delta = cursor - idle.start;
        if delta.is_multiple_of(6)
            && let Some(&hold) = idle.holds.get((delta / 6) as usize)
        {
            return ((delta / 6) as u8, hold, NO_CELL);
        }
    }
    // Not on the idle table at all (a trace seeded mid-sequence elsewhere) --
    // fall back to cell 0, same as `main.rs` today.
    (0, idle.holds.first().copied().unwrap_or(1), NO_CELL)
}

fn one() -> u16 {
    1
}

#[derive(Deserialize)]
struct TracePlayer {
    x: i16,
    y: i16,
    facing: u8,
    state: u8,
    cell: u16,
    #[serde(default)]
    anim: u32,
    #[serde(default)]
    throw_dk: i16,
    #[serde(default)]
    throw_mag: i16,
    #[serde(default, rename = "box")]
    hit_box: [i16; 4],
    #[serde(default)]
    energy: i16,
    #[serde(default)]
    reach: i16,
    #[serde(default)]
    discs_out: i16,
    #[serde(default)]
    disc_cap: i16,
    #[serde(default)]
    x_delta: i16,
}

#[derive(Deserialize)]
struct TraceDisc {
    wx: i16,
    wy: i16,
    wz: i16,
    vx: i16,
    flag: u16,
    #[serde(default)]
    vy: i16,
    #[serde(default)]
    act: Option<u8>,
    #[serde(default)]
    own: Option<u8>,
    #[serde(default)]
    hook: Option<u32>,
    #[serde(default)]
    dmg: Option<i16>,
}

fn steer_hook(raw: u32) -> SteerHook {
    match raw {
        0 => SteerHook::None,
        0xa71a => SteerHook::AtP1,
        0xa78e => SteerHook::AtP1Wide,
        0xa7d8 => SteerHook::AtP2Wide,
        0xa816 => SteerHook::AtP2Deep,
        other => panic!("trace has an unknown disc+$12 hook: ${other:x}"),
    }
}

#[derive(Deserialize)]
struct Frame {
    frame: u32,
    vbl_6ab4: u16,
    joy_6c58: u8,
    #[serde(default = "one")]
    updates: u16,
    #[serde(default)]
    pass_joy: Vec<u8>,
    #[serde(default)]
    pass_ai: Vec<u8>,
    #[serde(default = "one")]
    outer: u16,
    #[serde(default)]
    ai_6da1: u8,
    player: [TracePlayer; 2],
    disc: [TraceDisc; DISC_SLOTS],
    #[serde(deserialize_with = "grid_column")]
    grid: [(u16, i16); TILE_CELLS],
    #[serde(default)]
    banks: Vec<(u16, i16)>,
}

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

impl Frame {
    fn passes(&self, prev_joy: u8, prev_ai: u8) -> Vec<[Input; 2]> {
        let n = usize::from(self.updates);
        let mut out = Vec::with_capacity(n);
        let (mut pj, mut pa) = (prev_joy, prev_ai);
        for k in 0..n {
            let j = self.pass_joy.get(k).copied().unwrap_or(self.joy_6c58);
            let a = self.pass_ai.get(k).copied().unwrap_or(self.ai_6da1);
            out.push([
                Input {
                    dir: DirBits(j & 0x0f),
                    fire_edge: j & 0x80 != 0 && pj & 0x80 == 0,
                    fire_held: j & 0x80 != 0,
                },
                Input {
                    dir: DirBits(a & 0x0f),
                    fire_edge: a & 0x80 != 0 && pa & 0x80 == 0,
                    fire_held: a & 0x80 != 0,
                },
            ]);
            pj = j;
            pa = a;
        }
        out
    }

    fn last_bytes(&self, prev_joy: u8, prev_ai: u8) -> (u8, u8) {
        (
            self.pass_joy.last().copied().unwrap_or(prev_joy),
            self.pass_ai.last().copied().unwrap_or(prev_ai),
        )
    }

    /// Identical to `disc-tools`' `Frame::seed`, field for field.
    fn seed(&self) -> GameState {
        let mut st = GameState {
            frame: u32::from(self.vbl_6ab4),
            updates: self.updates,
            ..GameState::default()
        };
        for (i, (p, t)) in st.players.iter_mut().zip(&self.player).enumerate() {
            let idle = idle_anim(if i == 0 { PlayerId::One } else { PlayerId::Two });
            let (anim_cell, anim_hold, anim_shown) = seed_anim_cell(idle, t.anim);
            *p = Player {
                world_x: t.x,
                world_y: t.y,
                facing: t.facing,
                state_index: t.state,
                grid_cell: t.cell,
                pending_state: 0,
                anim_hold,
                anim_cell,
                anim_shown,
                anim_base: idle.start,
                anim_cursor: t.anim,
                throw_dir_kind: t.throw_dk,
                throw_damage: t.throw_mag,
                hit_box: t.hit_box,
                energy: t.energy,
                reach: t.reach,
                discs_out: t.discs_out,
                disc_cap: t.disc_cap,
                x_delta: t.x_delta,
                threw_left: false,
                round_over: false,
                down: false,
            };
        }
        for (d, t) in st.discs.iter_mut().zip(&self.disc) {
            *d = DiscSlot {
                active: t.act.unwrap_or(if t.flag != 0 { 0xff } else { 0 }),
                aim: match t.own {
                    Some(0) | None => PlayerId::Two,
                    Some(_) => PlayerId::One,
                },
                hook: t.hook.map_or(SteerHook::None, steer_hook),
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
        for (tile, &(tile_type, hp)) in st.tiles_far.iter_mut().zip(&self.banks) {
            *tile = Tile { tile_type, hp };
        }
        st
    }
}

/// `disc-tools`' `feed_disc_inputs`, minus the three lines this bead retires
/// for **player 1 only**. Player 2's copies stay fed, the same shape as the
/// ORIGINAL five fields' own split (`players[0].*` compared, `players[1].*`
/// waived:discr-b6x): this crate's player 2 state machine does not yet cover
/// every sequence player 2 runs (a sixth, uncatalogued table surfaces at
/// `$449e` within the first few ticks of `golden.ndjson` alone), and
/// `disc::THROW_STATES`' release gate reads player 2's `anim_cursor`
/// directly -- so an unfed, wrong p2 cursor desyncs the serve and
/// contaminates the disc simulation for BOTH players, which is a player-2
/// state-machine gap (`discr-75o`/`discr-b6x`), not a cell-format one.
fn feed_disc_inputs_minus_anim(state: &mut GameState, want: &GameState) {
    for (s, w) in state.players.iter_mut().zip(&want.players) {
        s.throw_dir_kind = w.throw_dir_kind;
        s.throw_damage = w.throw_damage;
        s.reach = w.reach;
        s.disc_cap = w.disc_cap;
    }
    // Player 2 (index 1): keep feeding the three target fields, same as
    // `disc-tools` does today.
    let (s2, w2) = (&mut state.players[1], &want.players[1]);
    s2.anim_cursor = w2.anim_cursor;
    s2.hit_box = w2.hit_box;
    s2.x_delta = w2.x_delta;

    for (s, w) in state.discs.iter_mut().zip(&want.discs) {
        s.aim = w.aim;
    }
}

/// One compared value, `main.rs`'s `Check` plus a `waived` marker so the
/// report can say whether a mismatch is one of the schema's existing waivers
/// (player 2's whole row set, `discr-b6x`) or a real regression.
struct Check {
    field: String,
    expected: i64,
    got: i64,
    waived: bool,
}

/// `main.rs`'s `checks()`, unmodified, PLUS the three fields this bead adds:
/// `anim_cursor`, `x_delta`, and the four `hit_box` words. Same field order
/// convention (frame, then players, then discs, then tiles).
fn checks(expected: &Frame, got: &GameState) -> Vec<Check> {
    let mut v = Vec::new();
    let mut push = |field: String, e: i64, g: i64, waived: bool| {
        v.push(Check {
            field,
            expected: e,
            got: g,
            waived,
        });
    };

    push(
        "frame".into(),
        i64::from(expected.vbl_6ab4),
        i64::from(got.frame as u16),
        false,
    );

    for (n, (e, g)) in expected.player.iter().zip(&got.players).enumerate() {
        let p2 = n == 1;
        push(
            format!("players[{n}].world_x"),
            e.x.into(),
            g.world_x.into(),
            p2,
        );
        push(
            format!("players[{n}].world_y"),
            e.y.into(),
            g.world_y.into(),
            p2,
        );
        push(
            format!("players[{n}].facing"),
            e.facing.into(),
            g.facing.into(),
            p2,
        );
        push(
            format!("players[{n}].state_index"),
            e.state.into(),
            g.state_index.into(),
            p2,
        );
        push(
            format!("players[{n}].grid_cell"),
            e.cell.into(),
            g.grid_cell.into(),
            p2,
        );
        push(
            format!("players[{n}].energy"),
            e.energy.into(),
            g.energy.into(),
            p2,
        );
        // discr-rxx.1: the three fields this measurement is about.
        push(
            format!("players[{n}].anim_cursor"),
            i64::from(e.anim),
            i64::from(g.anim_cursor),
            p2,
        );
        push(
            format!("players[{n}].x_delta"),
            e.x_delta.into(),
            g.x_delta.into(),
            p2,
        );
        for k in 0..4 {
            push(
                format!("players[{n}].hit_box[{k}]"),
                e.hit_box[k].into(),
                g.hit_box[k].into(),
                p2,
            );
        }
    }

    for (n, (e, g)) in expected.disc.iter().zip(&got.discs).enumerate() {
        push(
            format!("discs[{n}].world_x"),
            e.wx.into(),
            g.world_x.into(),
            false,
        );
        push(
            format!("discs[{n}].world_y"),
            e.wy.into(),
            g.world_y.into(),
            false,
        );
        push(
            format!("discs[{n}].world_z"),
            e.wz.into(),
            g.world_z.into(),
            false,
        );
        push(
            format!("discs[{n}].vel_x"),
            e.vx.into(),
            g.vel_x.into(),
            false,
        );
        push(
            format!("discs[{n}].dir_kind"),
            i64::from(e.flag as i16),
            g.dir_kind.into(),
            false,
        );
        push(
            format!("discs[{n}].vel_y"),
            e.vy.into(),
            g.vel_y.into(),
            false,
        );
        if let Some(act) = e.act {
            push(
                format!("discs[{n}].active"),
                act.into(),
                g.active.into(),
                false,
            );
        }
        if let Some(dmg) = e.dmg {
            push(
                format!("discs[{n}].damage"),
                dmg.into(),
                g.damage.into(),
                false,
            );
        }
    }

    for (n, (&(tile_type, hp), g)) in expected.grid.iter().zip(&got.tiles).enumerate() {
        push(
            format!("tiles[{n}].tile_type"),
            tile_type.into(),
            g.tile_type.into(),
            false,
        );
        push(format!("tiles[{n}].hp"), hp.into(), g.hp.into(), false);
    }

    v
}

/// Per-fixture result: the first non-waived divergence (if any, as `(tick,
/// field, expected, got)`), and per-field `(agree, total)` for the three
/// target fields specifically -- counted only up to that first divergence,
/// since a diverged simulation's later ticks are noise (same principle as
/// `tracecheck`'s own "first mismatch, not a tally").
struct Result_ {
    first_divergence: Option<(u32, String, i64, i64)>,
    anim_cursor: (usize, usize),
    x_delta: (usize, usize),
    hit_box: (usize, usize),
    ticks_run: usize,
}

fn measure(path: &str) -> Result_ {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let frames: Vec<Frame> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{path}: {e}")))
        .collect();
    let [first, rest @ ..] = frames.as_slice() else {
        panic!("{path}: empty trace");
    };

    let mut state = first.seed();
    // `anim_hold` (ST `$6ce2`) has no trace column and cannot be recovered
    // from frame 0 alone: `anim_cursor` names the cell exactly, but not how
    // many ticks are left on it -- `golden.ndjson`'s own frame 0 sits on
    // `ANIM_P1_IDLE` cell 4 (hold 48) with only 7 ticks left on it, which
    // `seed_anim_cell`'s "freshly entered" guess gets wrong by 41. This is a
    // seeding-only gap (once a real `enter_anim` fires, the hold it loads is
    // exact), not a flaw in the cell-advance mechanism itself, and the whole
    // fixture is on disk already -- so the remaining hold at frame 0 is
    // recovered by counting how many leading frames share frame 0's own
    // `anim` value, which is exactly how many ticks this crate's tail has
    // left to run before it advances.
    for (i, p) in state.players.iter_mut().enumerate() {
        let start = first.player[i].anim;
        let run = frames
            .iter()
            .take_while(|f| f.player[i].anim == start)
            .count();
        if run > 0 {
            p.anim_hold = run as u16;
        }
    }
    let mut prev_joy = first.joy_6c58;
    let mut prev_ai = first.ai_6da1;
    let mut prev = first;

    let (mut cursor_agree, mut cursor_total) = (0, 0);
    let (mut xd_agree, mut xd_total) = (0, 0);
    let (mut hb_agree, mut hb_total) = (0, 0);
    let mut first_divergence = None;
    let mut ticks_run = 0;

    let diag = std::env::var("ANIM_DIAG").is_ok();

    for expected in rest {
        feed_disc_inputs_minus_anim(&mut state, &prev.seed());
        let passes = expected.passes(prev_joy, prev_ai);
        state.tick_frame(&passes, usize::from(expected.outer));
        ticks_run += 1;

        let cs = checks(expected, &state);

        // Tally the three target fields for every player, waived or not --
        // this measurement's whole point is whether they hold with NOTHING
        // waived, same bar Part 10j set for the rest of the schema.
        for c in &cs {
            if c.field.contains("anim_cursor") {
                cursor_total += 1;
                if c.expected == c.got {
                    cursor_agree += 1;
                } else if diag {
                    eprintln!(
                        "{path} tick {}: {} expected={} got={}",
                        expected.frame, c.field, c.expected, c.got
                    );
                }
            } else if c.field.contains("x_delta") {
                xd_total += 1;
                if c.expected == c.got {
                    xd_agree += 1;
                } else if diag {
                    eprintln!(
                        "{path} tick {}: {} expected={} got={}",
                        expected.frame, c.field, c.expected, c.got
                    );
                }
            } else if c.field.contains("hit_box") {
                hb_total += 1;
                if c.expected == c.got {
                    hb_agree += 1;
                } else if diag {
                    eprintln!(
                        "{path} tick {}: {} expected={} got={}",
                        expected.frame, c.field, c.expected, c.got
                    );
                }
            }
        }

        if first_divergence.is_none()
            && let Some(d) = cs.iter().find(|c| !c.waived && c.expected != c.got)
        {
            first_divergence = Some((expected.frame, d.field.clone(), d.expected, d.got));
            break;
        }

        (prev_joy, prev_ai) = expected.last_bytes(prev_joy, prev_ai);
        prev = expected;
    }

    Result_ {
        first_divergence,
        anim_cursor: (cursor_agree, cursor_total),
        x_delta: (xd_agree, xd_total),
        hit_box: (hb_agree, hb_total),
        ticks_run,
    }
}

fn fixture(name: &str) -> String {
    format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn report(name: &str) {
    let r = measure(&fixture(name));
    println!(
        "{name}: {} tick(s) run; anim_cursor {}/{}, x_delta {}/{}, hit_box {}/{}",
        r.ticks_run,
        r.anim_cursor.0,
        r.anim_cursor.1,
        r.x_delta.0,
        r.x_delta.1,
        r.hit_box.0,
        r.hit_box.1
    );
    match &r.first_divergence {
        Some((frame, field, e, g)) => {
            println!("  first non-waived divergence: frame {frame} {field} expected={e} got={g}");
        }
        None => println!("  no non-waived divergence -- ran the whole fixture"),
    }
}

#[test]
fn measure_all_fixtures() {
    for f in [
        "golden.ndjson",
        "tile_damage.ndjson",
        "p1_walk.ndjson",
        "handover.ndjson",
        "bonus.ndjson",
        "farbank.ndjson",
    ] {
        report(f);
    }
}
