//! Round-level bookkeeping: the four possession counters the wall handlers
//! move, and what is decoded (but not modelled) about round init, round-over
//! and win/loss.
//!
//! Bead `discr-st8`. Full chain and every address: `reports/part12-round.md`.
//!
//! # The four counters
//!
//! ST `$6d8a`/`$6d8c` (player 2's own outstanding-disc count and its cap,
//! [`crate::Player::discs_out`]/[`crate::Player::disc_cap`] at index 1) and
//! their mirror `$6d0a`/`$6d0c` (player 1's, index 0). Three kinds of writer,
//! per `reports/part12-owner.md`:
//!
//! * **Serve** (`$a9aa`): bumps the thrower's own `discs_out`. Only player 2
//!   ever serves in this game mode -- [`crate::GameState`]'s own update loop
//!   does this already, right next to [`crate::disc::serve`]'s call site.
//! * **Catch** (`$caae`/`$cb1e` then `$cab2`/`$cb22`): decrements the
//!   catcher's own `discs_out` when a hit test's catch window closes over a
//!   disc -- [`crate::player::p2_hit_test`] (and `hit_test`, mirrored)
//!   already do this, at the same [`crate::disc::ACTIVE_RETIRE_STEP`] site.
//! * **Wall transfer** (`$a5d0`-`$a5fa` far wall, `$a612`-`$a63c` near wall):
//!   this module. A disc that reaches a wall UNCAUGHT (the owner byte still
//!   reads the thrower's own value) moves out of the thrower's ledger and
//!   into the other player's -- `discs_out` and `disc_cap` both move, in
//!   lockstep, for BOTH players, in the same tick the owner byte flips.
//!
//! [`transfer_at_far_wall`] and [`transfer_at_near_wall`] are the third kind,
//! called from [`crate::disc::step`]'s two wall-bound match arms.
//!
//! # Measured
//!
//! `tests/fixtures/p1_walk.ndjson` frame 220 (the far wall, disc 0's owner
//! flipping 0 -> 255): `players[1]` (P2) `discs_out`/`disc_cap` 4,4 -> 3,3 and
//! `players[0]` (P1) 0,0 -> 1,1, in the SAME tick. `tracecheck`'s replay
//! reaches this frame live (the fixture's own gate is `--min-agree 274`, the
//! whole trace) once `discs_out`/`disc_cap` are compared rather than fed or
//! silently unchecked -- see `crates/disc-tools/src/main.rs`'s `checks()`.
//!
//! `tests/fixtures/handover.ndjson` records BOTH directions on one disc slot
//! (frames 259 far wall, 339 near wall -- see `reports/part12-owner.md`), but
//! `disc-core`'s own replay of that fixture diverges earlier (an unrelated
//! `discs[0].active` gap, tick 21 bare / 222 skip-waived) on a different
//! field, so [`transfer_at_near_wall`] is measured against the fixture's own
//! recorded columns rather than against a tracecheck run that reaches frame
//! 339. A future fixture that survives past the active-byte gap would let
//! tracecheck confirm it directly.
//!
//! # Not modelled here
//!
//! * **The `$6ca0.b != 1` gate** on both wall transfers (`reports/
//!   part12-owner.md` flagged it undecoded; this part names it: `$6ca0` is
//!   the game-MODE byte -- 1 selects training, read at `$9746`/`$97a8` in the
//!   round loop's own win-check, and `$116c4` sets it for player 2's
//!   hardcoded throw stats (`docs/disc-notes.md`'s discr-qqt section), both
//!   independently. So the wall transfer is gated OFF in training mode -- no
//!   fixture on hand is a training-mode capture with a live wall crossing, so
//!   this crate does not gate on it and would over-transfer in training.
//!   `// UNKNOWN: see bd discr-st8`.
//! * **Round init, round-over and win/loss** -- decoded from the static image
//!   only (Ghidra plus a raw byte-pattern scan of `discram.bin` with
//!   `capstone`, the same technique `reports/part12-rng.md` and
//!   `part12-dirkind.md` used for code outside Ghidra's own CFG walk -- this
//!   whole span was outside it too: `xref`/`scan` on `$aa50` both return zero
//!   hits). No live Hatari capture in this project's history brackets a round
//!   transition (the FDC boundary), so none of this is measured dynamically.
//!   Full chain and every instruction: `reports/part12-round.md`. Summary:
//!   - `$9628`-`$96b6`: round-start init, ~20 subroutine calls found by a
//!     `bsr.w` targeting `$aa50` (Ghidra's own analysis has no xref to it --
//!     the caller sits in the same undissassembled span Part 12b's `$968a`
//!     reseed was found in). `$aa50` itself (disassembled in full) resets the
//!     8 disc records: `active`/`aim` cleared, `world_y := 0x52`, and a
//!     previously undocumented word at `disc+$18 := 3` this crate does not
//!     carry (between `damage` at `+$16` and the excluded pointer at `+$1a`).
//!     `$9682 clr.b $6c83` resets the round-over counter in the same chain,
//!     alongside the already-known `$968a` RNG reseed.
//!   - `$9600`-`$97d6`: the per-VBL round-play loop (`$96ba`'s `bsr $a4ea` is
//!     `GameState::update`'s own disc loop, already modelled; `$9700`-`$972e`
//!     is the VBL-pacing wait this crate's `updates`/`outer` fields already
//!     account for from the trace side).
//!   - `$6c83`: a GLOBAL (not per-player) "deaths this round" tally, bumped
//!     +1 by each player's own state-23 terminal handler (`$10abe` player 1,
//!     `$c3b6` player 2 -- `docs/disc-notes.md`'s state-23 section already
//!     names the player-1 site and its bump) and, via that same section's
//!     "state 31 forces an immediate round reset" finding, by state 31's
//!     shared fallthrough into state 23's terminal code.
//!   - `$9746`-`$975c`: the round-over THRESHOLD, gated by the mode byte
//!     `$6ca0` (a word at player 1's own base address, `+$00` -- distinct
//!     from any modelled `Player` field, which starts at `+$02`): training
//!     ends the round on the first death (`tst.b $6c83; bne`), challenge and
//!     tournament wait for two (`cmpi.b #2,$6c83; bge`).
//!   - `$97b2`-`$97cc`: a win/loss comparison at a previously undocumented
//!     field, `player+$72` (`$6d12` p1 / `$6d92` p2, right after
//!     `throw_damage` at `+$70`) -- the smaller value's owner is marked DOWN
//!     (`$cac`/`$cad`... i.e. the existing `down` field) and the larger
//!     value's owner has its own `round_over` set. A BCD-style
//!     incrementer-with-carry at `$9938` (mirrored at `$9956` for player 2)
//!     writes it, chained off a second word at `+$74`: an increment
//!     overflowing past 9 (`cmpi.b #$a`) carries into the digit above. Not
//!     tied to any specific in-game event by evidence on hand -- no fixture's
//!     `player+$72` ever moves -- but it is a strong candidate for the
//!     on-screen score.
//!   - `$97ea`: the round-over exit -- clears the "round active" flag
//!     (`$6c4a`, set at `$9658` in the init chain) and returns to its caller.
//!     That return is where the FDC boundary sits: `reports/part10-report.md`
//!     found the round's end without its beginning; this part finds the
//!     beginning and the counter that triggers the end, but the exit's OWN
//!     caller -- whatever loads the next round or the match summary off
//!     floppy -- is outside every snapshot this project has taken.
//!
//!   None of this is implemented: no committed fixture crosses a round
//!   transition, so there is nothing to measure a `GameState` model against.
//!   A future fixture would need a live Hatari watch armed on
//!   `$6c83`/`$6ca0`/`$6d12`/`$6d92` from mid-round through the `$97ea` exit
//!   and into whatever comes next -- `scripts/collect.py --scenario
//!   scenarios/round_watch.yaml` is set up for exactly this, not yet run
//!   through a full round to completion in this pass.

