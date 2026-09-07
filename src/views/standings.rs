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

/// Abbr column width — the row grid's [`crate::board::rows::ABBR_W`]: four
/// cells for the abbr, the fifth is air.
const ABBR_W: usize = crate::board::rows::ABBR_W as usize;
/// W/L/third value column width (right-aligned): 3-digit season totals (MLB
/// plays 162) plus a leading space.
const VAL_W: usize = 4;

/// What the Standings view says for college football when the FBS fetch came
/// back with nothing. `standings_url` pins `?group=80`, which answered with
/// 138 FBS teams on 2026-08-31 — so this is the failure path, and the copy
/// says only that (no table right now), never that ESPN has none to give.
const CFB_NO_TABLE: &str = "no FBS standings right now";

/// Total composed body lines for `table` — the key handler's scroll clamp.
/// Per group: name + column header + rows, with one blank line between groups.
pub fn line_count(table: &StandingsTable) -> usize {
    let rows: usize = table.groups.iter().map(|g| 2 + g.rows.len()).sum();
    rows + table.groups.len().saturating_sub(1)
}

/// Two-column gate. Receipt: a conference table is 48 columns
/// at its widest (abbr 5 + the longest NFL club name 26 + three 4-col value
/// columns + a leading space, rounded up for the rule), so two of them plus a
/// 4-column gutter need 100. Below that the table stays one column.
pub const TWO_COL_MIN: u16 = 100;
/// Gutter between the two tables.
const GUTTER: u16 = 4;

/// How many table columns fit `width` for `table`: two once the frame is wide
/// enough and there is something to split, one otherwise.
///
/// Receipt for the `> 4`: a one-group table splits by halving its rows
/// ([`sections`], `div_ceil(2)`), and each half pays a title line of its own
/// (`NAME · 1-n`) plus the column header. At 5 rows the halves are 3 and 2 —
/// the short side is still a table. At 4 they are 2 and 2, so the second
/// column costs two lines of chrome to show two lines of teams and reads as a
/// stub beside 48 columns of air. Four is therefore the last row count that
/// stays one column; five is the first that earns a split. (Multi-group
/// leagues split by group instead and never consult this number.)
fn column_count(table: &StandingsTable, width: u16) -> usize {
    if width >= TWO_COL_MIN && (table.groups.len() > 1 || table.groups[0].rows.len() > 4) {
        2
    } else {
        1
    }
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

/// Draws the table and returns the absolute y of its last content row when the
/// whole table fits — the key bar anchors there. A clipped
/// table fills the pane, so it reports `None` and the bar stays on the floor.
pub fn draw(app: &mut App, frame: &mut Frame, area: Rect, league: League) -> Option<u16> {
    let th = theme::current();
    let table = app.standings.get(&league);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let teams: usize = table
        .map(|t| t.groups.iter().map(|g| g.rows.len()).sum())
        .unwrap_or(0);
    draw_header(frame, chunks[0], league, teams, table.and_then(label));
    // A table with no groups is as empty as no table at all — both get the
    // message, never a blank pane pretending to be a standings board.
    let Some(table) = table.filter(|t| !t.groups.is_empty()) else {
        // The on-demand fetch is in flight (or failed upstream) — say so.
        // For college football, say the specific thing rather than implying
        // the table is a moment away.
        // A fetch that failed is not a fetch in flight: name the error and
        // say a retry is coming, so an empty table never reads as "ESPN has
        // no standings for this league".
        let msg = match app.aux_error(league, "standings") {
            Some(err) => format!("standings unavailable · {err} · retrying"),
            None => match league {
                League::Cfb => CFB_NO_TABLE.to_string(),
                _ => "no standings yet".to_string(),
            },
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            chunks[1],
        );
        return Some(chunks[1].y);
    };
    // A wide frame gets two tables side by side instead of one
    // narrow column against a half-empty right side.
    let pane = chunks[1];
    let cols = column_count(table, pane.width);
    let col_w = if cols == 2 {
        (pane.width.saturating_sub(GUTTER) / 2) as usize
    } else {
        pane.width as usize
    };
    let columns = column_lines(app, table, cols, col_w);
    let total = columns.iter().map(Vec::len).max().unwrap_or(0);
    // Clamp the offset so the table's tail always fills the pane — you can
    // scroll to the end but never past it into blank space. The key handler
    // clamps against the offset recorded here, which is column-count aware:
    // two columns halve how far there is to scroll.
    let (body, max_offset) = window(total, pane.height as usize);
    app.standings_max_scroll = Some(max_offset);
    app.page_rows = Some(body);
    // The clamp is written back, not just rendered with: a frame that got
    // wider (or a table that got shorter) leaves a stored offset past the new
    // end, and a stale offset spends the next j/k snapping itself back —
    // a keypress the reader sees do nothing.
    app.standings_scroll = app.standings_scroll.min(max_offset);
    let offset = app.standings_scroll;
    for (i, col) in columns.into_iter().enumerate() {
        let lines: Vec<Line> = col.into_iter().skip(offset).take(body).collect();
        let rect = Rect {
            x: pane.x + (i as u16) * (col_w as u16 + GUTTER),
            y: pane.y,
            width: col_w as u16,
            height: body.min(pane.height as usize) as u16,
        };
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(th.bg)),
            rect,
        );
    }
    if total > body {
        let marker = Rect {
            y: pane.y + body as u16,
            height: 1,
            ..pane
        };
        frame.render_widget(
            Paragraph::new(more_marker(offset, total.saturating_sub(offset + body)))
                .style(Style::default().bg(th.bg)),
            marker,
        );
        // Clipped: the table owns the pane, so the key bar keeps the floor.
        return None;
    }
    Some(pane.y + total.saturating_sub(1) as u16)
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
    let phase = match table.season_type {
        Some(1) => " PRESEASON",
        Some(3) => " POSTSEASON",
        _ => "",
    };
    match &table.season {
        Some(season) => Some(format!("{season}{phase}")),
        None => table
            .fetched_at
            .map(|t| format!("updated {}{phase}", crate::text::fmt_hm12(t))),
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
            Style::default()
                .fg(th.bg)
                .bg(th.star)
                .add_modifier(Modifier::BOLD),
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
    let right = if teams > 0 {
        format!("{teams} TEAMS ")
    } else {
        String::new()
    };
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let spacer = (area.width as usize).saturating_sub(left_len + right.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(right, Style::default().fg(th.muted)));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// One rendered section of a column: a title (the group's name, or a half of
/// a single-group league's table named by its row range) and the rows under
/// it.
struct Section<'a> {
    title: String,
    rows: &'a [crate::domain::StandingRow],
}

