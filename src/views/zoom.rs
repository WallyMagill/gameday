//! Zoomed single-game view (`z`/Enter): a tab bar — OVERVIEW │ PLAYS │ STATS
//! — over one game's full-body surface. Overview is the expanded tile the old
//! focus mode rendered; Plays is the game's full feed with a j/k highlight;
//! Stats fills in Task 4.

use crate::app::App;
use crate::domain::Game;
use crate::theme;
use crate::tiles::packer::{pack, LayoutPref};
use crate::tiles::render_tile;
use crate::views::ZoomTab;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(app: &App, frame: &mut Frame, area: Rect, game_id: &str, tab: ZoomTab) {
    let th = theme::current();
    let Some(game) = app.game_by_id(game_id) else {
        // The zoomed game left every board (final pruned, feed hiccup).
        frame.render_widget(
            Paragraph::new(format!("game {game_id:?} is not on any board · esc back"))
                .style(Style::default().fg(th.muted).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_tab_bar(frame, chunks[0], &game, tab);
    match tab {
        ZoomTab::Overview => draw_overview(app, frame, chunks[1], &game),
        ZoomTab::Plays => draw_plays(app, frame, chunks[1], &game),
        ZoomTab::Stats => draw_stats(frame, chunks[1]),
    }
}

/// `OVERVIEW │ PLAYS │ STATS` — active tab in the same chip style as the
/// active league tab; matchup + score right-aligned for orientation.
fn draw_tab_bar(frame: &mut Frame, area: Rect, game: &Game, active: ZoomTab) {
    let th = theme::current();
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in ZoomTab::ALL.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(th.dim)));
        }
        let style = if tab == active {
            Style::default().fg(th.bg).bg(th.star).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        spans.push(Span::styled(tab.label(), style));
    }
    let right = format!(
        "{} {} @ {} {} ",
        game.away.abbr, game.away_score, game.home.abbr, game.home_score
    );
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let spacer = (area.width as usize).saturating_sub(left_len + right.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(right, Style::default().fg(th.muted)));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// The old focus view: the game as one full-area tile (LED digits, field
/// bar, plays, timeline all live inside the tile renderer).
fn draw_overview(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    let fx = app.tile_fx(game);
    let one = [game.clone()];
    for tile in pack(&one, area, LayoutPref::One, 0) {
        render_tile(
            frame,
            tile.area,
            tile.game,
            tile.density,
            true,
            fx,
            app.config.score_style,
        );
    }
}

/// Full play feed for this game (its `last_plays`, newest first as mapped);
/// `app.zoom_scroll` is the highlighted row, kept on screen by a simple
/// scroll window. Wheel scrolling arrives with mouse support (Task 9).
fn draw_plays(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    if game.last_plays.is_empty() {
        frame.render_widget(
            Paragraph::new("no plays yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let sel = app.zoom_scroll.min(game.last_plays.len() - 1);
    // Keep the highlight visible: scroll the window once it walks past the
    // bottom row.
    let visible = area.height.max(1) as usize;
    let skip = sel.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = game
        .last_plays
        .iter()
        .enumerate()
        .skip(skip)
        .take(visible)
        .map(|(i, play)| {
            let marker = if i == sel { "▸ " } else { "  " };
            let text_style = if play.scoring {
                Style::default().fg(th.live).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.fg)
            };
            let mut spans = vec![
                Span::styled(marker, Style::default().fg(th.star)),
                Span::styled(format!("{:>5} ", play.clock), Style::default().fg(th.cyan)),
                Span::styled(
                    format!("{:<4}", play.team),
                    Style::default()
                        .fg(App::team_color(game, &play.team))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(play.text.clone(), text_style),
            ];
            if i == sel {
                spans[3] = spans[3].clone().style(text_style.add_modifier(Modifier::BOLD));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        area,
    );
}

/// Box-score stats land in Task 4; until then the tab says so plainly.
fn draw_stats(frame: &mut Frame, area: Rect) {
    let th = theme::current();
    frame.render_widget(
        Paragraph::new("no stats yet")
            .style(Style::default().fg(th.dim).bg(th.bg))
            .alignment(Alignment::Center),
        area,
    );
}
