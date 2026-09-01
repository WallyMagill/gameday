//! Standings view (`:standings [league]`): one league's table, one block per
//! conference/division group — group header, muted column chrome, aligned
//! W/L/third columns. Read-only; j/k and PgUp/PgDn scroll by line. Team abbrs
//! take their team color when the team is on the league's current board
//! (scoreboard payloads carry colors; the standings feed doesn't), FG
//! otherwise. A table taller than the pane gets a one-row marker at the
//! bottom saying how many lines are hidden above/below, so a clipped
//! conference never reads as a complete one.

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

/// What the Standings view says for college football when the FBS fetch came
/// back with nothing. `standings_url` pins `?group=80`, which answered with
/// 138 FBS teams on 2026-08-31 — so this is the failure path, and the copy
/// says only that (no table right now), never that ESPN has none to give.
const CFB_NO_TABLE: &str =
    "no FBS standings right now · try :standings <conf> (coming in v3.3)";

/// Total composed body lines for `table` — the key handler's scroll clamp.
/// Per group: name + column header + rows, with one blank line between groups.
pub fn line_count(table: &StandingsTable) -> usize {
    let rows: usize = table.groups.iter().map(|g| 2 + g.rows.len()).sum();
    rows + table.groups.len().saturating_sub(1)
}

/// How a table of `lines` lines fits a pane of `pane` rows: the number of
/// table rows shown (the pane, minus one for the more-marker when clipped)
/// and the largest top offset that still fills those rows.
pub fn window(lines: usize, pane: usize) -> (usize, usize) {
    let pane = pane.max(1);
    if lines <= pane {
        return (pane, 0);
    }
    let body = (pane - 1).max(1);
    (body, lines.saturating_sub(body))
}

pub fn draw(app: &mut App, frame: &mut Frame, area: Rect, league: League) {
    let th = theme::current();
    let table = app.standings.get(&league);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let teams: usize = table.map(|t| t.groups.iter().map(|g| g.rows.len()).sum()).unwrap_or(0);
    draw_header(frame, chunks[0], league, teams, table.and_then(label));
    // A table with no groups is as empty as no table at all — both get the
    // message, never a blank pane pretending to be a standings board.
    let Some(table) = table.filter(|t| !t.groups.is_empty()) else {
        // The on-demand fetch is in flight (or failed upstream) — say so.
        // For college football, say the specific thing rather than implying
        // the table is a moment away.
        let msg = match league {
            League::Cfb => CFB_NO_TABLE,
            _ => "no standings yet",
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            chunks[1],
        );
        return;
    };
    let lines = body_lines(app, table);
    // Clamp the offset so the table's tail always fills the pane — you can
    // scroll to the end but never past it into blank space. The key handler
    // clamps against the same row count, recorded here.
    let (body, max_offset) = window(lines.len(), chunks[1].height as usize);
    app.standings_visible = body;
    let offset = app.standings_scroll.min(max_offset);
    let total = lines.len();
    let mut lines: Vec<Line> = lines.into_iter().skip(offset).take(body).collect();
    if total > body {
        lines.push(more_marker(offset, total.saturating_sub(offset + body)));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[1],
    );
}

/// The scroll affordance for a clipped table: `▲ 12 ABOVE  ▼ 8 BELOW` (each
/// half only when non-zero) plus the keys that move it.
fn more_marker<'a>(above: usize, below: usize) -> Line<'a> {
    let th = theme::current();
    let mut parts = Vec::new();
    if above > 0 {
        parts.push(format!("▲ {above} ABOVE"));
    }
    if below > 0 {
        parts.push(format!("▼ {below} BELOW"));
    }
    Line::from(vec![
        Span::styled(
            format!(" {}", parts.join("  ")),
            Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  J/K SCROLL", Style::default().fg(th.dim)),
    ])
}

/// What the header prints after the league: the season the feed labeled the
/// table with ("2025-26" — NBA/NHL/CBB serve last season's all summer), or
/// failing that when we took the snapshot ("updated 9:41 PM"). None when we
/// know neither, and then the header says nothing rather than something
/// reassuring.
fn label(table: &StandingsTable) -> Option<String> {
    match &table.season {
        Some(season) => Some(season.clone()),
        None => table
            .fetched_at
            .map(|t| format!("updated {}", crate::text::fmt_hm12(t))),
    }
}

/// `STANDINGS` chip (active-tab style, like the other full-screen views) plus
/// the league, the season/updated label, and a right-aligned team count for
/// orientation while scrolled.
fn draw_header(frame: &mut Frame, area: Rect, league: League, teams: usize, label: Option<String>) {
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
    if let Some(label) = label {
        spans.push(Span::styled(
            format!("  ·  {label}"),
            Style::default().fg(th.dim),
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::window;

    #[test]
    fn window_reserves_a_marker_row_only_when_clipped() {
        assert_eq!(window(12, 30), (30, 0), "fits: whole pane, no scroll");
        assert_eq!(window(30, 30), (30, 0), "exact fit: no marker");
        // 41 lines in 22 rows: 21 table rows + the marker, top offset 20.
        assert_eq!(window(41, 22), (21, 20));
        assert_eq!(window(5, 1), (1, 4), "a one-row pane still shows a row");
    }
}
