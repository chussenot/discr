//! `disc-app` -- a playable macroquad front end over [`disc_core`].
//!
//! This crate owns **no game rules**. Everything here is clock, keyboard,
//! menu/round bookkeeping and rectangles; movement, clamping, disc steering
//! and tile damage all live in `disc-core` (see `docs/state-schema.md`). If
//! something needed here is not exposed by `disc-core`, that is a contract
//! gap to be filed, not something to reimplement locally. Two exceptions,
//! both clearly labelled at their own definitions because `disc-core` itself
//! documents them as gaps it does not model:
//!
//! * [`round`] -- round/match bookkeeping. `disc-core::round`'s own module
//!   docs say round init, round-over and win/loss are "not modelled here";
//!   seeing a menu through to a match and a match through to game over
//!   (discr-13v item 1) has to happen somewhere, and core is explicit that it
//!   is not there.
//! * [`ai_fallback`] -- what player 2 does on the eighteen AI rows
//!   `disc_core::ai` does not implement (`bd discr-rxx.3`). Never consulted
//!   while the two decoded rows have an opinion; see `p2_input` below.
//! * [`MatchState::serve_workaround`] -- `disc-core`'s own auto-serve is
//!   gated on player 2's `anim_cursor`, a field nothing in the crate advances
//!   outside a replayed ST trace (`discr-b6x`), so it can never fire in a
//!   live match. The workaround calls the same public `disc::serve` with the
//!   same decoded constants; see its own doc for exactly what it does and
//!   does not take over from core.
//!
//! # Fixed 50 Hz
//!
//! The ST runs exactly one [`GameState::tick`] per PAL VBL. [`Clock`]
//! therefore steps the simulation in whole 1/50 s steps and never on the
//! render clock, so a run here is bit-identical to a headless run of the same
//! input sequence. The sub-tick leftover is used as a render interpolation
//! alpha and nothing derived from it is ever written back into [`GameState`].

#![forbid(unsafe_code)]

mod ai_fallback;
mod audio;
mod hud;
mod round;

use std::mem;

use disc_core::ai::Ai;
use disc_core::{DirBits, Event, GameState, Input, TILE_TYPE_DESTROYED, tile};
use macroquad::prelude::*;

use audio::{Cue, Sfx};
use round::{Match, Mode, Phase};

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
/// 0..153 (`disc::X_MIN`/`X_MAX`), so 160 covers both with a margin.
const ARENA_W: f32 = 160.0;

/// World-space height of the whole two-platform band players and discs are
/// drawn in. Not an ST value -- there is no recovered floor geometry (see
/// [`BANK_COLS`]'s doc) -- just enough vertical room for both banks plus the
/// throw lane between them.
const ARENA_H: f32 = 40.0;

/// A player's `world_y` is a small in-place figure (crouch/knock-down
/// travel), not a screen-scale height -- and the two players operate on
/// visibly different ranges of it. Player 1's is small
/// (`disc_core::player::STRUCK_Y_FLOOR..RISE_Y_CEILING` is 2..25); player 2's
/// measured bounds sit much higher (`RISE_Y_FLOOR_P2..STRUCK_Y_CEILING_P2` is
/// 50..69, and `disc_core::ai`'s own escape targets use rows 54/64), and this
/// crate's own `round::FRESH_WORLD_Y_P2` starts it at 78. These are the two
/// assumed rendering bands used to place each player within its own platform
/// band -- a rendering choice, not a decoded range; using player 1's narrow
/// band for both would leave player 2 pinned to one edge of its platform for
/// its entire operating range.
const PLAYER1_Y_RANGE: (i16, i16) = (0, 32);
const PLAYER2_Y_RANGE: (i16, i16) = (32, 90);

