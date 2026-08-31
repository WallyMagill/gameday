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
        ZoomTab::Stats => draw_stats(app, frame, chunks[1], &game),
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

/// Value columns are sized to the widest value ("6-17", "31:26"), floored at
/// the 3-char abbr header width plus a space.
const STAT_COL_MIN: usize = 4;

/// Box score: comparison rows (label + away/home value columns under the team
/// abbrs) scrolled by j/k, with the LEADERS block pinned below. Empty until
/// the ~30s stats poll answers — says so instead of rendering a blank pane.
fn draw_stats(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let stats = app.stats.get(&game.id);
    let Some(stats) = stats.filter(|s| !s.rows.is_empty() || !s.leaders.is_empty()) else {
        frame.render_widget(
            Paragraph::new("no stats yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    // LEADERS gets its rows plus a header, but never more than half the pane;
    // the comparison table keeps the rest.
    let leaders_h = if stats.leaders.is_empty() {
        0
    } else {
        (stats.leaders.len() as u16 + 2).min(area.height / 2)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(leaders_h)])
        .split(area);

    let col_w = stats
        .rows
        .iter()
        .flat_map(|r| [r.away.chars().count(), r.home.chars().count()])
        .max()
        .unwrap_or(0)
        .max(STAT_COL_MIN);
    // Label column hugs the widest label instead of stretching to the pane
    // edge — a 120-col pane would otherwise put ~70 blank cells between a
    // label and its values.
    let widest_label = stats
        .rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    let label_w = widest_label.min((chunks[0].width as usize).saturating_sub(2 * (col_w + 2) + 3));
    let mut lines = vec![Line::from(vec![
        Span::raw(" ".repeat(label_w + 3)),
        Span::styled(
            format!("{:>col_w$}", game.away.abbr),
            Style::default()
                .fg(theme::rgb(game.away.color))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:>col_w$}", game.home.abbr),
            Style::default()
                .fg(theme::rgb(game.home.color))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    // j/k move a highlight through the rows; the window follows it, minus the
    // abbr header line.
    let sel = app.zoom_scroll.min(stats.rows.len().saturating_sub(1));
    let visible = (chunks[0].height.max(1) as usize).saturating_sub(1).max(1);
    let skip = sel.saturating_sub(visible.saturating_sub(1));
    for (i, row) in stats.rows.iter().enumerate().skip(skip).take(visible) {
        let marker = if i == sel { "▸ " } else { "  " };
        let label_style = if i == sel {
            Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        let mut label: String = row.label.chars().take(label_w).collect();
        let pad = label_w.saturating_sub(label.chars().count());
        label.push_str(&" ".repeat(pad));
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(th.star)),
            Span::styled(label, label_style),
            Span::raw(" "),
            Span::styled(format!("{:>col_w$}", row.away), Style::default().fg(th.fg)),
            Span::raw("  "),
            Span::styled(format!("{:>col_w$}", row.home), Style::default().fg(th.fg)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[0],
    );

    if leaders_h > 0 {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " LEADERS",
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            )),
        ];
        let label_w = stats
            .leaders
            .iter()
            .map(|l| l.label.chars().count())
            .max()
            .unwrap_or(0);
        for leader in &stats.leaders {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<4}", leader.team),
                    Style::default()
                        .fg(App::team_color(game, &leader.team))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<label_w$}  ", leader.label.to_uppercase()),
                    Style::default().fg(th.muted),
                ),
                Span::styled(leader.text.clone(), Style::default().fg(th.fg)),
            ]));
        }
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(th.bg)),
            chunks[1],
        );
    }
}
