//! The RedZone board body: live mosaic + slate strip + sidebar. Extracted
//! verbatim from `app.rs` (Task 3); all state and derived game lists stay on
//! `App`, this module only draws them.

use crate::app::{App, Tab};
use crate::domain::Game;
use crate::text::truncate;
use crate::theme;
use crate::tiles::packer::pack;
use crate::tiles::render_tile;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    let main = if area.width >= 100 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(60), Constraint::Length(22)])
            .split(area);
        draw_sidebar(app, frame, cols[1]);
        cols[0]
    } else {
        area
    };
    let show_slate = matches!(app.tab, Tab::League(_)) && main.height >= 24;
    let mosaic = if show_slate {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(7)])
            .split(main);
        draw_slate(app, frame, parts[1]);
        parts[0]
    } else {
        main
    };
    draw_mosaic(app, frame, mosaic);
}

fn draw_sidebar(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let events = app.scoring_events();
    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "⚑ GLOBAL ALERTS",
        Style::default().fg(th.live).add_modifier(Modifier::BOLD),
    )));
    if events.is_empty() {
        lines.push(Line::from(Span::styled("no alerts", Style::default().fg(th.dim))));
    }
    for (game, play) in events.iter().take(4) {
        let word = theme::scoring_word(game.league);
        let label = format!("{:<4}{:<11}", play.team, word);
        let clock = &play.clock;
        let pad = w.saturating_sub(label.chars().count() + clock.len());
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<4}", play.team),
                Style::default()
                    .fg(App::team_color(game, &play.team))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{word:<11}"), Style::default().fg(th.live)),
            Span::raw(" ".repeat(pad)),
            Span::styled(clock.clone(), Style::default().fg(th.cyan)),
        ]));
    }
    lines.push(rule(w));

    lines.push(Line::from(Span::styled(
        "TOP PLAYS",
        Style::default().fg(th.star).add_modifier(Modifier::BOLD),
    )));
    for (game, play) in events.iter().take(5) {
        // Right-aligned clock column per row (reference board), the play
        // text ellipsis-truncated so it never hard-clips against it.
        let clock = play.clock.as_str();
        let text = truncate(&play.text, w.saturating_sub(2 + clock.chars().count() + 1));
        let pad = w.saturating_sub(2 + text.chars().count() + clock.chars().count());
        lines.push(Line::from(vec![
            Span::styled("★ ", Style::default().fg(th.star)),
            Span::styled(text, Style::default().fg(th.league_accent(game.league))),
            Span::raw(" ".repeat(pad)),
            Span::styled(clock.to_string(), Style::default().fg(th.cyan)),
        ]));
    }
    if events.is_empty() {
        lines.push(Line::from(Span::styled("no scoring yet", Style::default().fg(th.dim))));
    }
    lines.push(rule(w));

    lines.push(Line::from(Span::styled(
        "RECORDS",
        Style::default().fg(th.magenta).add_modifier(Modifier::BOLD),
    )));
    let mut teams: Vec<&crate::domain::Team> = Vec::new();
    let games = app.visible_games();
    for g in &games {
        teams.push(&g.away);
        teams.push(&g.home);
    }
    let mut rows: Vec<(&crate::domain::Team, u32, u32)> = teams
        .into_iter()
        .filter_map(|t| {
            let mut parts = t.record.split('-');
            let win: u32 = parts.next()?.trim().parse().ok()?;
            let loss: u32 = parts.next()?.trim().parse().ok()?;
            Some((t, win, loss))
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    lines.push(Line::from(Span::styled(
        format!("{:<12}{:>3}{:>3}", "TEAM", "W", "L"),
        Style::default().fg(th.muted),
    )));
    for (i, (team, win, loss)) in rows.iter().take(6).enumerate() {
        lines.push(Line::from(vec![
            Span::styled(format!("{}. ", i + 1), Style::default().fg(th.muted)),
            Span::styled(
                format!("{:<9}", truncate(&team.name, 9)),
                Style::default().fg(theme::rgb(team.color)),
            ),
            Span::styled(format!("{win:>3}{loss:>3}"), Style::default().fg(th.fg)),
        ]));
    }
    lines.truncate(inner.height as usize);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_mosaic(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let games = app.mosaic_games();
    match app.tab {
        // An active filter that matches nothing names the pattern instead
        // of pretending the board is empty.
        _ if games.is_empty() && app.active_filter().is_some() => {
            let needle = app.active_filter().unwrap_or_default();
            frame.render_widget(
                Paragraph::new(format!("no games match \"{needle}\" · esc clears"))
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }
        Tab::Home if games.is_empty() => {
            frame.render_widget(
                Paragraph::new("pin a game from nfl (space) · t fav home")
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }
        Tab::League(_) if app.visible_games().is_empty() => {
            frame.render_widget(
                Paragraph::new("next kickoff")
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }
        _ => {}
    }

    // on_key wraps the page, but the board can shrink between keys.
    let page = app.page.min(app.page_count() - 1);
    let packed = pack(&games, area, app.effective_layout(), page);
    let start = packed
        .first()
        .and_then(|tile| games.iter().position(|g| g.id == tile.game.id))
        .unwrap_or(0);
    for (i, tile) in packed.iter().enumerate() {
        // Mosaic tiles are the head of selection_list, so the page-global
        // index start+i compares directly against app.selected.
        let selected = start + i == app.selected;
        render_tile(
            frame,
            tile.area,
            tile.game,
            tile.density,
            selected,
            app.tile_fx(tile.game),
            app.config.score_style,
        );
    }
}

fn draw_slate(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.dim))
        .title(Span::styled(
            " SLATE ",
            Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Selection continues past the live tiles into these rows; the
    // selected row gets the same star accent as a selected tile border.
    let live_len = app.live_games().len();
    let sel = app.selected.checked_sub(live_len);
    let lines: Vec<Line> = app
        .slate_games()
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut spans = if Some(i) == sel {
                vec![
                    Span::styled("▸ ", Style::default().fg(th.star)),
                    Span::styled(
                        slate_line(g),
                        Style::default().fg(th.star).add_modifier(Modifier::BOLD),
                    ),
                ]
            } else {
                vec![Span::styled(
                    format!("  {}", slate_line(g)),
                    Style::default().fg(th.muted),
                )]
            };
            // Odds ride the pre-game row, dim so the slate stays a
            // departure board, not a betting sheet.
            if g.status == crate::domain::Status::Pre {
                if let Some(odds) = &g.odds {
                    spans.push(Span::styled(
                        format!("  {odds}"),
                        Style::default().fg(th.dim),
                    ));
                }
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg).fg(th.muted)),
        inner,
    );
}

fn rule(width: usize) -> Line<'static> {
    let th = theme::current();
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(th.dim),
    ))
}

/// Departure-board slate row (gegen's status grammar): the status token —
/// start time or FINAL — is a fixed-width first column, then the matchup in
/// aligned columns, so rows stack like a split-flap board.
fn slate_line(game: &Game) -> String {
    match game.status {
        crate::domain::Status::Pre => format!(
            "{:<9} {:>4} @ {:<4} {}",
            game.start_time.as_deref().unwrap_or("--:--"),
            game.away.abbr,
            game.home.abbr,
            game.broadcast.as_deref().unwrap_or(""),
        )
        .trim_end()
        .to_string(),
        crate::domain::Status::Final => format!(
            "{:<9} {:>4} {:>3}  {:<4} {:>3}",
            "FINAL", game.away.abbr, game.away_score, game.home.abbr, game.home_score
        ),
        crate::domain::Status::Live => String::new(),
    }
}
