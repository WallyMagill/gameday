//! In-app config editor (`:config`): league tab toggles, favorites, and the
//! display prefs (theme / score style / layout), one selectable row each.
//! j/k moves, space/enter activates (toggle, remove, edit), h/l cycles the
//! display rows. Every change writes through `Config::save_to` immediately —
//! there is no "save" step to forget. The only free text is the favorite
//! abbr editor the ADD FAVORITE row opens.

use crate::app::App;
use crate::domain::League;
use crate::theme;
use crate::tiles::packer::LayoutPref;
use crate::tiles::ScoreStyle;
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
    Score,
    Layout,
}

/// Every selectable row for the current config state. Section headers are
/// render-only — the cursor never lands on them.
pub fn rows(app: &App) -> Vec<ConfigRow> {
    let mut out: Vec<ConfigRow> = League::ALL.into_iter().map(ConfigRow::Tab).collect();
    out.extend((0..app.config.favorites.len()).map(ConfigRow::Favorite));
    out.push(ConfigRow::AddFavorite);
    out.extend([ConfigRow::Theme, ConfigRow::Score, ConfigRow::Layout]);
    out
}

/// Cycle order for the LAYOUT row (h/l): matches the 1/2/4/s board keys plus
/// auto.
pub const LAYOUTS: [LayoutPref; 5] = [
    LayoutPref::Auto,
    LayoutPref::One,
    LayoutPref::Two,
    LayoutPref::Four,
    LayoutPref::Sidebar,
];

fn layout_label(pref: LayoutPref) -> &'static str {
    match pref {
        LayoutPref::Auto => "auto",
        LayoutPref::One => "one",
        LayoutPref::Two => "two",
        LayoutPref::Four => "four",
        LayoutPref::Sidebar => "sidebar",
    }
}

fn score_label(style: ScoreStyle) -> &'static str {
    match style {
        ScoreStyle::Big => "big",
        ScoreStyle::Compact => "compact",
    }
}

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_header(frame, chunks[0]);

    let rows = rows(app);
    let cursor = app.config_cursor.min(rows.len().saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor_line = 0usize;
    for (i, row) in rows.iter().enumerate() {
        // Section headers, injected before the first row of each section.
        match row {
            ConfigRow::Tab(_) if i == 0 => lines.push(section(" TABS", "space toggles")),
            ConfigRow::Favorite(0) => {
                lines.push(Line::from(""));
                lines.push(section(" FAVORITES", "enter removes / adds"));
            }
            ConfigRow::AddFavorite if app.config.favorites.is_empty() => {
                lines.push(Line::from(""));
                lines.push(section(" FAVORITES", "enter adds"));
            }
            ConfigRow::Theme => {
                lines.push(Line::from(""));
                lines.push(section(" DISPLAY", "h/l cycles"));
            }
            _ => {}
        }
        if i == cursor {
            cursor_line = lines.len();
        }
        lines.push(row_line(app, *row, i == cursor));
    }

    // Keep the cursor's line on screen (small tables fit whole; a long
    // favorites list scrolls under it).
    let visible = chunks[1].height.max(1) as usize;
    let skip = (cursor_line + 1).saturating_sub(visible);
    let lines: Vec<Line> = lines.into_iter().skip(skip).take(visible).collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[1],
    );
}

/// `CONFIG` chip (active-tab style, like the other full-screen views) plus a
/// reminder that edits persist immediately.
fn draw_header(frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let spans = vec![
        Span::raw(" "),
        Span::styled(
            "CONFIG",
            Style::default().fg(th.bg).bg(th.star).add_modifier(Modifier::BOLD),
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

fn section(title: &'static str, hint: &'static str) -> Line<'static> {
    let th = theme::current();
    Line::from(vec![
        Span::styled(title, Style::default().fg(th.star).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  · {hint}"), Style::default().fg(th.dim)),
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
                spans.push(Span::styled("+ ADD FAVORITE: ", Style::default().fg(th.muted)));
                spans.push(Span::styled(
                    buf.clone(),
                    Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled("▌", Style::default().fg(th.star)));
            }
            None => spans.push(Span::styled(
                "+ ADD FAVORITE",
                if selected { label_style } else { Style::default().fg(th.muted) },
            )),
        },
        ConfigRow::Theme => {
            push_cycler(&mut spans, "THEME", theme::current_name().as_str(), label_style)
        }
        ConfigRow::Score => push_cycler(
            &mut spans,
            "SCORE",
            score_label(app.config.score_style),
            label_style,
        ),
        ConfigRow::Layout => push_cycler(
            &mut spans,
            "LAYOUT",
            layout_label(app.config.layout),
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
    spans.push(Span::styled(value.to_string(), Style::default().fg(th.cyan)));
    spans.push(Span::styled(" ▸", Style::default().fg(th.dim)));
}
