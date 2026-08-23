//! `disc-app` -- a thin macroquad front end over [`disc_core`].
//!
//! This crate owns **no game rules**. Everything here is clock, keyboard and
//! rectangles; movement, clamping, disc steering and tile damage all live in
//! `disc-core` (see `docs/state-schema.md`). If something needed here is not
//! exposed by `disc-core`, that is a contract gap to be filed, not something to
//! reimplement locally.
//!
//! # Fixed 50 Hz
//!
//! The ST runs exactly one [`GameState::tick`] per PAL VBL. [`Clock`] therefore
//! steps the simulation in whole 1/50 s steps and never on the render clock, so
//! a run here is bit-identical to a headless run of the same input sequence.
//! The sub-tick leftover is used as a render interpolation alpha and nothing
//! derived from it is ever written back into [`GameState`].

#![forbid(unsafe_code)]

use std::mem;

use disc_core::{DirBits, GameState, Input, TILE_TYPE_DESTROYED};
use macroquad::prelude::*;

/// One PAL VBL. ST video is 50 Hz and `$8198` runs once per frame.
const TICK_SECS: f64 = 1.0 / 50.0;

/// Upper bound on the real time fed to the accumulator in one render frame.
///
/// Without it, a stall (window drag, breakpoint) hands the accumulator seconds
/// of debt and the catch-up ticks stall it further -- the spiral of death. The
/// simulation drops behind wall-clock instead, which is the correct trade.
const MAX_FRAME_SECS: f64 = 0.25;

/// World-space extent of the arena, in ST world units.
///
/// Player X is walkable 8..152 (`WALK_X_MIN`/`WALK_X_MAX`) and disc X spans
/// 0..153, so 160 covers both with a margin.
const ARENA_W: f32 = 160.0;
/// World-space height of the arena band. Player Y is a small row number
/// (`FAR_ROW_Y` is 14), so the arena is wide and short.
const ARENA_H: f32 = 32.0;

/// Columns in the debug tile layout.
///
/// ponytail: the ST's real floor geometry is not recovered -- the only evidence
/// is `grid_cell = 8 + column(world_x) + (4 if world_y > 14)` and the 145-byte
/// column table at `$7bfe`, which gives cells 9..16 and says nothing about the
/// other eight. So the 17 cells are drawn index-ordered in two rows: this is a
/// debug view of the array, not a picture of the arena floor. Replace it with
/// real geometry once `$7bfe` is decoded.
const TILE_COLS: usize = 9;

/// Fixed-timestep clock: accumulates real time and hands out whole 50 Hz ticks.
#[derive(Default)]
struct Clock {
    /// Real time owed to the simulation, always less than [`TICK_SECS`] after
    /// [`Clock::feed`] returns.
    acc: f64,
}

impl Clock {
    /// Adds `dt` seconds of real time and returns how many whole ticks to run.
    ///
    /// The remainder is kept, so the tick rate is exactly 50 Hz on average
    /// regardless of the render frame rate.
    fn feed(&mut self, dt: f64) -> u32 {
        self.acc += dt.clamp(0.0, MAX_FRAME_SECS);
        // `acc` is non-negative and bounded, so the cast cannot wrap.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (self.acc / TICK_SECS) as u32;
        self.acc -= f64::from(ticks) * TICK_SECS;
        ticks
    }

    /// How far the render is between the last tick and the next, in `0.0..1.0`.
    ///
    /// Render-only. Never feed this back into [`GameState`].
    #[allow(clippy::cast_possible_truncation)]
    fn alpha(&self) -> f32 {
        (self.acc / TICK_SECS) as f32
    }
}

/// Keyboard state for player 1, latched between ticks.
///
/// Fire is an EDGE, mirroring the ST's `bclr #7,(a0)` at `$f606`/`$f81a`/
/// `$fb90`: it is true for exactly one tick per physical key press. The latch
/// is what makes that survive a render frame rate that differs from 50 Hz --
/// a press during a render frame that runs no tick is remembered, and a render
/// frame that runs three ticks spends the edge on the first of them only.
#[derive(Default)]
struct Pad {
    /// Direction bits as of the last poll; a level, not an edge.
    dir: DirBits,
    /// A key-down seen since the last tick consumed one.
    fire_pending: bool,
}