/// The table's sections dealt into `cols` columns: by group when the league
/// has groups to split (NFL's two conferences), by halving the row list when
/// it is one table (EPL). Balanced by height, so neither column runs long.
fn sections<'a>(table: &'a StandingsTable, cols: usize) -> Vec<Vec<Section<'a>>> {
    let title = |g: &crate::domain::StandingsGroup| g.name.to_uppercase();
    if cols < 2 {
        return vec![table
            .groups
            .iter()
            .map(|g| Section {
                title: title(g),
                rows: &g.rows,
            })
            .collect()];
    }
    if table.groups.len() == 1 {
        // One table, no groups to split: halve the rows and say which slice
        // each column holds, so the split never reads as two leagues.
        let g = &table.groups[0];
        let half = g.rows.len().div_ceil(2);
        let (a, b) = g.rows.split_at(half);
        let name = title(g);
        return vec![
            vec![Section {
                title: format!("{name} · 1-{}", a.len()),
                rows: a,
            }],
            vec![Section {
                title: format!("{name} · {}-{}", a.len() + 1, g.rows.len()),
                rows: b,
            }],
        ];
    }
    // Fill the left column until it holds half the lines, then the right.
    let height = |g: &crate::domain::StandingsGroup| 2 + g.rows.len();
    let total: usize = table.groups.iter().map(height).sum();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut used = 0usize;
    for g in &table.groups {
        let section = Section {
            title: title(g),
            rows: &g.rows,
        };
        if left.is_empty() || used + height(g) <= total.div_ceil(2) {
            used += height(g);
            left.push(section);
        } else {
            right.push(section);
        }
    }
    vec![left, right]
}

/// Each column's lines: per section a titled rule row, a muted column-header
/// row, then aligned team rows, with a blank line between sections.
fn column_lines<'a>(
    app: &App,
    table: &StandingsTable,
    cols: usize,
    col_w: usize,
) -> Vec<Vec<Line<'a>>> {
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
    let mut out: Vec<Vec<Line>> = Vec::new();
    for column in sections(table, cols) {
        let mut lines: Vec<Line> = Vec::new();
        for (si, section) in column.iter().enumerate() {
            if si > 0 {
                lines.push(Line::from(""));
            }
            lines.push(title_rule(&section.title, col_w));
            let mut header = format!(
                " {:<ABBR_W$}{:<name_w$}{:>VAL_W$}{:>VAL_W$}",
                "TEAM", "", "W", "L"
            );
            if let Some(label) = third_label {
                header.push_str(&format!("{label:>VAL_W$}"));
            }
            lines.push(Line::from(Span::styled(
                header,
                Style::default().fg(th.muted),
            )));
            for row in section.rows {
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
        out.push(lines);
    }
    out
}

/// A section's title row: the name, then the board's own dim rule out to the
/// column's edge, so a table never ends in a ragged half-empty row.
fn title_rule<'a>(title: &str, col_w: usize) -> Line<'a> {
    let th = theme::current();
    let used = title.chars().count() + 2;
    let rule = col_w.saturating_sub(used + 1);
    Line::from(vec![
        Span::styled(
            format!(" {title} "),
            Style::default().fg(th.star).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(rule), Style::default().fg(th.dim)),
    ])
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
