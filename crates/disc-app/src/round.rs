//! Round and match bookkeeping -- an APP-level layer on top of `disc-core`.
//!
//! `disc-core::round`'s own module docs say plainly that round init,
//! round-over and win/loss are "not modelled here": `$aa50` (round init),
//! `$6c83` (the deaths tally), `$6ca0` (the training/challenge mode byte) and
//! `player+$72`/`+$74` (the score/win-loss BCD field) are all decoded from the
//! static image only, with no live capture on hand to check an implementation
//! against (`reports/part12-round.md`, `crates/disc-core/src/round.rs`
//! module docs). Building "a real game loop: menu -> match -> round
//! transitions -> game over" (discr-13v item 1) therefore cannot be done by
//! calling into core -- there is nothing there to call. This module is the
//! app's own bookkeeping, driven by the one thing core DOES expose honestly:
//! [`disc_core::player::STATE_DEAD`] transitions and [`Player::energy`].
//!
//! Two numbers here are decoded and cited exactly:
//! - [`Mode::deaths_to_end_round`]: training ends a round on the first death,
//!   challenge waits for two (`reports/part12-round.md`'s `$9746`-`$975c`:
//!   `tst.b $6c83; bne` for training vs `cmpi.b #2,$6c83; bge` otherwise).
//!
//! Everything else below this line is an app-level POLICY choice, not a
//! decode, because the decode does not reach far enough to fix it:
//! - **Which player wins a round.** `player+$72`'s BCD comparison decides
//!   this on the ST, but no fixture ever moves that field (round.rs: "not
//!   tied to any specific in-game event by evidence on hand"), so there is no
//!   observed rule to transcribe. This module credits the round to whoever
//!   did NOT contribute the round's last death -- a plausible, simple stand-in
//!   documented here as a guess, not a fact.
//! - **How many round wins end the match.** The round-over exit's own caller
//!   -- whatever loads the next round or a match summary -- sits outside every
//!   snapshot this project has taken (round.rs: "outside every snapshot this
//!   project has taken"). [`GAME_OVER_ROUND_WINS`] is an arbitrary, clearly
//!   labelled app choice (best-of-five), not a decoded value.
//! - **Round-start state.** [`fresh_round_state`] is not `$aa50`: that ST
//!   routine sets `disc+$18 := 3` (a field this crate does not carry) among
//!   other things core's own docs flag as unmodelled (`discr-st8`). This
//!   function uses `GameState::default()` plus the one MEASURED value on
//!   hand for each field it touches (see its own doc comment for citations).
//!
//! Serve alternation ("rounds alternate serves", discr-13v item 1) is not
//! implemented: `disc-core`'s own `GameState::update` wires `disc::serve` to
//! player 2's throw states only -- player 1's control routine has its own
//! undecoded call sites (`// UNKNOWN: see bd discr-b6x`, `disc-core/src/
//! lib.rs`). There is no core entry point that would let player 1 serve, so
//! there is nothing here to alternate; player 2 (the AI) always serves.

use disc_core::player::STATE_DEAD;
use disc_core::{GameState, Player, Tile, WALK_X_MAX, WALK_X_MIN};

/// First-to-N round wins ends the match. **Not decoded** -- see the module
/// docs. Five is an arbitrary, common-arcade choice.
pub const GAME_OVER_ROUND_WINS: u32 = 3;

/// The starting floor for a fresh round: every cell of both banks walkable
/// with the same `{tile_type, hp}` `disc-core`'s own tests use as "the floor
/// before any disc lands" (`crates/disc-core/src/player.rs` test module's
/// `FLOOR` constant, `Tile { tile_type: 1, hp: 4 }`). Not a decode of
/// `$aa50`'s real tile-init values (unrecovered), just the same reasonable
/// stand-in core's own test suite already relies on.
const FRESH_FLOOR: Tile = Tile {
    tile_type: 1,
    hp: 4,
};

/// Starting energy for both players. `disc-core::types::Player::energy`'s own
/// doc cites the one measured value on hand: "Player 1 reads 5 at the start
/// of the golden fixture." Applied to both players for lack of a second
/// measurement.
const FRESH_ENERGY: i16 = 5;