/// Columns in one tile bank's real geometry.
///
/// ponytail: the ST's exact floor geometry is not recovered, but the index
/// arithmetic IS: `disc::disc_cell` and `player`'s own `grid_cell` agree that
/// a bank's 16 stored cells are really only **8 distinct tiles** (`tile.rs`,
/// "A bank is eight tiles held twice") arranged 4 columns (`COLUMN_WIDTH`
/// spacing, `types::COLUMN_WIDTH` = 40 over `X_MIN..X_MAX`) by 2 rows (near
/// row / far row, split at each formula's own row constant). This crate
/// therefore draws indices `0..8` of each bank -- the disc-damage records --
/// as a 4x2 grid, which is the one thing about the layout that IS measured.
const BANK_COLS: usize = 4;
const BANK_ROWS: usize = 2;
/// The eight-tile half of each 16-cell bank that carries real HP: the record
/// a disc's damage path writes (`tile.rs`, "Cells 1..8 and 9..16 are the same
/// eight tiles"). The other eight are the movement code's own delayed copy,
/// which [`draw_bank`] represents via [`GameState::collapse`] instead of by
/// drawing a second, confusingly duplicate grid.
const BANK_REAL_CELLS: usize = 8;

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
    /// Fire as of the last poll: a **level**, not an edge.
    ///
    /// The ST reads the bit both ways. The walk handlers consume it with `bclr`
    /// and so see an edge; `$c1b4 btst #$7,(a0)` in player 2's intercept wants
    /// it still held, so it sees a level. `Input` carries both.
    fire_held: bool,
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
        self.fire_held = is_key_down(KeyCode::Space);
    }

    /// Consumes one tick's worth of input, clearing the fire edge.
    ///
    /// Reads only fields [`Pad::poll`] set, so it stays callable outside the
    /// macroquad runtime -- which is what the tests below need.
    fn take(&mut self) -> Input {
        let held = self.fire_held;
        Input {
            dir: self.dir,
            fire_edge: mem::take(&mut self.fire_pending),
            fire_held: held,
        }
    }
}

/// One playable match: simulation state, its predecessor for interpolation,
/// the clock, player 1's pad, player 2's AI and the round/match bookkeeping.
struct MatchState {
    cur: GameState,
    prev: GameState,
    clock: Clock,
    pad: Pad,
    ai: Ai,
    m: Match,
    paused: bool,
    /// Ticks left before [`Self::serve_workaround`] will act again. See that
    /// method's docs.
    p2_serve_cooldown: u16,
    /// The original `.SPL` samples, silent by default -- see [`Self::with_sfx`].
    sfx: Sfx,
}

/// Ticks between [`MatchState::serve_workaround`] attempts: a pacing choice
/// so a continuously-eligible AI does not fill all four disc slots in four
/// ticks. Not a decoded value -- roughly 4/5 of a second at 50 Hz.
const SERVE_COOLDOWN_TICKS: u16 = 40;

impl MatchState {
    fn new(mode: Mode) -> Self {
        let gs = Match::fresh_round_state();
        MatchState {
            cur: gs.clone(),
            prev: gs,
            clock: Clock::default(),
            pad: Pad::default(),
            ai: Ai::default(),
            m: Match::new(mode),
            paused: false,
            p2_serve_cooldown: 0,
            sfx: Sfx::default(),
        }
    }

    /// Attach the loaded original samples. A builder, not a `new` parameter,
    /// so every existing `MatchState::new` call site (all of them in
    /// `#[cfg(test)]`, which never touches macroquad's audio context) keeps
    /// working unchanged -- see [`Sfx::default`]'s doc: silent, not missing.
    fn with_sfx(mut self, sfx: Sfx) -> Self {
        self.sfx = sfx;
        self
    }

