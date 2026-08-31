//! Global scoring feed (`:plays`): one row per scoring play across every
//! enabled board — `[chip] clock ABBR WORD text  matchup score` — newest
//! first as the boards report them. j/k and PgUp/PgDn move the ▸ highlight;
//! the window follows it. Wheel scrolling arrives with mouse support.

use crate::app::App;
use crate::domain::{Game, Play};
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let events = app.scoring_events();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_header(frame, chunks[0], events.len());
    if events.is_empty() {
        frame.render_widget(
            Paragraph::new("no scoring plays yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            chunks[1],
        );
        return;
    }
    let sel = app.feed_scroll.min(events.len() - 1);
    // Keep the highlight visible: scroll the window once it walks past the
    // bottom row (same windowing as the Zoom Plays tab).
    let visible = chunks[1].height.max(1) as usize;
    let skip = sel.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = events
        .iter()
        .enumerate()
        .skip(skip)
        .take(visible)
        .map(|(i, (game, play))| feed_row(game, play, i == sel))
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[1],
    );
}

/// `PLAYS` chip (active-tab style, like the Zoom tab bar) plus the row count
/// right-aligned so a scrolled feed still says how much there is.
fn draw_header(frame: &mut Frame, area: Rect, count: usize) {
    let th = theme::current();
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "PLAYS",
            Style::default().fg(th.bg).bg(th.star).add_modifier(Modifier::BOLD),
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

/// One feed row: marker, league chip, clock, credited team, scoring word,
/// play text, then the matchup score dimly at the end for orientation.
fn feed_row<'a>(game: &Game, play: &Play, selected: bool) -> Line<'a> {
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
    Line::from(vec![
        Span::styled(marker, Style::default().fg(th.star)),
        Span::styled(
            format!("[{}]", game.league.slug().to_uppercase()),
            Style::default()
                .fg(th.league_accent(game.league))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {:>5} ", play.clock), Style::default().fg(th.cyan)),
        Span::styled(
            format!("{:<4}", play.team),
            Style::default()
                .fg(App::team_color(game, &play.team))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", theme::scoring_word(game.league)),
            Style::default().fg(th.live).add_modifier(Modifier::BOLD),
        ),
        Span::styled(play.text.clone(), text_style),
        Span::styled(
            format!("  {}@{} {lead}-{trail}", game.away.abbr, game.home.abbr),
            Style::default().fg(th.muted),
        ),
    ])
}