/// Starting `world_x` for both players: the middle of the walkable range.
/// `GameState::default()` leaves this at 0, which is off the near end of
/// [`WALK_X_MIN`]`..`[`WALK_X_MAX`] and, worse, at the very edge of the
/// rendered arena -- not a decoded spawn point (round-init is `discr-st8`,
/// unmodelled), just a sane place to stand.
const FRESH_WORLD_X: i16 = (WALK_X_MIN + WALK_X_MAX) / 2;

/// Player 2's starting `disc_cap` (the outstanding-discs cap `disc::serve`'s
/// caller checks `discs_out` against). `disc_core::types::Player::disc_cap`'s
/// own doc: "Reads 4 for player 2 ... Never written anywhere in the analysed
/// image" -- i.e. this is round-init's job ($aa50, `discr-st8`, unmodelled),
/// and `GameState::default()` leaves it 0. Left at 0 that cap can never be
/// cleared, `ai_fallback::should_serve` is always false, and player 2 goes
/// through the whole match without ever serving. This is the one measured
/// value on hand for it. Player 1's own cap is left at 0, matching its own doc:
/// "0 for player 1" -- consistent with player 1 never serving at all
/// (`disc-core`'s `GameState::update` only wires `disc::serve` to player 2).
const P2_FRESH_DISC_CAP: i16 = 4;

/// Player 2's `throw_dir_kind`/`throw_damage`: the two fields `disc::serve`
/// copies straight into a served disc's own `dir_kind`/`damage`.
/// `disc_core::types::Player`'s own docs measure both directly: `dir_kind`
/// "reads ... -3 for player 2" (exactly [`disc_core::disc::RETURN_DIR_KIND`])
/// and `damage` "reads ... 3" for player 2 -- both flagged "`// UNKNOWN (its
/// writer)`", i.e. round-init's job, left at `GameState::default()`'s 0.
/// Left at 0, a served disc has no damage and, worse, a `dir_kind` of 0 means
/// `disc::step`'s `world_z += dir_kind` never advances it at all -- the served
/// disc does not travel, and whatever out-of-range starting `world_z` it
/// happened to get (see [`FRESH_WORLD_Y_P2`]) is read as an instant,
/// untravelled wall crossing on its very next tick. This is what actually
/// starts a served disc moving.
const P2_THROW_DIR_KIND: i16 = disc_core::disc::RETURN_DIR_KIND;
const P2_THROW_DAMAGE: i16 = 3;
/// The same two fields for player 1, for parity/documentation -- never
/// actually read, since `disc-core` only wires a serve to player 2
/// (`discr-b6x`). Measured values: `+1` and `1`.
const P1_THROW_DIR_KIND: i16 = disc_core::disc::SERVE_DIR_KIND;
const P1_THROW_DAMAGE: i16 = 1;

/// Player 1's starting `world_y`. Not a decoded spawn value (round-init,
/// `discr-st8`, is unmodelled) -- 18 is the value `disc-core`'s own player.rs
/// test fixtures use for "a player standing on the floor"
/// (`player::tests::walking`), reused here for the same reason `FRESH_FLOOR`
/// reuses that module's own `FLOOR` constant: it is a value core's own
/// authors already treated as a reasonable stand-in. It also puts player 1's
/// hit-test depth threshold (`player::hit_test` crosses at `player.world_y`)
/// well inside the near half of a served disc's travel, so a disc actually
/// reaches it.
const FRESH_WORLD_Y_P1: i16 = 18;

/// Player 2's starting `world_y`. `disc::serve`'s own parameter build uses
/// the THROWER's `world_y` as the served disc's starting `world_z` (depth) --
/// `$c088 d1.high = $6d26 - 1`, i.e. `thrower.world_y - 1`. With
/// `GameState::default()`'s `world_y = 0`, that starting depth is `-1`,
/// already past [`disc_core::disc::Z_NEAR`] (0) before the disc has moved at
/// all: the very next `disc::step` reads it as an untravelled wall crossing.
/// `disc_core::disc::Z_FAR` (79) puts the served disc's starting depth at 78
/// -- near the far wall, matching this crate's own near/far rendering
/// (`main.rs`'s two-platform layout) and giving it the full court to cross at
/// [`P2_THROW_DIR_KIND`]'s 3-per-tick before reaching player 1. Not a decoded
/// spawn value; chosen for these two reasons.
const FRESH_WORLD_Y_P2: i16 = disc_core::disc::Z_FAR;