    /// Work around a `disc-core` gap that would otherwise leave a freshly
    /// simulated match with no way to ever put a disc in play.
    ///
    /// `GameState::update` already calls `disc::serve` for player 2 -- but
    /// only when `player[1].anim_cursor` exactly matches one of
    /// `disc::THROW_STATES`' four gates. `Player::anim_cursor`'s own doc
    /// (`disc-core/src/types.rs`) says plainly that player 2's copy "stays
    /// fed": nothing in `disc-core` advances it outside a replayed ST trace
    /// (`discr-b6x`). A live match has no trace to feed it from, so that gate
    /// can never open and player 2 -- the only player this game mode ever
    /// lets serve -- would never throw a single disc.
    ///
    /// This calls the exact same public `disc::serve` `GameState::update`
    /// calls, with the same standing-throw constants from
    /// `disc::THROW_STATES` (states 15/16, chosen by facing -- not the
    /// running smashes at 3/4, which wind up over several held frames this
    /// workaround does not attempt to reconstruct), and repeats the same two
    /// lines of bookkeeping `GameState::update` does on a successful serve
    /// (`state_index` to [`disc_core::disc::STATE_AFTER_THROW`],
    /// `discs_out` incremented). It calls `disc-core`'s own decoded function
    /// with `disc-core`'s own decoded constants; the only thing "worked
    /// around" is WHEN it fires, not what it does.
    ///
    /// Deliberately independent of the `Input` fed to `GameState::tick`:
    /// `movement` supplies only the sideways step / pop-up `disc::serve`
    /// itself reads off `Input::dir`, and `want_serve` is
    /// [`ai_fallback::should_serve`] read directly, never a fire bit run
    /// through core's own dispatch -- see `ai_fallback::fallback_input`'s doc
    /// for why threading a held fire through there instead breaks player 2's
    /// ordinary movement.
    fn serve_workaround(&mut self, movement: Input, want_serve: bool) {
        if self.p2_serve_cooldown > 0 {
            self.p2_serve_cooldown -= 1;
            return;
        }
        let p2 = self.cur.players[1];
        if !want_serve || p2.discs_out >= p2.disc_cap {
            return;
        }
        // disc::THROW_STATES[2] is state 15 (throw left), [3] is state 16
        // (throw right) -- see that constant's own table.
        let (_, _, x_offset, step) = if p2.facing == disc_core::FACING_LEFT {
            disc_core::disc::THROW_STATES[2]
        } else {
            disc_core::disc::THROW_STATES[3]
        };
        let mut scratch = Vec::new();
        let served = disc_core::disc::serve(
            &mut self.cur.discs,
            &p2,
            movement,
            x_offset,
            step,
            &mut scratch,
        );
        if served.is_some() {
            self.cur.players[1].state_index = disc_core::disc::STATE_AFTER_THROW;
            self.cur.players[1].discs_out += 1;
            self.p2_serve_cooldown = SERVE_COOLDOWN_TICKS;
            // Bypasses `disc-core`'s own event list (this calls `disc::serve`
            // directly, per this method's own doc) -- cue it here instead.
            self.sfx.play(Cue::Serve);
        }
    }

    /// Player 2's full input for one tick: the two decoded AI rows first,
    /// falling back to [`ai_fallback`] only when they are silent -- see that
    /// module's docs for why 0 is ambiguous and why that is an acceptable
    /// place to fall back rather than guess further inside `disc-core`.
    fn p2_input(&mut self) -> Input {
        let bits = self.ai.p2_policy(&self.cur);
        if bits != 0 {
            Input {
                dir: DirBits(bits),
                fire_edge: false,
                fire_held: false,
            }
        } else {
            ai_fallback::fallback_input(&self.cur.players[1], &self.cur.discs)
        }
    }

    /// Advance exactly one 50 Hz tick.
    fn step_tick(&mut self) {
        match self.m.phase {
            Phase::Playing => {
                self.prev = self.cur.clone();
                let p1 = self.pad.take();
                let p2 = self.p2_input();
                let events = self.cur.tick([p1, p2]);
                self.cue_core_events(&events);
                self.cue_state_edges();
                let want_serve = ai_fallback::should_serve(&self.cur.players[1]);
                self.serve_workaround(p2, want_serve);
                let was_game_over = matches!(self.m.phase, Phase::GameOver { .. });
                self.m.observe(&mut self.cur.players);
                if !was_game_over && matches!(self.m.phase, Phase::GameOver { .. }) {
                    self.sfx.play(Cue::Win);
                }
            }
            Phase::RoundOver { .. } => {
                if self.m.observe(&mut self.cur.players) {
                    let fresh = Match::fresh_round_state();
                    self.prev = fresh.clone();
                    self.cur = fresh;
                    self.ai = Ai::default();
                    self.p2_serve_cooldown = 0;
                    self.m.start_round();
                    self.sfx.play(Cue::Round);
                }
            }
            Phase::GameOver { .. } => {}
        }
    }

    /// Cue the four `disc-core` [`Event`]s this crate has a sample for. See
    /// [`Cue`]'s own doc for exactly why each mapping was picked.
    fn cue_core_events(&self, events: &[Event]) {
        for event in events {
            match event {
                Event::TileDamaged { .. } => self.sfx.play(Cue::Impact),
                Event::TileDestroyed { .. } => self.sfx.play(Cue::TileDestroyed),
                Event::DiscReflected { .. } => self.sfx.play(Cue::Block),
                // In practice never emitted live -- `GameState::update`'s own
                // auto-serve gate can't open outside a replayed trace (see
                // `serve_workaround`'s doc) -- but cued here too so a future
                // core fix that makes it fire is not silently unheard.
                Event::DiscServed { .. } => self.sfx.play(Cue::Serve),
                _ => {}
            }
        }
    }

