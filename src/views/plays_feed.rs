//! Global scoring feed (`:plays`): one row per scoring play across every
//! enabled board — `[chip] clock ABBR WORD text  matchup score` — newest
//! first as the boards report them. j/k, PgUp/PgDn, and the mouse wheel move
//! the ▸ highlight; the window follows it.

use crate::app::App;
use crate::domain::{Game, Play};
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Draws the feed and returns the absolute y of its last row — the END OF FEED
/// rule when the whole feed fits — so the key bar sits with the list. A feed
/// that fills the pane reports `None` and the bar keeps the
/// terminal floor.
pub fn draw(app: &mut App, frame: &mut Frame, area: Rect) -> Option<u16> {
    let th = theme::current();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    // Scoped so the `derived()` borrow ends before `page_rows` is written —
    // `app` needs to be mutable for that, and every read of `events` happens
    // above this block. `None` (an empty feed has nothing to page) leaves
    // `page_rows` untouched, same as the board's empty state.
    let (result, page_rows) = {
        let events = &app.derived().scoring;
        draw_header(frame, chunks[0], events.len());
        if events.is_empty() {
            frame.render_widget(
                Paragraph::new("no scoring plays yet")
                    .style(Style::default().fg(th.dim).bg(th.bg))
                    .alignment(Alignment::Center),
                chunks[1],
            );
            (Some(chunks[1].y), None)
        } else {
            let sel = app.feed_scroll.min(events.len() - 1);
            // Keep the highlight visible: scroll the window once it walks
            // past the bottom row (same windowing as the Zoom Plays tab).
            let visible = chunks[1].height.max(1) as usize;
            let skip = sel.saturating_sub(visible.saturating_sub(1));
            let mut lines: Vec<Line> = events
                .iter()
                .enumerate()
                .skip(skip)
                .take(visible)
                .map(|(i, (game, play))| feed_row(game, play, i == sel, chunks[1].width as usize))
                .collect();
            // A short feed closes with an end marker so the blank pane below
            // reads as "that's all", not as rows that failed to render. The
            // marker is the board's own rule, drawn to the frame's edge, so
            // the list ends on a line rather than trailing off mid-row.
            if lines.len() < visible {
                let head = "  ── END OF FEED ";
                let rule = (chunks[1].width as usize).saturating_sub(head.chars().count() + 1);
                lines.push(Line::from(Span::styled(
                    format!("{head}{}", "─".repeat(rule)),
                    Style::default().fg(th.dim),
                )));
            }
            let end = chunks[1].y + lines.len().saturating_sub(1) as u16;
            let full = lines.len() >= visible;
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().bg(th.bg)),
                chunks[1],
            );
            ((!full).then_some(end), Some(visible))
        }
    };
    if let Some(visible) = page_rows {
        app.page_rows = Some(visible);
    }
    result
}

/// `PLAYS` chip (active-tab style, like the Zoom tab bar) plus the row count
/// right-aligned so a scrolled feed still says how much there is.
fn draw_header(frame: &mut Frame, area: Rect, count: usize) {
    let th = theme::current();
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "PLAYS",
            Style::default()
                .fg(th.bg)
                .bg(th.star)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  all boards", Style::default().fg(th.muted)),
    ];
    let plural = if count == 1 { "" } else { "S" };
    let right = format!("{count} SCORING PLAY{plural} ");
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let spacer = (area.width as usize).saturating_sub(left_len + right.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(right, Style::default().fg(th.muted)));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// Widest league chip: `[WNBA]`. Every chip is padded to it so the stamp
/// column — and everything after it — lands on one x for every row, the way
/// the board's tier rows hold their stamp column.
const CHIP_W: usize = 6;

/// One feed row: marker, league chip, clock, credited team, scoring word,
/// play text, then the matchup score at the row's right edge for orientation
/// (the row is the width of the frame, not of its text).
fn feed_row<'a>(game: &Game, play: &Play, selected: bool, width: usize) -> Line<'a> {
    let th = theme::current();
    let marker = if selected { "▸ " } else { "  " };
    let text_style = if selected {
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.fg)
    };
    let (lead, trail) = if game.away_score >= game.home_score {
        (game.away_score, game.home_score)
    } else {
        (game.home_score, game.away_score)
    };
    let stamp = crate::tiles::play_stamp(play);
    let word = theme::scoring_word(game.league);
    let score = format!("{}@{} {lead}-{trail} ", game.away.abbr, game.home.abbr);
    // Everything but the play text is fixed-width; the play text gets
    // whatever's left after that budget and the score, minimum a 2-cell gap
    // — long play text is truncated with an ellipsis rather than letting it
    // push the score off the row's right edge.
    let fixed = marker.chars().count()
        + CHIP_W
        + 10 // " {stamp:>8} "
        + crate::board::rows::ABBR_W as usize // team abbr column
        + word.chars().count()
        + 1
        + score.chars().count();
    let text_budget = width.saturating_sub(fixed + 2);
    let text = crate::text::truncate(&play.text, text_budget);
    let spoken = fixed + text.chars().count();
    let gap = width.saturating_sub(spoken).max(2);
    Line::from(vec![
        Span::styled(marker, Style::default().fg(th.star)),
        Span::styled(
            format!(
                "{:<CHIP_W$}",
                format!("[{}]", game.league.slug().to_uppercase())
            ),
            Style::default()
                .fg(th.chip(game.league))
                .add_modifier(Modifier::BOLD),
        ),
        // `play_stamp`, not `play.clock`: a baseball play carries its
        // half-inning in `period` and no clock at all, and printing the clock
        // field left the MLB rows of the feed with a blank stamp column.
        // Widened to 8 cells: a college row's stamp is now `Q2 14:52` (period
        // and clock together), not the clock alone.
        Span::styled(format!(" {stamp:>8} "), Style::default().fg(th.clock())),
        Span::styled(
            // The board's abbr width, not a hardcoded 4 — a 4-letter code
            // (`BOIS`) used to glue straight into the word after it
            // ("BOISPASSING").
            format!("{:<w$}", play.team, w = crate::board::rows::ABBR_W as usize),
            Style::default()
                .fg(App::team_color(game, &play.team))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{word} "),
            Style::default().fg(th.live).add_modifier(Modifier::BOLD),
        ),
        Span::styled(text, text_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(score, Style::default().fg(th.muted)),
    ])
}