/// Which mode this match is being played in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Training,
    Challenge,
}

impl Mode {
    /// Deaths needed to end the round. `reports/part12-round.md`,
    /// `$9746`-`$975c`: training on the first death, challenge (and
    /// tournament, not offered by this menu) on the second.
    #[must_use]
    pub const fn deaths_to_end_round(self) -> u32 {
        match self {
            Mode::Training => 1,
            Mode::Challenge => 2,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Mode::Training => "TRAINING",
            Mode::Challenge => "CHALLENGE",
        }
    }
}

/// Where the match currently is. Owned by the app; `disc-core` knows nothing
/// about any of this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Ticking normally.
    Playing,
    /// A round just ended; `winner` is the player index credited with it.
    /// Held for [`ROUND_OVER_HOLD_TICKS`] before the next round starts.
    RoundOver { winner: usize, hold: u16 },
    /// The match is decided.
    GameOver { winner: usize },
}

/// How long to hold on the round-over screen before dealing the next round,
/// in ticks (50/s). Two seconds. App pacing choice, not a decode.
pub const ROUND_OVER_HOLD_TICKS: u16 = 100;

/// One match: a mode, a running score, and the current phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    pub mode: Mode,
    /// Round wins, indexed like [`GameState::players`].
    pub round_wins: [u32; 2],
    pub phase: Phase,
    /// Deaths counted so far in the CURRENT round (the app's stand-in for the
    /// ST's global `$6c83` tally -- see the module docs).
    deaths_this_round: u32,
    /// Which player index contributed the most recent death, for the
    /// round-winner policy documented above.
    last_to_die: Option<usize>,
    /// Edge-detects entry into [`STATE_DEAD`] per player, so a player sitting
    /// in that terminal state across many ticks is counted once.
    was_dead: [bool; 2],
}

impl Match {
    #[must_use]
    pub fn new(mode: Mode) -> Self {
        Match {
            mode,
            round_wins: [0, 0],
            phase: Phase::Playing,
            deaths_this_round: 0,
            last_to_die: None,
            was_dead: [false, false],
        }
    }

    /// Build the app-level round-start state. See the module docs for what
    /// this is and is not.
    #[must_use]
    pub fn fresh_round_state() -> GameState {
        let mut gs = GameState::default();
        for p in &mut gs.players {
            p.energy = FRESH_ENERGY;
            p.world_x = FRESH_WORLD_X;
        }
        gs.players[0].world_y = FRESH_WORLD_Y_P1;
        gs.players[0].throw_dir_kind = P1_THROW_DIR_KIND;
        gs.players[0].throw_damage = P1_THROW_DAMAGE;
        gs.players[1].world_y = FRESH_WORLD_Y_P2;
        gs.players[1].throw_dir_kind = P2_THROW_DIR_KIND;
        gs.players[1].throw_damage = P2_THROW_DAMAGE;
        gs.players[1].disc_cap = P2_FRESH_DISC_CAP;
        gs.tiles = [FRESH_FLOOR; disc_core::TILE_CELLS];
        gs.tiles_far = [FRESH_FLOOR; disc_core::TILE_CELLS];

        // Load each player's idle sequence. `GameState::default()` leaves
        // `anim_base` at 0, which `player::anim_for` matches nothing --
        // `player::anim_tick` (run every idle tick by `player::idle_tick`)
        // then returns `Holding` without ever copying a frame block, so
        // `Player::hit_box` stays `[0, 0, 0, 0]` forever. `hit_test`'s own X
        // window is built entirely from that box (`player.world_x - 8 + b0`
        // .. `+ 8 + b1`), so a `[0,0,0,0]` box is a real but wrong 8-unit
        // window nothing this crate serves ever happens to land in -- no
        // strike, no catch, ever, for the life of the match. `enter_anim`
        // with each player's own `idle_anim` is the one call a real round
        // init makes before anything else runs (every state handler's own
        // preamble does the same three-instruction load); it is not a game
        // rule, it is pointing an existing field at real, already-decoded
        // animation-table data instead of the zero `GameState::default()`
        // leaves it at.
        disc_core::player::enter_anim(
            &mut gs.players[0],
            disc_core::player::idle_anim(disc_core::PlayerId::One),
        );
        disc_core::player::enter_anim(
            &mut gs.players[1],
            disc_core::player::idle_anim(disc_core::PlayerId::Two),
        );
        gs
    }