    /// Cue the app-level state transitions `disc-core` does not surface as
    /// events -- death, fall, and a defended catch -- by comparing this
    /// tick's `state_index` against the one `self.prev` (this tick's
    /// pre-tick snapshot, see [`Self::step_tick`]) captured just before.
    fn cue_state_edges(&self) {
        use disc_core::player::{
            STATE_CATCH19, STATE_DEAD, STATE_INTERCEPT, STATE_STRUCK_DOWN, STATE_STRUCK_UP,
        };
        for (prev, cur) in self.prev.players.iter().zip(&self.cur.players) {
            let entered = |states: &[u8]| {
                states.contains(&cur.state_index) && !states.contains(&prev.state_index)
            };
            if entered(&[STATE_DEAD]) {
                self.sfx.play(Cue::Death);
            }
            if entered(&[STATE_STRUCK_DOWN, STATE_STRUCK_UP]) {
                self.sfx.play(Cue::Fall);
            }
            if entered(&[STATE_INTERCEPT, STATE_CATCH19]) {
                self.sfx.play(Cue::DefendedHit);
            }
        }
    }
}

/// Where the arena sits on screen: origin in pixels and pixels per world unit.
/// Bundled into one type so drawing functions take one parameter instead of
/// three, keeping their own argument counts under clippy's limit.
#[derive(Clone, Copy)]
struct View {
    ox: f32,
    oy: f32,
    s: f32,
}

fn view() -> View {
    let scale = (screen_width() / ARENA_W).min(screen_height() * 0.7 / ARENA_H);
    View {
        ox: (screen_width() - ARENA_W * scale) * 0.5,
        oy: (screen_height() - ARENA_H * scale) * 0.5,
        s: scale,
    }
}

/// Linear interpolation between two ST integer coordinates, for rendering only.
fn lerp(from: i16, to: i16, t: f32) -> f32 {
    let (from, to) = (f32::from(from), f32::from(to));
    from + (to - from) * t
}

/// Draw one 8-tile bank (see [`BANK_REAL_CELLS`]) at world-space `top`,
/// `height` tall, using `collapse` to show the delayed copy's own crumble
/// rather than drawing a confusing second grid for it.
fn draw_bank(
    tiles: &[disc_core::Tile; disc_core::TILE_CELLS],
    collapse: &[Option<tile::Collapse>; tile::COLLAPSE_SLOTS],
    v: View,
    top: f32,
    height: f32,
    frame: u32,
) {
    let cell_w = ARENA_W * v.s / BANK_COLS as f32;
    let cell_h = height * v.s / BANK_ROWS as f32;
    for (i, &t) in tiles.iter().enumerate().take(BANK_REAL_CELLS) {
        let col = i % BANK_COLS;
        let row = i / BANK_COLS;
        let x = v.ox + col as f32 * cell_w;
        let y = v.oy + top * v.s + row as f32 * cell_h;
        let pad = cell_w.min(cell_h) * 0.06;

        // The delayed movement-copy clear: `tile.rs`'s `WALK_COPY_OFFSET`
        // (8) is added to the struck cell to get the collapse's target, so a
        // collapse whose cell is `i + 8` is THIS tile crumbling.
        let collapsing = collapse
            .iter()
            .flatten()
            .any(|c| c.cell == i + tile::WALK_COPY_OFFSET);

        if t.tile_type == TILE_TYPE_DESTROYED && !collapsing {
            draw_rectangle_lines(
                x + pad,
                y + pad,
                cell_w - 2.0 * pad,
                cell_h - 2.0 * pad,
                1.0,
                MAROON,
            );
            continue;
        }

        if collapsing {
            // A crumbling tile: flicker between two dim reds so the 48-step
            // animation `disc-core::tile::collapse_step` runs reads as
            // motion rather than a static color. The bit checked is which of
            // core's own frame counter we are on, nothing invented in the
            // simulation.
            let flicker = if frame % 6 < 3 { 0.55 } else { 0.30 };
            draw_rectangle(
                x + pad,
                y + pad,
                cell_w - 2.0 * pad,
                cell_h - 2.0 * pad,
                Color::new(flicker, 0.12, 0.10, 1.0),
            );
            continue;
        }

        // Bit 7 of the HP word is the (never-yet-placed-by-core) bonus flag
        // -- `disc-core::tile::damage`'s own doc: "a second, unidentified
        // writer sets and later clears bit 7 of the HP word ... not modelled
        // here. See bd discr-dc0." disc-core never sets it today, so this
        // branch is currently dead in practice; it is here so the day
        // discr-dc0/discr-z8m land, this view is already correct. Mask it
        // out of the HP used for the wear shade below so a bonus tile does
        // not visually read as undamaged.
        let bonus = t.hp & 0x80 != 0;
        let hp = t.hp & 0x7f;
        let wear = (f32::from(hp).max(0.0) / 8.0).min(1.0);
        let base = if t.tile_type == 1 { 0.35 } else { 0.20 };
        draw_rectangle(
            x + pad,
            y + pad,
            cell_w - 2.0 * pad,
            cell_h - 2.0 * pad,
            Color::new(base, 0.30 + 0.45 * wear, 0.55, 1.0),
        );
        if bonus {
            draw_circle(
                x + cell_w * 0.5,
                y + cell_h * 0.5,
                cell_w.min(cell_h) * 0.18,
                GOLD,
            );
        }
    }
}

