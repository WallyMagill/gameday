//! The per-period box-score rows every scoreboard owes you: `1 2 3 … R`,
//! then a row per side, with baseball's `H E`.
//!
//! Two views draw it — the zoom's Overview (spec §5) and `:tv` — so it lives
//! in `board/` beside the hero rather than as a copy in each. The totals are
//! the game's own score, never a sum of the periods: a feed can hand us a
//! partial linescore and the score is still the truth.

use crate::domain::{Extras, Game};
use crate::theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Height of the block, in rows: the period header plus one row per side.
pub const ROWS: u16 = 3;

/// `None` when the feed carried no linescore.
pub fn lines(game: &Game) -> Option<Vec<Line<'static>>> {
    if game.linescore.is_empty() {
        return None;
    }
    let th = theme::current();
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
    let team_row = |text: String, color: [u8; 3]| {
        Line::from(Span::styled(
            text,
            Style::default().fg(th.team_text(color)).add_modifier(Modifier::BOLD),
        ))
    };
    Some(vec![
        Line::from(Span::styled(head, Style::default().fg(th.roles().dim))),
        team_row(away, game.away.color),
        team_row(home, game.home.color),
    ])
}