    /// Call once per tick, after `state.tick(..)`, while [`Phase::Playing`].
    /// Detects a death, tallies it, and transitions the phase when the
    /// round's (or match's) threshold is reached. Advances the
    /// [`Phase::RoundOver`] hold timer when not playing; returns `true` the
    /// instant a fresh round should be dealt (caller resets `GameState` from
    /// [`Self::fresh_round_state`] and calls [`Self::start_round`]).
    ///
    /// Takes `players` mutably for one reason: [`STATE_DEAD`]'s own doc says
    /// plainly "Terminal -- nothing leaves it". In training that is fine --
    /// one death always ends the round. In challenge, which waits for TWO
    /// (`Mode::deaths_to_end_round`), a genuinely terminal death would
    /// softlock every round after the first one: the dead player can never
    /// die a second time, and with them unable to return a served disc, the
    /// other player has nothing left to take a hit from either -- observed
    /// live, not hypothesised: a challenge match dealt this way never
    /// finishes its second round. Real round-over/round-init are both
    /// unmodelled (`discr-st8`, see the module docs), so there is no decoded
    /// mid-round respawn to transcribe; [`Self::observe`] revives a dead
    /// player back to a fresh standing state itself, the moment their death
    /// is tallied, whenever that tally has NOT yet ended the round -- an
    /// app-level policy this crate needs to make challenge mode playable at
    /// all, not a decode.
    pub fn observe(&mut self, players: &mut [Player; 2]) -> bool {
        match &mut self.phase {
            Phase::Playing => {
                for (i, player) in players.iter_mut().enumerate() {
                    let dead_now = player.state_index == STATE_DEAD;
                    if dead_now && !self.was_dead[i] {
                        self.deaths_this_round += 1;
                        self.last_to_die = Some(i);
                    }
                    self.was_dead[i] = dead_now;
                }
                if self.deaths_this_round >= self.mode.deaths_to_end_round() {
                    // Round-winner policy: NOT decoded, see module docs.
                    let loser = self.last_to_die.unwrap_or(0);
                    let winner = 1 - loser;
                    self.round_wins[winner] += 1;
                    self.phase = if self.round_wins[winner] >= GAME_OVER_ROUND_WINS {
                        Phase::GameOver { winner }
                    } else {
                        Phase::RoundOver {
                            winner,
                            hold: ROUND_OVER_HOLD_TICKS,
                        }
                    };
                } else {
                    // The round is not over yet but someone is dead (only
                    // reachable in a mode needing more than one death, i.e.
                    // challenge) -- revive them so play, and the possibility
                    // of a second death, can continue. See this method's own
                    // doc. Plain indices, not an iterator, because reviving
                    // player `i` also has to clear `round_over` on player
                    // `1 - i` -- `disc-core`'s own `GameState::update` sets
                    // it on the SURVIVOR when the other one enters
                    // `STATE_DEAD` (`player+$0d`, "set on the OTHER player
                    // when this one runs out of energy"), and never clears it
                    // (`// UNKNOWN (what clears it): see bd discr-st8`).
                    // Left set, `disc::step`'s own holder-owns-round_over
                    // check retires every disc the SURVIVOR owns the instant
                    // it is served -- since player 2 is this game mode's only
                    // server and every served disc's `aim` is unconditionally
                    // `PlayerId::Two` (`disc::serve`'s own doc), a player 1
                    // death leaving `players[1].round_over` set stillbirths
                    // every disc player 2 serves from then on, and a
                    // challenge round can never see the second death it is
                    // waiting for. Observed live, not hypothesised: a
                    // challenge match dealt this way serves discs (the
                    // cooldown keeps cycling) that vanish before the next
                    // sample every time.
                    for i in 0..2 {
                        if players[i].state_index == STATE_DEAD {
                            revive(&mut players[i], i);
                            players[1 - i].round_over = false;
                            self.was_dead[i] = false;
                        }
                    }
                }
                false
            }
            Phase::RoundOver { hold, .. } => {
                if *hold == 0 {
                    true
                } else {
                    *hold -= 1;
                    false
                }
            }
            Phase::GameOver { .. } => false,
        }
    }