/// Where a player's world-Y places it within its own platform band, `0.0` at
/// `range.0` and `1.0` at `range.1`. See [`PLAYER1_Y_RANGE`]'s doc for why
/// this is an assumed range per player, not a decoded one.
fn y_in_band(world_y: i16, range: (i16, i16)) -> f32 {
    let (min, max) = range;
    if max <= min {
        return 0.0;
    }
    ((f32::from(world_y) - f32::from(min)) / f32::from(max - min)).clamp(0.0, 1.0)
}

/// Draws one frame. `prev` and `cur` are the states either side of the current
/// sub-tick position `alpha`; neither is modified.
fn draw_match(ms: &MatchState, alpha: f32) {
    clear_background(Color::new(0.05, 0.05, 0.08, 1.0));
    let v = view();
    let prev = &ms.prev;
    let cur = &ms.cur;

    draw_rectangle_lines(v.ox, v.oy, ARENA_W * v.s, ARENA_H * v.s, 2.0, DARKGRAY);

    // Two platforms: player 2's (far, tiles_far) above, player 1's (near,
    // tiles) below, with a throw lane between them -- discs travel from the
    // near platform's world_z 0 up to the far platform's Z_FAR, so "up the
    // screen" is "away from the camera", matching `disc::step`'s own Z_NEAR/
    // Z_FAR naming.
    let far_top = 0.0;
    let far_h = ARENA_H * 0.28;
    let near_h = ARENA_H * 0.28;
    let near_top = ARENA_H - near_h;

    draw_bank(&cur.tiles_far, &cur.collapse, v, far_top, far_h, cur.frame);
    draw_bank(&cur.tiles, &cur.collapse, v, near_top, near_h, cur.frame);

    // The two players, interpolated between ticks. Player 1 (near, index 0)
    // stands on the near platform, player 2 (far, index 1) on the far one.
    for (i, (p, q)) in prev.players.iter().zip(&cur.players).enumerate() {
        let x = v.ox + lerp(p.world_x, q.world_x, alpha) * v.s;
        let y_range = if i == 0 {
            PLAYER1_Y_RANGE
        } else {
            PLAYER2_Y_RANGE
        };
        let band_t = y_in_band(
            #[allow(clippy::cast_possible_truncation)]
            {
                lerp(p.world_y, q.world_y, alpha) as i16
            },
            y_range,
        );
        let (band_top, band_h) = if i == 0 {
            (near_top, near_h)
        } else {
            (far_top, far_h)
        };
        let y = v.oy + (band_top + band_t * band_h) * v.s;
        let colour = if i == 0 { SKYBLUE } else { ORANGE };
        let down = q.down || q.state_index == disc_core::player::STATE_DEAD;
        // A crude pose: a downed/dead player reads as a flattened, dimmer
        // rectangle rather than the standing block -- state-appropriate
        // without inventing sprite art. `q.state_index` is core's own
        // decoded state machine cursor (see `disc_core::player`'s state
        // constants); this is the one bucket cheap enough to be worth
        // drawing distinctly given the fidelity budget here.
        let (w, h, colour) = if down {
            (
                8.0 * v.s,
                4.0 * v.s,
                Color::new(colour.r, colour.g, colour.b, 0.5),
            )
        } else {
            (6.0 * v.s, 10.0 * v.s, colour)
        };
        draw_rectangle(x - w * 0.5, y - h + h.min(10.0 * v.s), w, h, colour);
        // A notch on the facing side; 1 = left, 2 = right (`$6ca9`).
        if !down {
            let notch = if q.facing == disc_core::FACING_LEFT {
                x - 4.5 * v.s
            } else {
                x + 3.0 * v.s
            };
            draw_rectangle(notch, y - 8.0 * v.s, 1.5 * v.s, 3.0 * v.s, WHITE);
        }
        // Energy bar above the player. 5 is the one measured starting value
        // on hand (`Player::energy`'s own doc; also `round::FRESH_ENERGY`).
        let bar_y = if i == 0 {
            v.oy + ARENA_H * v.s + 16.0
        } else {
            v.oy - 14.0
        };
        let bar_w = 40.0;
        let frac = (f32::from(q.energy.max(0)) / 5.0).clamp(0.0, 1.0);
        draw_rectangle_lines(x - bar_w * 0.5, bar_y, bar_w, 6.0, 1.0, GRAY);
        draw_rectangle(x - bar_w * 0.5, bar_y, bar_w * frac, 6.0, GREEN);
    }
    draw_text(
        format!("P1 {}", hud::energy_bar(cur.players[0].energy, 5, 10)),
        12.0,
        v.oy + ARENA_H * v.s + 34.0,
        14.0,
        LIGHTGRAY,
    );
    draw_text(
        format!("P2 {}", hud::energy_bar(cur.players[1].energy, 5, 10)),
        12.0,
        v.oy - 26.0,
        14.0,
        LIGHTGRAY,
    );

    // Active discs, interpolated, with near/far depth: screen X/Y are
    // excluded from core state on purpose (the ST projects them through LUTs
    // at `$a6b2`/`$a6b6`), so depth is shown as a size/position lerp against
    // `world_z` rather than reimplemented as a projection.
    for (p, q) in prev.discs.iter().zip(&cur.discs) {
        // `disc+$10`'s bit 7: the ST simulates a record only while it is set,
        // and a caught disc counts down through 3, 2, 1 with its record frozen.
        if !q.simulated() {
            continue;
        }
        let x = v.ox + lerp(p.world_x, q.world_x, alpha) * v.s;
        let z = lerp(p.world_z, q.world_z, alpha) / f32::from(disc_core::disc::Z_FAR).max(1.0);
        let z = z.clamp(0.0, 1.0);
        // z=0 (near wall) sits over the near platform's far edge (closest to
        // the throw lane), z=1 (far wall) over the far platform's near edge.
        let y_band = (near_top) - z * (near_top - (far_top + far_h));
        let wobble = (lerp(p.world_y, q.world_y, alpha) - 40.0) * 0.05;
        let y = v.oy + (y_band + wobble) * v.s;
        let size = (5.0 - z * 3.0).max(1.5) * v.s;
        draw_rectangle(x - size * 0.5, y - size * 0.5, size, size, YELLOW);
    }

    // HUD.
    draw_text(hud::score_line(&ms.m), 12.0, 20.0, 20.0, LIGHTGRAY);
    draw_text(
        format!(
            "discs out  P1 {}/{}   P2 {}/{}",
            cur.players[0].discs_out,
            cur.players[0].disc_cap,
            cur.players[1].discs_out,
            cur.players[1].disc_cap
        ),
        12.0,
        40.0,
        16.0,
        GRAY,
    );
    draw_text(
        "arrows: move   space: fire   P: pause   R: restart   Esc: menu",
        12.0,
        screen_height() - 10.0,
        14.0,
        GRAY,
    );
    if let Some(banner) = hud::phase_banner(ms.m.phase) {
        let w = banner.len() as f32 * 12.0;
        draw_rectangle(
            screen_width() * 0.5 - w * 0.5 - 10.0,
            screen_height() * 0.5 - 20.0,
            w + 20.0,
            40.0,
            Color::new(0.0, 0.0, 0.0, 0.7),
        );
        draw_text(
            banner,
            screen_width() * 0.5 - w * 0.5,
            screen_height() * 0.5 + 6.0,
            24.0,
            WHITE,
        );
    }
    if ms.paused {
        draw_text(
            "PAUSED",
            screen_width() * 0.5 - 40.0,
            screen_height() * 0.5,
            28.0,
            WHITE,
        );
    }
}