use crate::Player;

/// Far wall transfer: the thrower's own disc reaches the far bound uncaught
/// and possession passes to the other player. ST `$a5ea`-`$a5fa`
/// (`disc::Z_FAR`), gated in the original on `$6ca0.b != 1` -- not modelled
/// here, see the module docs.
///
/// `players[1]` (P2) is the only one who ever serves in this game mode, so
/// the far wall (crossed outbound) is always P2's own disc transferring to
/// P1: `discs_out`/`disc_cap` move DOWN for `players[1]` and UP for
/// `players[0]`, in lockstep. `tests/fixtures/p1_walk.ndjson` frame 220:
/// `players[1]` 4,4 -> 3,3 and `players[0]` 0,0 -> 1,1 in the same tick as
/// `disc[0].own` flips 0 -> 255.
pub fn transfer_at_far_wall(players: &mut [Player; 2]) {
    players[1].discs_out = players[1].discs_out.saturating_sub(1);
    players[1].disc_cap = players[1].disc_cap.saturating_sub(1);
    players[0].discs_out = players[0].discs_out.saturating_add(1);
    players[0].disc_cap = players[0].disc_cap.saturating_add(1);
}

/// Near wall transfer: the exact mirror, ST `$a62c`-`$a63c`
/// (`disc::Z_NEAR`). A disc that transferred to player 1's ledger and
/// returns to the near wall uncaught passes back to player 2: `players[0]`
/// DOWN, `players[1]` UP.
///
/// Measured against `tests/fixtures/handover.ndjson`'s own recorded columns
/// (frame 339: `players[0]` 1,2 -> 0,1, `players[1]` 0,2 -> 1,3, as
/// `disc[1].own` flips 255 -> 0), not against a passing tracecheck replay --
/// see the module docs.
pub fn transfer_at_near_wall(players: &mut [Player; 2]) {
    players[0].discs_out = players[0].discs_out.saturating_sub(1);
    players[0].disc_cap = players[0].disc_cap.saturating_sub(1);
    players[1].discs_out = players[1].discs_out.saturating_add(1);
    players[1].disc_cap = players[1].disc_cap.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `p1_walk.ndjson` frame 220, the far wall: P2 4,4 -> 3,3, P1 0,0 -> 1,1.
    #[test]
    fn far_wall_moves_p2_down_and_p1_up() {
        let mut players = [Player::default(); 2];
        players[1].discs_out = 4;
        players[1].disc_cap = 4;
        transfer_at_far_wall(&mut players);
        assert_eq!((players[0].discs_out, players[0].disc_cap), (1, 1));
        assert_eq!((players[1].discs_out, players[1].disc_cap), (3, 3));
    }

    /// `handover.ndjson` frame 339, the near wall: P1 1,2 -> 0,1, P2 0,2 -> 1,3.
    #[test]
    fn near_wall_moves_p1_down_and_p2_up() {
        let mut players = [Player::default(); 2];
        players[0].discs_out = 1;
        players[0].disc_cap = 2;
        players[1].disc_cap = 2;
        transfer_at_near_wall(&mut players);
        assert_eq!((players[0].discs_out, players[0].disc_cap), (0, 1));
        assert_eq!((players[1].discs_out, players[1].disc_cap), (1, 3));
    }

    /// The two functions are exact mirrors of each other.
    #[test]
    fn far_and_near_are_mirrors() {
        let mut players = [Player::default(); 2];
        players[0].discs_out = 2;
        players[0].disc_cap = 3;
        players[1].discs_out = 5;
        players[1].disc_cap = 6;
        let before = players;
        transfer_at_far_wall(&mut players);
        transfer_at_near_wall(&mut players);
        assert_eq!(players, before);
    }
}