    /// Deal a new round: clears the per-round tally and returns to
    /// [`Phase::Playing`]. Round-win totals and the mode carry over.
    pub fn start_round(&mut self) {
        self.deaths_this_round = 0;
        self.last_to_die = None;
        self.was_dead = [false, false];
        self.phase = Phase::Playing;
    }
}

/// Bring a dead player back to a fresh standing state, mid-round. See
/// [`Match::observe`]'s doc for why this exists. `index` is the same 0/1
/// indexing every other player-array access in this crate uses
/// (`disc_core::PlayerId::index`).
fn revive(player: &mut Player, index: usize) {
    player.energy = FRESH_ENERGY;
    player.down = false;
    player.state_index = disc_core::player::STATE_IDLE;
    let who = if index == 0 {
        disc_core::PlayerId::One
    } else {
        disc_core::PlayerId::Two
    };
    // Re-run the same idle-sequence load `fresh_round_state` does at round
    // start -- otherwise `anim_base` is left pointing at `ANIM_DEAD` and
    // `hit_box` stays whatever that sequence's last frame left it at (see
    // `fresh_round_state`'s own doc on why this matters for `hit_test`).
    disc_core::player::enter_anim(player, disc_core::player::idle_anim(who));
}

#[cfg(test)]
mod tests {
    use super::*;
    use disc_core::player::STATE_IDLE;

    fn players_with_state(p0: u8, p1: u8) -> [Player; 2] {
        [
            Player {
                state_index: p0,
                ..Player::default()
            },
            Player {
                state_index: p1,
                ..Player::default()
            },
        ]
    }

    #[test]
    fn training_ends_the_round_on_the_first_death() {
        let mut m = Match::new(Mode::Training);
        assert!(!m.observe(&mut players_with_state(STATE_IDLE, STATE_IDLE)));
        assert_eq!(m.phase, Phase::Playing);
        m.observe(&mut players_with_state(STATE_DEAD, STATE_IDLE));
        assert_eq!(
            m.phase,
            Phase::RoundOver {
                winner: 1,
                hold: ROUND_OVER_HOLD_TICKS
            }
        );
        assert_eq!(m.round_wins, [0, 1]);
    }

    /// Also exercises the revive: without it, player 0 stays stuck in
    /// [`STATE_DEAD`] (its own doc: "Terminal -- nothing leaves it") and a
    /// challenge round can never see its second death. This uses one
    /// PERSISTENT `players` array across both calls -- unlike the
    /// single-death tests above, which only need one `observe` each -- so
    /// the revive `observe` performs on the first call is actually visible
    /// to the second, the way a real running match would see it.
    #[test]
    fn challenge_needs_two_deaths_and_revives_the_first_casualty() {
        let mut m = Match::new(Mode::Challenge);
        let mut players = players_with_state(STATE_DEAD, STATE_IDLE);
        assert!(!m.observe(&mut players));
        assert_eq!(m.phase, Phase::Playing, "one death is not enough yet");
        assert_eq!(m.round_wins, [0, 0]);
        assert_eq!(
            players[0].state_index, STATE_IDLE,
            "revived, not left stuck dead"
        );
        assert_eq!(players[0].energy, FRESH_ENERGY);
        assert!(!players[0].down);

        // Now player 1 dies too -- the round's second death.
        players[1].state_index = STATE_DEAD;
        m.observe(&mut players);
        assert!(matches!(m.phase, Phase::RoundOver { .. }));
    }