impl Pad {
    /// Reads the keyboard. Called once per *render* frame.
    fn poll(&mut self) {
        let mut dir = DirBits::NONE;
        if is_key_down(KeyCode::Up) {
            dir = dir.or(DirBits::UP);
        }
        if is_key_down(KeyCode::Down) {
            dir = dir.or(DirBits::DOWN);
        }
        if is_key_down(KeyCode::Left) {
            dir = dir.or(DirBits::LEFT);
        }
        if is_key_down(KeyCode::Right) {
            dir = dir.or(DirBits::RIGHT);
        }
        self.dir = dir;

        // `is_key_pressed` is already the key-down edge; latch it so no press
        // is lost between ticks. Holding the key never sets it again.
        if is_key_pressed(KeyCode::Space) {
            self.fire_pending = true;
        }
    }

    /// Consumes one tick's worth of input, clearing the fire edge.
    fn take(&mut self) -> Input {
        Input {
            dir: self.dir,
            fire_edge: mem::take(&mut self.fire_pending),
        }
    }
}

/// Linear interpolation between two ST integer coordinates, for rendering only.
fn lerp(from: i16, to: i16, t: f32) -> f32 {
    let (from, to) = (f32::from(from), f32::from(to));
    from + (to - from) * t
}

/// Where the arena sits on screen: origin in pixels and pixels per world unit.
fn view() -> (f32, f32, f32) {
    let scale = (screen_width() / ARENA_W).min(screen_height() * 0.5 / ARENA_H);
    let ox = (screen_width() - ARENA_W * scale) * 0.5;
    (ox, 48.0, scale)
}

/// Draws one frame. `prev` and `cur` are the states either side of the current
/// sub-tick position `alpha`; neither is modified.
fn draw(prev: &GameState, cur: &GameState, alpha: f32) {
    clear_background(Color::new(0.05, 0.05, 0.08, 1.0));
    let (ox, oy, s) = view();

    // Arena bounds.
    draw_rectangle_lines(ox, oy, ARENA_W * s, ARENA_H * s, 2.0, DARKGRAY);

    // The 17 floor cells. Destroyed (`tile_type == 0`) reads as an empty hole.
    let tile_y = oy + ARENA_H * s + 24.0;
    let cell = (ARENA_W * s / TILE_COLS as f32).min(40.0);
    for (i, tile) in cur.tiles.iter().enumerate() {
        let x = ox + (i % TILE_COLS) as f32 * cell;
        let y = tile_y + (i / TILE_COLS) as f32 * cell;
        let pad = cell * 0.08;
        if tile.tile_type == TILE_TYPE_DESTROYED {
            draw_rectangle_lines(
                x + pad,
                y + pad,
                cell - 2.0 * pad,
                cell - 2.0 * pad,
                1.0,
                MAROON,
            );
        } else {
            // Two live types, {1, 2}; shade by HP so damage is visible.
            let wear = (f32::from(tile.hp).max(0.0) / 8.0).min(1.0);
            let base = if tile.tile_type == 1 { 0.35 } else { 0.20 };
            draw_rectangle(
                x + pad,
                y + pad,
                cell - 2.0 * pad,
                cell - 2.0 * pad,
                Color::new(base, 0.30 + 0.45 * wear, 0.55, 1.0),
            );
        }
        draw_text(i.to_string(), x + pad + 3.0, y + pad + 13.0, 14.0, GRAY);
    }

    // The two players, interpolated between ticks.
    for (i, (p, q)) in prev.players.iter().zip(&cur.players).enumerate() {
        let x = ox + lerp(p.world_x, q.world_x, alpha) * s;
        let y = oy + lerp(p.world_y, q.world_y, alpha) * s;
        let colour = if i == 0 { SKYBLUE } else { ORANGE };
        draw_rectangle(x - 3.0 * s, y, 6.0 * s, 10.0 * s, colour);
        // A notch on the facing side; 1 = left, 2 = right (`$6ca9`).
        let notch = if q.facing == disc_core::FACING_LEFT {
            x - 4.5 * s
        } else {
            x + 3.0 * s
        };
        draw_rectangle(notch, y + 2.0 * s, 1.5 * s, 3.0 * s, WHITE);
    }

    // Active discs. Screen X/Y are excluded from core state on purpose (the ST
    // projects them through LUTs at `$a6b2`/`$a6b6`), so depth is shown as size
    // rather than reimplemented as a projection.
    for (p, q) in prev.discs.iter().zip(&cur.discs) {
        if !q.active {
            continue;
        }
        let x = ox + lerp(p.world_x, q.world_x, alpha) * s;
        let y = oy + lerp(p.world_y, q.world_y, alpha) * s;
        let size = (2.0 + lerp(p.world_z, q.world_z, alpha).abs() * 0.05).min(6.0) * s;
        draw_rectangle(x - size * 0.5, y - size * 0.5, size, size, YELLOW);
    }

    draw_text(
        format!(
            "frame {}  alpha {alpha:.2}  discs {}  render {:.0} fps  [arrows: move  space: fire  p1 only]",
            cur.frame,
            cur.discs.iter().filter(|d| d.active).count(),
            get_fps(),
        )
        .as_str(),
        12.0,
        24.0,
        18.0,
        LIGHTGRAY,
    );
}

