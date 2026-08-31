//! Standings view (`:standings [league]`): one league's table, one block per
//! conference/division group — group header, muted column chrome, aligned
//! W/L/third columns. Read-only; j/k and PgUp/PgDn scroll by line. Team abbrs
//! take their team color when the team is on the league's current board
//! (scoreboard payloads carry colors; the standings feed doesn't), FG
//! otherwise.

use crate::app::App;
use crate::domain::{League, StandingsTable};
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Abbr column width: 4-char abbrs plus a gap.
const ABBR_W: usize = 5;
/// W/L/third value column width (right-aligned): 3-digit season totals (MLB
/// plays 162) plus a leading space.
const VAL_W: usize = 4;

/// Total composed body lines for `table` — the key handler's scroll clamp.
/// Per group: name + column header + rows, with one blank line between groups.
pub fn line_count(table: &StandingsTable) -> usize {
    let rows: usize = table.groups.iter().map(|g| 2 + g.rows.len()).sum();
    rows + table.groups.len().saturating_sub(1)
}

pub fn draw(app: &App, frame: &mut Frame, area: Rect, league: League) {
    let th = theme::current();
    let table = app.standings.get(&league);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let teams: usize = table.map(|t| t.groups.iter().map(|g| g.rows.len()).sum()).unwrap_or(0);
    draw_header(frame, chunks[0], league, teams);
    let Some(table) = table else {
        // The on-demand fetch is in flight (or failed upstream) — say so
        // instead of rendering an empty table.
        frame.render_widget(
            Paragraph::new("no standings yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            chunks[1],
        );
        return;
    };
    let lines = body_lines(app, table);
    // Clamp the offset so the table's tail always fills the pane — you can
    // scroll to the end but never past it into blank space.
    let visible = chunks[1].height.max(1) as usize;
    let offset = app.standings_scroll.min(lines.len().saturating_sub(visible));
    let lines: Vec<Line> = lines.into_iter().skip(offset).take(visible).collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[1],
    );
}

/// `STANDINGS` chip (active-tab style, like the other full-screen views) plus
/// the league and a right-aligned team count for orientation while scrolled.
fn draw_header(frame: &mut Frame, area: Rect, league: League, teams: usize) {
    let th = theme::current();
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "STANDINGS",
            Style::default().fg(th.bg).bg(th.star).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", league.slug().to_uppercase()),
            Style::default().fg(th.muted),
        ),
    ];
    let right = if teams > 0 { format!("{teams} TEAMS ") } else { String::new() };
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let spacer = (area.width as usize).saturating_sub(left_len + right.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(right, Style::default().fg(th.muted)));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// The whole table as lines (scrolling happens on top of this): per group a
/// name row, a muted column-header row, then aligned team rows.
fn body_lines<'a>(app: &App, table: &StandingsTable) -> Vec<Line<'a>> {
    let th = theme::current();
    // One shared name-column width so every group's columns line up.
    let name_w = table
        .groups
        .iter()
        .flat_map(|g| g.rows.iter().map(|r| r.name.chars().count()))
        .max()
        .unwrap_or(4)
        .max(4);
    // The third column renders only when the feed carried one (ties/OTL) —
    // basketball and baseball get plain W/L.
    let third_label = table
        .groups
        .iter()
        .flat_map(|g| &g.rows)
        .find_map(|r| (!r.third_label.is_empty()).then_some(r.third_label));
    let mut lines: Vec<Line> = Vec::new();
    for (gi, group) in table.groups.iter().enumerate() {
        if gi > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            format!(" {}", group.name.to_uppercase()),
            Style::default().fg(th.star).add_modifier(Modifier::BOLD),
        )));
        let mut header = format!(" {:<ABBR_W$}{:<name_w$}{:>VAL_W$}{:>VAL_W$}", "TEAM", "", "W", "L");
        if let Some(label) = third_label {
            header.push_str(&format!("{label:>VAL_W$}"));
        }
        lines.push(Line::from(Span::styled(header, Style::default().fg(th.muted))));
        for row in &group.rows {
            let mut tail = format!("{:>VAL_W$}{:>VAL_W$}", row.wins, row.losses);
            if third_label.is_some() {
                match row.third {
                    Some(t) => tail.push_str(&format!("{t:>VAL_W$}")),
                    None => tail.push_str(&" ".repeat(VAL_W)),
                }
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<ABBR_W$}", row.abbr),
                    Style::default()
                        .fg(abbr_color(app, table.league, &row.abbr))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<name_w$}", row.name.to_uppercase()),
                    Style::default().fg(th.fg),
                ),
                Span::styled(tail, Style::default().fg(th.fg)),
            ]));
        }
    }
    lines
}

/// Team color for an abbr, when that team is on the league's current board;
/// FG otherwise (the standings payload itself carries no colors).
fn abbr_color(app: &App, league: League, abbr: &str) -> Color {
    app.boards
        .get(&league)
        .into_iter()
        .flatten()
        .find_map(|g| {
            if g.away.abbr.eq_ignore_ascii_case(abbr) {
                Some(theme::rgb(g.away.color))
            } else if g.home.abbr.eq_ignore_ascii_case(abbr) {
                Some(theme::rgb(g.home.color))
            } else {
                None
            }
        })
        .unwrap_or(theme::current().fg)
}
