use crate::domain::Team;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// A two-color pixel mark. `0` transparent, `1` primary, `2` alt.
/// Drawn with half-blocks, so a 12-row pixmap occupies 6 terminal rows.
pub struct Pixmap {
    pub width: usize,
    pub rows: Vec<Vec<u8>>,
}

pub fn parse_pixmap(raw: &str) -> Option<Pixmap> {
    let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() || lines.len() > 16 {
        return None;
    }
    let width = lines.iter().map(|l| l.chars().count()).max()?;
    if width > 16 {
        return None;
    }
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        let mut row = vec![0u8; width];
        for (i, ch) in line.chars().enumerate() {
            row[i] = match ch {
                '#' => 1,
                '+' => 2,
                '.' | ' ' => 0,
                _ => return None,
            };
        }
        rows.push(row);
    }
    Some(Pixmap { width, rows })
}

pub fn load_logo(key: &str) -> Option<Pixmap> {
    parse_pixmap(match key {
        "nfl/kc" => include_str!("../../assets/logos/nfl/kc.px"),
        "nfl/tb" => include_str!("../../assets/logos/nfl/tb.px"),
        "nba/den" => include_str!("../../assets/logos/nba/den.px"),
        "nba/bos" => include_str!("../../assets/logos/nba/bos.px"),
        "mlb/nyy" => include_str!("../../assets/logos/mlb/nyy.px"),
        "mlb/tor" => include_str!("../../assets/logos/mlb/tor.px"),
        "nhl/edm" => include_str!("../../assets/logos/nhl/edm.px"),
        "nhl/dal" => include_str!("../../assets/logos/nhl/dal.px"),
        _ => return None,
    })
}

/// Cell height a pixmap needs (two pixel rows per cell).
pub fn cell_height(map: &Pixmap) -> u16 {
    (map.rows.len().div_ceil(2)) as u16
}

pub fn draw_logo(frame: &mut Frame, area: Rect, team: &Team) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(map) = load_logo(&team.logo_key) else {
        draw_abbr_mark(frame, area, team);
        return;
    };
    let primary = theme::rgb(team.color);
    let alt = theme::rgb(team.alt_color);
    let w = (map.width as u16).min(area.width);
    let h = cell_height(&map).min(area.height);
    let x0 = area.x + (area.width - w) / 2;
    let y0 = area.y + (area.height - h) / 2;
    let buf = frame.buffer_mut();
    for cy in 0..h {
        let top = &map.rows[(cy * 2) as usize];
        let bottom = map.rows.get((cy * 2 + 1) as usize);
        for cx in 0..w {
            let t = top[cx as usize];
            let b = bottom.map_or(0, |r| r[cx as usize]);
            if t == 0 && b == 0 {
                continue;
            }
            let color = |v: u8| if v == 2 { alt } else { primary };
            let cell = &mut buf[(x0 + cx, y0 + cy)];
            match (t, b) {
                (0, b) => {
                    cell.set_char('▄');
                    cell.set_fg(color(b));
                    cell.set_bg(theme::BG);
                }
                (t, 0) => {
                    cell.set_char('▀');
                    cell.set_fg(color(t));
                    cell.set_bg(theme::BG);
                }
                (t, b) if t == b => {
                    cell.set_char('█');
                    cell.set_fg(color(t));
                    cell.set_bg(theme::BG);
                }
                (t, b) => {
                    cell.set_char('▀');
                    cell.set_fg(color(t));
                    cell.set_bg(color(b));
                }
            }
        }
    }
}

/// Missing mark: team abbreviation in team color, never a hole.
fn draw_abbr_mark(frame: &mut Frame, area: Rect, team: &Team) {
    let w = (team.abbr.chars().count() as u16 + 2).min(area.width);
    let slot = Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(1) / 2,
        width: w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(format!("⟨{}⟩", team.abbr))
            .style(
                Style::default()
                    .fg(theme::rgb(team.color))
                    .add_modifier(Modifier::BOLD),
            ),
        slot,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_bundled_marks() {
        for key in [
            "nfl/kc", "nfl/tb", "nba/den", "nba/bos", "mlb/nyy", "mlb/tor", "nhl/edm", "nhl/dal",
        ] {
            let map = load_logo(key).unwrap_or_else(|| panic!("mark {key} failed to parse"));
            assert!(map.width >= 8, "{key} width {} < 8", map.width);
            assert!(map.rows.len() >= 8, "{key} rows {} < 8", map.rows.len());
        }
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(load_logo("nfl/xyz").is_none());
    }

    #[test]
    fn parse_rejects_bad_chars() {
        assert!(parse_pixmap("##\nx#").is_none());
    }

    #[test]
    fn parse_pads_ragged_rows() {
        let m = parse_pixmap("##\n#").unwrap();
        assert_eq!(m.width, 2);
        assert_eq!(m.rows[1], vec![1, 0]);
    }
}