    /// The revive must clear `round_over` on the OTHER player too, or every
    /// disc player 2 (the only server) ever serves again is retired the
    /// instant it is simulated -- see `observe`'s own doc for the full chain.
    #[test]
    fn reviving_a_dead_player_clears_round_over_on_the_survivor() {
        let mut m = Match::new(Mode::Challenge);
        let mut players = players_with_state(STATE_DEAD, STATE_IDLE);
        // `GameState::update` sets this on the survivor when the other
        // player enters `STATE_DEAD` -- reproduced by hand here since this
        // module only ever reads `state_index`, not the tick that sets it.
        players[1].round_over = true;
        m.observe(&mut players);
        assert!(
            !players[1].round_over,
            "the survivor's round_over must be cleared so it can serve again"
        );
    }

    #[test]
    fn the_survivor_of_the_last_death_wins_the_round() {
        let mut m = Match::new(Mode::Challenge);
        // Player 1 dies first (revived by the first `observe`), then player
        // 0 -- player 0 contributed the LAST death, so player 1 is credited
        // the round under this module's documented (undecoded) policy.
        let mut players = players_with_state(STATE_IDLE, STATE_DEAD);
        m.observe(&mut players);
        players[0].state_index = STATE_DEAD;
        m.observe(&mut players);
        assert_eq!(
            m.phase,
            Phase::RoundOver {
                winner: 1,
                hold: ROUND_OVER_HOLD_TICKS
            }
        );
    }

    #[test]
    fn the_hold_counts_down_then_signals_a_fresh_round() {
        let mut m = Match::new(Mode::Training);
        m.observe(&mut players_with_state(STATE_DEAD, STATE_IDLE));
        let Phase::RoundOver { hold, .. } = m.phase else {
            panic!("expected RoundOver")
        };
        for _ in 0..hold {
            assert!(!m.observe(&mut players_with_state(STATE_IDLE, STATE_IDLE)));
        }
        assert!(m.observe(&mut players_with_state(STATE_IDLE, STATE_IDLE)));
        m.start_round();
        assert_eq!(m.phase, Phase::Playing);
    }

    #[test]
    fn game_over_at_the_configured_round_win_count() {
        let mut m = Match::new(Mode::Training);
        for n in 1..GAME_OVER_ROUND_WINS {
            m.observe(&mut players_with_state(STATE_DEAD, STATE_IDLE));
            m.start_round();
            assert_eq!(m.round_wins[1], n);
        }
        m.observe(&mut players_with_state(STATE_DEAD, STATE_IDLE));
        assert_eq!(m.phase, Phase::GameOver { winner: 1 });
    }

    #[test]
    fn fresh_round_state_has_a_walkable_floor_and_starting_energy() {
        let gs = Match::fresh_round_state();
        assert!(gs.tiles.iter().all(|t| t.walkable()));
        assert!(gs.tiles_far.iter().all(|t| t.walkable()));
        assert!(gs.players.iter().all(|p| p.energy == FRESH_ENERGY));
    }

    /// Without these, a served disc has `dir_kind == 0` and never advances in
    /// depth at all (see [`P2_THROW_DIR_KIND`]'s doc) -- the bug that made a
    /// live match's very first serve read as an instant, untravelled wall
    /// crossing instead of a disc actually crossing the court.
    #[test]
    fn fresh_round_state_gives_player_2_nonzero_throw_stats_and_cap() {
        let gs = Match::fresh_round_state();
        assert_eq!(gs.players[1].throw_dir_kind, P2_THROW_DIR_KIND);
        assert_ne!(gs.players[1].throw_dir_kind, 0);
        assert_eq!(gs.players[1].throw_damage, P2_THROW_DAMAGE);
        assert_eq!(gs.players[1].disc_cap, P2_FRESH_DISC_CAP);
    }

    /// A served disc's starting depth is `thrower.world_y - 1`
    /// (`disc::serve`'s own parameter build) -- it must land strictly inside
    /// `Z_NEAR..Z_FAR`, not already past a bound before the disc has moved.
    #[test]
    fn fresh_round_state_puts_p2_s_world_y_inside_the_valid_depth_range() {
        let gs = Match::fresh_round_state();
        let starting_z = gs.players[1].world_y - 1;
        assert!(starting_z > disc_core::disc::Z_NEAR && starting_z < disc_core::disc::Z_FAR);
    }
}