fn draw_menu(selected: Mode) {
    clear_background(Color::new(0.05, 0.05, 0.08, 1.0));
    let cx = screen_width() * 0.5;
    draw_text(
        hud::menu_title(),
        cx - 40.0,
        screen_height() * 0.35,
        48.0,
        WHITE,
    );
    for (i, mode) in [Mode::Training, Mode::Challenge].into_iter().enumerate() {
        draw_text(
            hud::menu_option_line(mode, selected),
            cx - 70.0,
            screen_height() * 0.5 + i as f32 * 32.0,
            28.0,
            if mode == selected { YELLOW } else { GRAY },
        );
    }
    draw_text(
        "up/down: select   space: start",
        cx - 110.0,
        screen_height() * 0.7,
        16.0,
        GRAY,
    );
}

enum Screen {
    Menu { selected: Mode },
    Match(Box<MatchState>),
}

#[macroquad::main("Disc (1990, Loriciel) -- disc-core front end")]
async fn main() {
    // Loaded once, before the menu ever shows: `assets/original/`'s `.SPL`
    // files decoded to macroquad `Sound`s (or silently skipped -- see
    // `audio`'s module doc). Cheap to clone into every `MatchState` since
    // `macroquad::audio::Sound` is itself an `Arc` internally.
    let sfx = audio::load().await;

    let mut screen = Screen::Menu {
        selected: Mode::Training,
    };

    loop {
        let mut next_screen = None;
        match &mut screen {
            Screen::Menu { selected } => {
                if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::Down) {
                    *selected = match *selected {
                        Mode::Training => Mode::Challenge,
                        Mode::Challenge => Mode::Training,
                    };
                }
                if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
                    next_screen = Some(Screen::Match(Box::new(
                        MatchState::new(*selected).with_sfx(sfx.clone()),
                    )));
                }
                draw_menu(*selected);
            }
            Screen::Match(ms) => {
                ms.pad.poll();
                if is_key_pressed(KeyCode::Escape) {
                    next_screen = Some(Screen::Menu {
                        selected: ms.m.mode,
                    });
                } else if is_key_pressed(KeyCode::R) {
                    next_screen = Some(Screen::Match(Box::new(
                        MatchState::new(ms.m.mode).with_sfx(sfx.clone()),
                    )));
                } else {
                    if is_key_pressed(KeyCode::P) {
                        ms.paused = !ms.paused;
                    }
                    if !ms.paused {
                        for _ in 0..ms.clock.feed(f64::from(get_frame_time())) {
                            ms.step_tick();
                        }
                    }
                    draw_match(ms, ms.clock.alpha());
                }
            }
        }
        if let Some(s) = next_screen {
            screen = s;
        }
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
            fire_held: n.is_multiple_of(7),
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
    fn the_real_bank_layout_covers_every_measured_cell() {
        const _: () = assert!(BANK_REAL_CELLS <= TILE_CELLS);
        assert_eq!(BANK_COLS * BANK_ROWS, BANK_REAL_CELLS);
    }

    /// Without [`MatchState::serve_workaround`], `disc_core::GameState::tick`
    /// alone never serves a disc for a freshly-simulated player 2 (its own
    /// auto-serve gate never opens outside a replayed trace -- see that
    /// method's doc). The workaround must actually put one in play.
    #[test]
    fn serve_workaround_puts_a_disc_in_play_when_p2_wants_to_serve() {
        let mut ms = MatchState::new(Mode::Training);
        assert!(
            ms.cur.discs.iter().all(|d| !d.simulated()),
            "nothing served yet"
        );
        ms.serve_workaround(Input::default(), true);
        assert_eq!(
            ms.cur.discs.iter().filter(|d| d.simulated()).count(),
            1,
            "one disc now in play"
        );
        assert_eq!(ms.cur.players[1].discs_out, 1);
    }

    #[test]
    fn serve_workaround_respects_the_cooldown_and_the_disc_cap() {
        let mut ms = MatchState::new(Mode::Training);
        // Wanting to serve every tick for far longer than the cooldown must
        // not exceed the disc cap (4, `round::P2_FRESH_DISC_CAP`).
        for _ in 0..(u32::from(SERVE_COOLDOWN_TICKS) * 6) {
            ms.serve_workaround(Input::default(), true);
        }
        let in_play = ms.cur.discs.iter().filter(|d| d.simulated()).count();
        assert!(in_play <= 4, "never exceeds the disc cap, got {in_play}");
        assert!(in_play >= 1, "still served at least one");
    }

    #[test]
    fn serve_workaround_does_nothing_when_not_wanted() {
        let mut ms = MatchState::new(Mode::Training);
        ms.serve_workaround(Input::default(), false);
        assert!(ms.cur.discs.iter().all(|d| !d.simulated()));
    }

    /// `p2_input` must never touch the fallback while a decoded row (0/1)
    /// has an opinion -- that is the whole reason the fallback is safe to be
    /// undecoded: it never overrides real behaviour, only fills silence.
    #[test]
    fn decoded_ai_rows_take_priority_over_the_fallback() {
        let mut ms = MatchState::new(Mode::Training);
        // Force player 2 onto a destroyed floor cell in `tiles_far` so
        // `disc_core::ai`'s row 0 (escape) fires: grid_cell 9 maps to
        // tiles_far[0] (see `disc_core::ai::test_escape`).
        ms.cur.players[1].grid_cell = 9;
        ms.cur.tiles_far[0].tile_type = TILE_TYPE_DESTROYED;

        // What the decoded row alone produces, from a fresh `Ai` against the
        // same state -- this is the ground truth `p2_input` must not deviate
        // from by consulting the fallback instead.
        let mut probe = Ai::default();
        let expected = probe.p2_policy(&ms.cur);
        assert_ne!(expected, 0, "row 0 should fire on a destroyed own cell");

        let input = ms.p2_input();
        assert_eq!(
            input.dir.0, expected,
            "the decoded row's own bits, verbatim"
        );
        assert!(!input.fire_held, "row 0 never sets fire");
    }

    #[test]
    fn step_tick_deals_a_fresh_round_after_the_round_over_hold_elapses() {
        let mut ms = MatchState::new(Mode::Training);
        ms.cur.players[0].state_index = disc_core::player::STATE_DEAD;
        ms.step_tick(); // Playing -> observe sees the death -> RoundOver.
        assert!(matches!(ms.m.phase, Phase::RoundOver { .. }));
        let Phase::RoundOver { hold, .. } = ms.m.phase else {
            unreachable!()
        };
        // `hold` more ticks bring the counter to 0; one more is the tick
        // that actually observes 0 and deals the fresh round (see
        // `round::Match::observe`'s `RoundOver` arm).
        for _ in 0..=hold {
            ms.step_tick();
        }
        assert_eq!(ms.m.phase, Phase::Playing, "a fresh round was dealt");
        assert!(ms.cur.tiles.iter().all(|t| t.walkable()), "fresh floor");
    }

    #[test]
    fn y_in_band_clamps_to_the_assumed_range() {
        assert_eq!(y_in_band(-5, PLAYER1_Y_RANGE), 0.0);
        assert_eq!(y_in_band(0, PLAYER1_Y_RANGE), 0.0);
        assert!(y_in_band(16, PLAYER1_Y_RANGE) > 0.0 && y_in_band(16, PLAYER1_Y_RANGE) < 1.0);
        assert_eq!(y_in_band(999, PLAYER1_Y_RANGE), 1.0);
    }

    /// Player 2's own operating range sits much higher than player 1's --
    /// see [`PLAYER2_Y_RANGE`]'s doc -- and must map into the same `0.0..1.0`
    /// band, not read as permanently clamped to one edge.
    #[test]
    fn player_2_s_higher_y_range_still_spans_its_band() {
        assert_eq!(y_in_band(PLAYER2_Y_RANGE.0, PLAYER2_Y_RANGE), 0.0);
        assert_eq!(y_in_band(PLAYER2_Y_RANGE.1, PLAYER2_Y_RANGE), 1.0);
        let mid = y_in_band((PLAYER2_Y_RANGE.0 + PLAYER2_Y_RANGE.1) / 2, PLAYER2_Y_RANGE);
        assert!(mid > 0.4 && mid < 0.6);
    }

    /// A full unattended challenge match must actually reach game over,
    /// within generously many ticks -- not softlock. This is a regression
    /// guard on the `round_over` fix `round::Match::observe` documents: left
    /// unset, player 1's first death leaves player 2's `round_over` (set by
    /// `disc-core` itself when player 1 dies) permanently true, and since
    /// every disc's `aim` is always `PlayerId::Two` (`disc::serve`'s own
    /// doc), `disc::step`'s holder-owns-round_over check retires every disc
    /// player 2 serves from then on, on the very tick after it is served --
    /// so a challenge round can never reach the second death it needs.
    #[test]
    fn an_unattended_challenge_match_reaches_game_over() {
        let mut ms = MatchState::new(Mode::Challenge);
        let ticks_per_second = 50;
        let generous_budget = ticks_per_second * 60; // a full minute at 50 Hz
        for _ in 0..generous_budget {
            ms.step_tick();
            if matches!(ms.m.phase, Phase::GameOver { .. }) {
                return;
            }
        }
        panic!(
            "challenge match did not reach game over within {generous_budget} ticks -- \
             phase stuck at {:?}",
            ms.m.phase
        );
    }
}
