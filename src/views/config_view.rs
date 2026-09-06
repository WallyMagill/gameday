//! In-app config editor (`:config`): league tab toggles, favorites, and the
//! display prefs (theme / sort), one selectable row each.
//! j/k moves, space/enter activates (toggle, remove, edit), h/l cycles the
//! display rows. Every change writes through `Config::save_to` immediately —
//! there is no "save" step to forget. The only free text is the favorite
//! abbr editor the ADD FAVORITE row opens.

use crate::app::App;
use crate::domain::League;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// One selectable row of the editor, in render order. `rows()` is the single
/// source of truth the cursor, the key handler, and the renderer all share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigRow {
    /// Toggle `league` in `enabled_tabs` (space/enter).
    Tab(League),
    /// Remove favorite at this index (space/enter).
    Favorite(usize),
    /// Open the typed-abbr editor (enter).
    AddFavorite,
    /// Cycle with h/l (enter steps forward).
    Theme,
    Sort,
}

/// Every selectable row for the current config state. Section headers are
/// render-only — the cursor never lands on them.
pub fn rows(app: &App) -> Vec<ConfigRow> {
    let mut out: Vec<ConfigRow> = League::ALL.into_iter().map(ConfigRow::Tab).collect();
    out.extend((0..app.config.favorites.len()).map(ConfigRow::Favorite));
    out.push(ConfigRow::AddFavorite);
    out.extend([ConfigRow::Theme, ConfigRow::Sort]);
    out
}

/// One panel's width: the longest row (`+ ADD FAVORITE`, `THEME   ◂ broadcast
/// ▸`) is 26 columns with its marker, and the section rule wants room to read
/// as a rule rather than a dash.
const PANEL_W: usize = 44;
/// Gutter between the two panels.
const GUTTER: usize = 4;
/// Two-panel gate: 2×44 + 4 = 92 columns of content, gated at the same 100 as
/// the standings table so both screens change shape at one width.
const TWO_PANEL_MIN: u16 = crate::views::standings::TWO_COL_MIN;

/// Draws the editor and returns the absolute y of its last content row — the
/// key bar anchors there (spec v3.3 §5), instead of floating on the terminal
/// floor under a gulf of blank rows.
pub fn draw(app: &App, frame: &mut Frame, area: Rect) -> u16 {
    let th = theme::current();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_header(frame, chunks[0]);
    let pane = chunks[1];
    // Spec v3.3 §5: two panels side by side once the frame is wide enough —
    // TABS on the left, FAVORITES + DISPLAY on the right — so the editor is a
    // balanced block, not one narrow column down the left edge.
    let two = pane.width >= TWO_PANEL_MIN;
    let panel_w = PANEL_W.min(pane.width as usize);
    let rows = rows(app);
    let cursor = app.config_cursor.min(rows.len().saturating_sub(1));
    let mut tabs: Vec<Line> = vec![section(" TABS", "space toggles", panel_w)];
    let mut side: Vec<Line> = Vec::new();
    // (column, line) of the cursor's row, so a clipped editor still scrolls
    // it into view.
    let mut cursor_at = (0usize, 0usize);
    let mut favorites_open = false;
    for (i, row) in rows.iter().enumerate() {
        let selected = i == cursor;
        let line = row_line(app, *row, selected);
        match row {
            ConfigRow::Tab(_) => {
                if selected {
                    cursor_at = (0, tabs.len());
                }
                tabs.push(line);
            }
            ConfigRow::Favorite(_) | ConfigRow::AddFavorite => {
                if !favorites_open {
                    favorites_open = true;
                    let hint = if app.config.favorites.is_empty() {
                        "enter adds"
                    } else {
                        "enter removes / adds"
                    };
                    side.push(section(" FAVORITES", hint, panel_w));
                }
                if selected {
                    cursor_at = (1, side.len());
                }
                side.push(line);
            }
            ConfigRow::Theme | ConfigRow::Sort => {
                if *row == ConfigRow::Theme {
                    if !side.is_empty() {
                        side.push(Line::from(""));
                    }
                    side.push(section(" DISPLAY", "h/l cycles", panel_w));
                }
                if selected {
                    cursor_at = (1, side.len());
                }
                side.push(line);
            }
        }
    }
    // `cursor_col` is which rendered column the cursor sits in, so only that
    // column's scroll follows it — v3.3 review: a single shared skip walked
    // the *other* panel's rows off-screen too whenever a tall TABS list
    // pushed FAVORITES/DISPLAY into view, even though that panel had room to
    // just show its own top.
    let (columns, cursor_line, cursor_col) = if two {
        (vec![tabs, side], cursor_at.1, cursor_at.0)
    } else {
        // One column: the sections stack, so the cursor's line moves down by
        // everything the TABS panel drew.
        let offset = tabs.len() + 1;
        let line = if cursor_at.0 == 0 {
            cursor_at.1
        } else {
            cursor_at.1 + offset
        };
        let mut all = tabs;
        all.push(Line::from(""));
        all.extend(side);
        (vec![all], line, 0)
    };
    let height = columns.iter().map(Vec::len).max().unwrap_or(0);
    let pane_h = pane.height.max(1) as usize;
    let visible = height.min(pane_h);
    // Keep the cursor's line on screen when the editor is taller than the
    // pane; a block that fits is centered in the frame instead.
    let skip = (cursor_line + 1).saturating_sub(visible);
    let block_w = if two { panel_w * 2 + GUTTER } else { panel_w };
    let x = pane.x + (pane.width as usize).saturating_sub(block_w) as u16 / 2;
    let y = pane.y + (pane_h - visible) as u16 / 2;
    for (i, col) in columns.into_iter().enumerate() {
        // Only the cursor's own column scrolls; every other column renders
        // from its own top.
        let col_skip = if i == cursor_col { skip } else { 0 };
        let lines: Vec<Line> = col.into_iter().skip(col_skip).take(visible).collect();
        let rect = Rect {
            x: x + (i * (panel_w + GUTTER)) as u16,
            y,
            width: panel_w as u16,
            height: visible as u16,
        };
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(th.bg)),
            rect,
        );
    }
    y + visible.saturating_sub(1) as u16
}