#[macroquad::main("Disc (1990, Loriciel) -- disc-core front end")]
async fn main() {
    let mut cur = GameState::default();
    let mut prev = cur.clone();
    let mut clock = Clock::default();
    let mut pad = Pad::default();

    loop {
        pad.poll();

        for _ in 0..clock.feed(f64::from(get_frame_time())) {
            prev = cur.clone();
            // Player 2 is inert: the opponent AI is an open unknown, discr-b6x.
            let _events = cur.tick([pad.take(), Input::default()]);
        }

        draw(&prev, &cur, clock.alpha());
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use disc_core::TILE_CELLS;

    use super::*;

    /// A render clock that is neither 50 Hz nor even must still produce exactly
    /// the ticks the elapsed real time is worth, and no others.
    #[test]
    fn accumulator_runs_one_tick_per_fiftieth_of_a_second() {
        let mut clock = Clock::default();
        // Ragged frame times, none of them a whole tick.
        let dts = [0.003_f64, 0.031, 0.0004, 0.0176, 0.009, 0.0125];
        let total: f64 = dts.iter().sum();
        let ticks: u32 = dts.iter().map(|&dt| clock.feed(dt)).sum();

        assert_eq!(ticks, (total / TICK_SECS) as u32);
        assert!(clock.acc < TICK_SECS, "remainder must stay sub-tick");
        assert!((0.0..1.0).contains(&clock.alpha()));
    }

    /// The whole point of the accumulator: what you get out of `tick` cannot
    /// depend on how the real time arrived.
    #[test]
    fn stepping_through_the_clock_is_bit_identical_to_a_headless_run() {
        let script = |n: u32| Input {
            dir: if n.is_multiple_of(3) {
                DirBits::RIGHT
            } else {
                DirBits::LEFT
            },
            fire_edge: n.is_multiple_of(7),
        };

        // Headless: 100 plain ticks.
        let mut headless = GameState::default();
        for n in 0..100 {
            headless.tick([script(n), Input::default()]);
        }

        // Through the clock, fed lumpy render frames until 100 ticks have run.
        let mut through = GameState::default();
        let mut clock = Clock::default();
        let mut run = 0_u32;
        let mut dt = 0.001_f64;
        while run < 100 {
            for _ in 0..clock.feed(dt) {
                if run == 100 {
                    break;
                }
                through.tick([script(run), Input::default()]);
                run += 1;
            }
            dt = (dt * 1.7).min(0.2);
        }

        assert_eq!(headless, through);
    }

    /// Fire mirrors `bclr #7`: one edge per press, however long it is held and
    /// however the render frames line up against the ticks.
    #[test]
    fn fire_is_an_edge_not_a_level() {
        // Held down across many polls -- `is_key_pressed` only latches once.
        let mut pad = Pad {
            fire_pending: true,
            ..Default::default()
        };
        assert!(pad.take().fire_edge, "the press frame fires");
        assert!(!pad.take().fire_edge, "holding must not fire again");
        assert!(!pad.take().fire_edge);

        // A press landing on a render frame that runs three catch-up ticks is
        // spent on the first of them only.
        pad.fire_pending = true;
        let edges = [pad.take(), pad.take(), pad.take()];
        assert_eq!(edges.iter().filter(|i| i.fire_edge).count(), 1);
    }

    #[test]
    fn a_stall_cannot_spiral() {
        let mut clock = Clock::default();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cap = (MAX_FRAME_SECS / TICK_SECS) as u32;
        assert_eq!(clock.feed(30.0), cap, "a 30 s stall owes at most the cap");
    }

    #[test]
    fn the_debug_tile_layout_covers_every_cell() {
        assert!(TILE_CELLS.div_ceil(TILE_COLS) * TILE_COLS >= TILE_CELLS);
    }
}
