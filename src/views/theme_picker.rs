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
        // The ROLES in six cells, in the order the board spends them:
        // ground, ink, dim, digits, hot, cool. Not the raw palette — a theme
        // is a role mapping, not a list of colors. (Two themes once shared a
        // palette outright, so a palette strip drew them as the same theme;
        // the press-box studio has its own grays now, and the strip still
        // shows what the board will actually spend.)
        let r = p.roles();
        for c in [r.ground, r.ink, r.dim, r.digits, r.hot, r.cool] {
            spans.push(Span::styled("■", Style::default().fg(c)));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    /// The strip draws ROLES, not the palette. In v3.2 broadcast and studio
    /// shipped the same eleven colors, so a palette strip rendered their two
    /// rows identically and the picker claimed they were the same theme. This
    /// is the cell-level assertion that they are not — and since v3.3 rebuilt
    /// studio as press-box monochrome, the delta is every swatch, not one.
    #[test]
    fn broadcast_and_studio_draw_different_swatch_rows() {
        theme::set_current("broadcast").unwrap();
        let dir = std::env::temp_dir().join(format!("gd-picker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        let (w, h) = (80u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(&app, f, f.area())).unwrap();
        let buf = term.backend().buffer().clone();

        // The six swatch cells on the row whose name column reads `name`.
        let swatches = |name: &str| -> Vec<Color> {
            let y = (0..h)
                .find(|&y| {
                    (0..w)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                        .contains(name)
                })
                .unwrap_or_else(|| panic!("{name} has no row in the picker"));
            let cells: Vec<Color> = (0..w)
                .filter(|&x| buf[(x, y)].symbol() == "■")
                .map(|x| buf[(x, y)].fg)
                .collect();
            assert_eq!(cells.len(), 6, "{name} draws six role swatches");
            cells
        };
        let b = swatches("broadcast");
        let s = swatches("studio");
        let d = swatches("daygame");
        assert_ne!(b, s, "broadcast and studio must not render as the same row");
        assert_ne!(
            b, d,
            "broadcast and daygame must not render as the same row"
        );
        assert_ne!(s, d, "studio and daygame must not render as the same row");
        // Where they differ hardest: the digits swatch (4th) is amber vs white.
        let (bt, st, dt) = (
            theme::builtin("broadcast"),
            theme::builtin("studio"),
            theme::builtin("daygame"),
        );
        assert_eq!(b[3], bt.star, "broadcast's digits swatch is amber");
        assert_eq!(s[3], st.bright, "studio's digits swatch is white");
        assert_eq!(d[3], dt.star, "daygame's digits swatch is deep amber");
        // v3.3: even the ground swatch differs now — studio's press-box ground
        // is a near-black gray, not broadcast's true black.
        assert_ne!(b[0], s[0], "studio's ground swatch is its own");
        // v3.4: daygame is the one theme with the ground/ink relationship
        // reversed — its ground swatch is the lightest of the three, not the
        // darkest.
        assert_ne!(d[0], b[0], "daygame's ground swatch is its own");
        assert_ne!(d[0], s[0], "daygame's ground swatch is its own");
        let luma = |c: Color| -> i32 {
            let Color::Rgb(r, g, bl) = c else {
                panic!("swatch is not truecolor")
            };
            r as i32 + g as i32 + bl as i32
        };
        assert!(
            luma(d[0]) > luma(b[0]) && luma(d[0]) > luma(s[0]),
            "daygame's ground swatch is the light one"
        );
        // Every swatch but `hot` is gray on studio; `hot` is the one chroma
        // the two themes still share.
        for (i, c) in s.iter().enumerate() {
            let Color::Rgb(r, g, bl) = *c else {
                panic!("studio swatch {i} is not truecolor")
            };
            let chroma = r.max(g).max(bl) as i32 - r.min(g).min(bl) as i32;
            if i == 4 {
                assert_eq!(*c, st.live, "studio's hot swatch is the red");
            } else {
                assert!(
                    chroma <= 8,
                    "studio swatch {i} #{r:02x}{g:02x}{bl:02x} has chroma {chroma}, expected <= 8"
                );
            }
        }
        assert_eq!(b[4], s[4], "the identity floor: both spend the same red");
    }
}
