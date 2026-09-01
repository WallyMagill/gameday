//! The RedZone board body: live mosaic + slate strip + sidebar. Extracted
//! verbatim from `app.rs` (Task 3); all state and derived game lists stay on
//! `App`, this module only draws them.

use crate::app::{App, Tab};
use crate::domain::Game;
use crate::app::net::NetChip;
use crate::text::{leading_surname, truncate};
use crate::theme::{self, SidebarHeader};
use crate::tiles::packer::pack;
use crate::tiles::render_tile;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// Team-name column of the sidebar RECORDS rail, in cells.
const RECORDS_NAME_W: usize = 9;

pub fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
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

    // The three headers take their hues from the theme's sidebar mode:
    // multi (live/star/magenta), single (all star) or muted.
    lines.push(Line::from(Span::styled(
        "⚑ GLOBAL ALERTS",
        Style::default()
            .fg(th.sidebar_header(SidebarHeader::Alerts))
            .add_modifier(Modifier::BOLD),
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
            Span::styled(clock.clone(), Style::default().fg(th.clock())),
        ]));
    }
    lines.push(rule(w));

    lines.push(Line::from(Span::styled(
        "TOP PLAYS",
        Style::default()
            .fg(th.sidebar_header(SidebarHeader::TopPlays))
            .add_modifier(Modifier::BOLD),
    )));
    for (game, play) in events.iter().take(5) {
        // A leaderboard line, not a play feed: abbr + surname + clock, the
        // clock right-aligned (reference board). The full sentence already
        // lives in the tile's LAST PLAYS and the ticker's ALERTS lane.
        let clock = play.clock.as_str();
        let who = truncate(
            &leading_surname(&play.text),
            w.saturating_sub(2 + 4 + clock.chars().count() + 1),
        );
        let pad = w.saturating_sub(2 + 4 + who.chars().count() + clock.chars().count());
        lines.push(Line::from(vec![
            Span::styled("★ ", Style::default().fg(th.star)),
            Span::styled(
                format!("{:<4}", play.team),
                Style::default()
                    .fg(App::team_color(game, &play.team))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(who, Style::default().fg(th.league_text(game.league))),
            Span::raw(" ".repeat(pad)),
            Span::styled(clock.to_string(), Style::default().fg(th.clock())),
        ]));
    }
    if events.is_empty() {
        lines.push(Line::from(Span::styled("no scoring yet", Style::default().fg(th.dim))));
    }
    lines.push(rule(w));

    lines.push(Line::from(Span::styled(
        "RECORDS",
        Style::default()
            .fg(th.sidebar_header(SidebarHeader::Records))
            .add_modifier(Modifier::BOLD),
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
        // A name that doesn't fit the 9-cell column shows as the abbr rather
        // than "Buccanee…".
        let name = if team.name.chars().count() <= RECORDS_NAME_W {
            team.name.clone()
        } else {
            team.abbr.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}. ", i + 1), Style::default().fg(th.muted)),
            Span::styled(
                format!("{:<RECORDS_NAME_W$}", truncate(&name, RECORDS_NAME_W)),
                Style::default().fg(th.team_text(team.color)),
            ),
            Span::styled(format!("{win:>3}{loss:>3}"), Style::default().fg(th.fg)),
        ]));
    }
    lines.truncate(inner.height as usize);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// A two-line block sitting on the vertical middle of `area` — the empty
/// board's message reads as a centered statement, not a top-left log line.
fn center_two_lines(area: Rect) -> Rect {
    let h = 2u16.min(area.height);
    Rect {
        y: area.y + (area.height.saturating_sub(h)) / 2,
        height: h,
        ..area
    }
}

fn draw_mosaic(app: &mut App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let games = app.mosaic_games();
    let net = app.net.chip(std::time::Instant::now());
    match app.tab {
        // An active filter that matches nothing names the pattern, the scope
        // it searched and where the pattern IS live, instead of pretending
        // the board is empty. It wraps: the scope is the whole point of the
        // message, so a narrow board must never chop it off.
        _ if games.is_empty() && app.active_filter().is_some() => {
            frame.render_widget(
                Paragraph::new(app.filter_miss_message())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }
        // An empty board during an outage must never read as "no games
        // tonight": name the outage and the error that caused it.
        _ if games.is_empty()
            && matches!(net, NetChip::Offline { .. } | NetChip::NoDataYet) =>
        {
            let detail = match &net {
                NetChip::Offline { error, .. } => format!("last error: {error}"),
                _ => "waiting for the first scoreboard…".to_string(),
            };
            let headline = net.label().unwrap_or_default();
            let color = if matches!(net, NetChip::Offline { .. }) {
                th.live
            } else {
                th.muted
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        headline,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(detail, Style::default().fg(th.muted))),
                ])
                .style(Style::default().bg(th.bg))
                .alignment(Alignment::Center),
                center_two_lines(area),
            );
            return;
        }
        // Home carries every live game now, so an empty Home means nothing is
        // live anywhere — name the next start instead of asking for a pin.
        Tab::Home if games.is_empty() => {
            let msg = match app.next_start() {
                Some(g) => format!(
                    "nothing live · next: {} @ {} {}",
                    g.away.abbr,
                    g.home.abbr,
                    crate::text::fmt_start(
                        g.start.expect("next_start only returns games with a start"),
                        app.now()
                    )
                ),
                None => "nothing live on the enabled boards · :config to add leagues".to_string(),
            };
            frame.render_widget(
                Paragraph::new(msg)
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
        // A click anywhere on the tile selects it (same index space as j/k).
        app.hit_zones
            .push((tile.area, crate::keymap::Hit::Tile(start + i)));
    }
}

fn draw_slate(app: &mut App, frame: &mut Frame, area: Rect) {
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
    let now = app.now();
    let lines: Vec<Line> = app
        .slate_games()
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut spans = if Some(i) == sel {
                vec![
                    Span::styled("▸ ", Style::default().fg(th.star)),
                    Span::styled(
                        slate_line(g, now),
                        Style::default().fg(th.star).add_modifier(Modifier::BOLD),
                    ),
                ]
            } else {
                vec![Span::styled(
                    format!("  {}", slate_line(g, now)),
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
    // Each rendered slate row is a click zone; the row index is relative to
    // the slate (on_hit re-adds the live-tile prefix).
    let visible_rows = app.slate_games().len().min(inner.height as usize);
    for i in 0..visible_rows {
        app.hit_zones.push((
            Rect {
                x: inner.x,
                y: inner.y + i as u16,
                width: inner.width,
                height: 1,
            },
            crate::keymap::Hit::SlateRow(i),
        ));
    }
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
fn slate_line(game: &Game, now: time::OffsetDateTime) -> String {
    match game.status {
        crate::domain::Status::Pre => format!(
            "{:<9} {:>4} @ {:<4} {}",
            game.start
                .map(|t| crate::text::fmt_start(t, now))
                .unwrap_or_else(|| "--:--".into()),
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
