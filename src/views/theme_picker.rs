//! Theme picker (`:theme` with no argument): a panel over the live board
//! listing every loaded theme with a five-swatch strip of its palette. j/k
//! move the cursor AND apply that theme, so the board behind the panel is the
//! preview; Enter keeps it (persisting to config.toml), Esc reverts to the
//! theme that was current when the picker opened.

use crate::app::App;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Panel width: name column + swatches + tag, with breathing room.
const PANEL_W: u16 = 44;

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    let th = theme::current();
    let entries = theme::entries();
    let current = theme::current_name();
    let mut lines: Vec<Line> = Vec::new();
    for entry in &entries {
        let selected = entry.name.eq_ignore_ascii_case(&current);
        let marker = if selected { " ▸ " } else { "   " };
        let name_style = if selected {
            Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.fg)
        };
        let p = &entry.theme;
        let mut spans = vec![
            Span::styled(marker, Style::default().fg(th.star)),
            Span::styled(format!("{:<18}", entry.name), name_style),
        ];
        // The palette in five cells: live, green, cyan, magenta, star.
        for c in [p.live, p.green, p.cyan, p.magenta, p.star] {
            spans.push(Span::styled("■", Style::default().fg(c)));
        }
        spans.push(Span::styled("■", Style::default().fg(p.fg)));
        if entry.user {
            spans.push(Span::styled("  user", Style::default().fg(th.muted)));
        } else if entry.name == "broadcast" {
            spans.push(Span::styled("  default", Style::default().fg(th.muted)));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " J/K PREVIEW · ENTER KEEP · ESC REVERT",
        Style::default().fg(th.dim),
    )));
    let w = PANEL_W.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let panel = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    // Keep the cursor's row inside a short panel.
    let visible = (h.saturating_sub(2)) as usize;
    let cursor = app.theme_cursor.min(entries.len().saturating_sub(1));
    let skip = (cursor + 1).saturating_sub(visible.saturating_sub(2).max(1));
    let lines: Vec<Line> = lines.into_iter().skip(skip).take(visible).collect();
    frame.render_widget(Clear, panel);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.star))
        .title(Span::styled(
            " THEMES ",
            Style::default().fg(th.star).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(th.bg).fg(th.fg)),
        panel,
    );
}
