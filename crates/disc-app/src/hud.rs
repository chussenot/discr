//! Pure HUD text formatting -- kept separate from `main.rs`'s drawing calls
//! so it is testable without a macroquad context.

use crate::round::{Match, Mode, Phase};

/// The mode-select menu's title line.
#[must_use]
pub fn menu_title() -> &'static str {
    "DISC"
}

/// One line per selectable mode, with a marker on the current selection.
#[must_use]
pub fn menu_option_line(mode: Mode, selected: Mode) -> String {
    let marker = if mode == selected { ">" } else { " " };
    format!("{marker} {}", mode.label())
}

/// The top HUD line during play: mode and round score.
#[must_use]
pub fn score_line(m: &Match) -> String {
    format!(
        "{}   P1 {} - {} P2",
        m.mode.label(),
        m.round_wins[0],
        m.round_wins[1]
    )
}

/// A fixed-width text energy bar, `filled` characters of `width`, clamped so
/// negative or over-max energy never over/underflows the bar.
#[must_use]
pub fn energy_bar(energy: i16, max: i16, width: usize) -> String {
    if max <= 0 {
        return "?".repeat(width);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let filled = ((i64::from(energy).max(0) * width as i64) / i64::from(max)).clamp(0, width as i64)
        as usize;
    format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
}

/// The banner shown on [`Phase::RoundOver`] / [`Phase::GameOver`].
#[must_use]
pub fn phase_banner(phase: Phase) -> Option<String> {
    match phase {
        Phase::Playing => None,
        Phase::RoundOver { winner, .. } => Some(format!("ROUND TO P{}", winner + 1)),
        Phase::GameOver { winner } => Some(format!("P{} WINS THE MATCH", winner + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_marks_the_selected_mode() {
        assert_eq!(
            menu_option_line(Mode::Training, Mode::Training),
            "> TRAINING"
        );
        assert_eq!(
            menu_option_line(Mode::Challenge, Mode::Training),
            "  CHALLENGE"
        );
    }

    #[test]
    fn score_line_reports_mode_and_wins() {
        let mut m = Match::new(Mode::Challenge);
        m.round_wins = [2, 1];
        assert_eq!(score_line(&m), "CHALLENGE   P1 2 - 1 P2");
    }

    #[test]
    fn energy_bar_scales_to_width() {
        assert_eq!(energy_bar(5, 5, 10), "[##########]");
        assert_eq!(energy_bar(0, 5, 10), "[----------]");
        assert_eq!(energy_bar(2, 4, 10), "[#####-----]");
    }

    #[test]
    fn energy_bar_clamps_out_of_range_values() {
        // Overkill damage or a stale read must never panic or overflow the bar.
        assert_eq!(energy_bar(-3, 5, 4), "[----]");
        assert_eq!(energy_bar(999, 5, 4), "[####]");
    }

    #[test]
    fn energy_bar_handles_a_zero_max_without_dividing_by_it() {
        assert_eq!(energy_bar(0, 0, 3), "???");
    }

    #[test]
    fn phase_banner_is_none_while_playing() {
        assert_eq!(phase_banner(Phase::Playing), None);
    }

    #[test]
    fn phase_banner_names_the_round_and_match_winner() {
        assert_eq!(
            phase_banner(Phase::RoundOver { winner: 0, hold: 0 }),
            Some("ROUND TO P1".to_string())
        );
        assert_eq!(
            phase_banner(Phase::GameOver { winner: 1 }),
            Some("P2 WINS THE MATCH".to_string())
        );
    }
}
