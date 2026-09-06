//! The per-period box-score rows every scoreboard owes you: `1 2 3 … R`,
//! then a row per side, with baseball's `H E`.
//!
//! Two views draw it — the zoom's Overview (spec §5) and `:tv` — so it lives
//! in `board/` beside the hero rather than as a copy in each. The totals are
//! the game's own score, never a sum of the periods: a feed can hand us a
//! partial linescore and the score is still the truth.

use crate::domain::{Extras, Game};
use crate::theme::{self, Theme};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Height of the block, in rows: the period header plus one row per side.
pub const ROWS: u16 = 3;

/// `None` when the feed carried no linescore. The team rows wear the same
/// [`theme::hero_pair`] colors the hero's digits use (spec v3.3 §5) — the
/// gated `team_text` role could fall back to plain `fg` (white) with the
/// discipline off, which is exactly the white-digit miss the v3.2 review
/// caught on an NYY row. `hero_pair` has no such fallback path, and it is
/// also the one place two lookalike navies are guaranteed to separate.
pub fn linescore_lines(game: &Game, th: &Theme) -> Option<Vec<Line<'static>>> {
    if game.linescore.is_empty() {
        return None;
    }
    let (away_color, home_color, _) = theme::hero_pair(th, game.away.color, game.home.color);
    // Baseball's hits/errors ride the same row as R; other sports have none.
    let (hits, errors) = match &game.extras {
        Extras::Baseball { hits, errors } => (*hits, *errors),
        _ => (None, None),
    };
    let cell = |s: String| format!("{s:>3}");
    let mut head = format!("{:<5}", "");
    let mut away = format!("{:<5}", game.away.abbr);
    let mut home = format!("{:<5}", game.home.abbr);
    for (i, (a, h)) in game.linescore.iter().enumerate() {
        head.push_str(&cell((i + 1).to_string()));
        away.push_str(&cell(a.to_string()));
        home.push_str(&cell(h.to_string()));
    }
    head.push_str(&format!("{:>4}", "R"));
    away.push_str(&format!("{:>4}", game.away_score));
    home.push_str(&format!("{:>4}", game.home_score));
    for (label, pair) in [("H", hits), ("E", errors)] {
        let Some((a, h)) = pair else { continue };
        head.push_str(&cell(label.to_string()));
        away.push_str(&cell(a.to_string()));
        home.push_str(&cell(h.to_string()));
    }
    let team_row = |text: String, color: ratatui::style::Color| {
        Line::from(Span::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    };
    Some(vec![
        Line::from(Span::styled(head, Style::default().fg(th.roles().dim))),
        team_row(away, away_color),
        team_row(home, home_color),
    ])
}