/// `CONFIG` chip (active-tab style, like the other full-screen views) plus a
/// reminder that edits persist immediately.
fn draw_header(frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let spans = vec![
        Span::raw(" "),
        Span::styled(
            "CONFIG",
            Style::default()
                .fg(th.bg)
                .bg(th.star)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  changes save to config.toml immediately",
            Style::default().fg(th.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// A section header: title, its hint, then the board's dim rule out to the
/// panel's edge so the panel reads as a panel and not a ragged list.
fn section(title: &'static str, hint: &'static str, panel_w: usize) -> Line<'static> {
    let th = theme::current();
    let head = format!("{title}  · {hint} ");
    let rule = panel_w.saturating_sub(head.chars().count() + 1);
    Line::from(vec![
        Span::styled(
            title,
            Style::default().fg(th.star).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  · {hint} "), Style::default().fg(th.dim)),
        Span::styled("─".repeat(rule), Style::default().fg(th.dim)),
    ])
}

fn row_line(app: &App, row: ConfigRow, selected: bool) -> Line<'static> {
    let th = theme::current();
    let marker = if selected { " ▸ " } else { "   " };
    let mut spans = vec![Span::styled(marker, Style::default().fg(th.star))];
    let label_style = if selected {
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.fg)
    };
    match row {
        ConfigRow::Tab(league) => {
            let enabled = app.config.enabled_tabs.contains(&league);
            let (mark, style) = if enabled {
                ("[x]", label_style)
            } else {
                ("[ ]", Style::default().fg(th.muted))
            };
            spans.push(Span::styled(format!("{mark} "), style));
            spans.push(Span::styled(league.slug().to_uppercase(), style));
        }
        ConfigRow::Favorite(i) => {
            spans.push(Span::styled("★ ", Style::default().fg(th.star)));
            let text = app
                .config
                .favorites
                .get(i)
                .map(|f| format!("{} {}", f.league.slug().to_uppercase(), f.team_abbr))
                .unwrap_or_default();
            spans.push(Span::styled(text, label_style));
        }
        ConfigRow::AddFavorite => match &app.config_edit {
            // The editor is only ever open with the cursor on this row —
            // opening it is this row's enter action and typing captures j/k.
            Some(buf) => {
                spans.push(Span::styled(
                    "+ ADD FAVORITE: ",
                    Style::default().fg(th.muted),
                ));
                spans.push(Span::styled(
                    buf.clone(),
                    Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled("▌", Style::default().fg(th.star)));
            }
            None => spans.push(Span::styled(
                "+ ADD FAVORITE",
                if selected {
                    label_style
                } else {
                    Style::default().fg(th.muted)
                },
            )),
        },
        ConfigRow::Theme => push_cycler(
            &mut spans,
            "THEME",
            theme::current_name().as_str(),
            label_style,
        ),
        ConfigRow::Sort => push_cycler(
            &mut spans,
            "SORT",
            &app.config.sort.label().to_ascii_lowercase(),
            label_style,
        ),
    }
    Line::from(spans)
}

/// `LABEL   ◂ value ▸` — the h/l affordance for the display rows.
fn push_cycler(spans: &mut Vec<Span<'static>>, label: &str, value: &str, style: Style) {
    let th = theme::current();
    spans.push(Span::styled(format!("{label:<8}"), style));
    spans.push(Span::styled("◂ ", Style::default().fg(th.dim)));
    spans.push(Span::styled(
        value.to_string(),
        Style::default().fg(th.cyan),
    ));
    spans.push(Span::styled(" ▸", Style::default().fg(th.dim)));
}
