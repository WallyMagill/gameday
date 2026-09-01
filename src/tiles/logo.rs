use crate::domain::Team;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One cell of pregenerated logo art. `None` colors mean "terminal default":
/// no bg = transparent over the board, no fg on a space = nothing to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtCell {
    pub ch: char,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
}

pub struct AnsiArt {
    pub width: u16,
    pub cells: Vec<Vec<ArtCell>>,
}

/// Parse chafa `-f symbols` output: truecolor SGR (38;2 / 48;2), reset (0),
/// default-color resets (39/49), reverse video (7/27), cursor hide/show noise.
pub fn parse_ansi_art(raw: &str) -> Option<AnsiArt> {
    let mut rows: Vec<Vec<ArtCell>> = Vec::new();
    let mut row: Vec<ArtCell> = Vec::new();
    let mut fg: Option<(u8, u8, u8)> = None;
    let mut bg: Option<(u8, u8, u8)> = None;
    let mut reverse = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() != Some(&'[') {
                    continue;
                }
                chars.next();
                let mut seq = String::new();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        seq.push(c);
                        break;
                    }
                    seq.push(c);
                }
                let Some(final_byte) = seq.pop() else { continue };
                if final_byte != 'm' {
                    continue; // cursor hide/show etc.
                }
                let params: Vec<&str> = seq.split(';').collect();
                let mut i = 0;
                while i < params.len() {
                    match params[i] {
                        "" | "0" => {
                            fg = None;
                            bg = None;
                            reverse = false;
                        }
                        "7" => reverse = true,
                        "27" => reverse = false,
                        "39" => fg = None,
                        "49" => bg = None,
                        "38" | "48" if params.get(i + 1) == Some(&"2") && i + 4 < params.len() => {
                            let rgb = (
                                params[i + 2].parse().ok()?,
                                params[i + 3].parse().ok()?,
                                params[i + 4].parse().ok()?,
                            );
                            if params[i] == "38" {
                                fg = Some(rgb);
                            } else {
                                bg = Some(rgb);
                            }
                            i += 4;
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            '\n' => {
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            ch => {
                let (mut cfg, mut cbg) = (fg, bg);
                if reverse {
                    std::mem::swap(&mut cfg, &mut cbg);
                }
                row.push(ArtCell { ch, fg: cfg, bg: cbg });
            }
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    // Drop rows that draw nothing (chafa pads square art with blank lines).
    rows.retain(|r| r.iter().any(cell_visible));
    if rows.is_empty() {
        return None;
    }
    let width = rows.iter().map(|r| r.len()).max()? as u16;
    Some(AnsiArt { width, cells: rows })
}

fn cell_visible(c: &ArtCell) -> bool {
    if c.ch == ' ' {
        c.bg.is_some()
    } else {
        c.fg.is_some() || c.bg.is_some()
    }
}

/// Every bundled mark, key → raw chafa output. Compiled into the binary;
/// parsed at most once each (see [`load_logo`]).
const LOGO_SOURCES: &[(&str, &str)] = &[
    ("nfl/ari", include_str!("../../assets/logos/nfl/ari.ans")),
    ("nfl/atl", include_str!("../../assets/logos/nfl/atl.ans")),
    ("nfl/bal", include_str!("../../assets/logos/nfl/bal.ans")),
    ("nfl/buf", include_str!("../../assets/logos/nfl/buf.ans")),
    ("nfl/car", include_str!("../../assets/logos/nfl/car.ans")),
    ("nfl/chi", include_str!("../../assets/logos/nfl/chi.ans")),
    ("nfl/cin", include_str!("../../assets/logos/nfl/cin.ans")),
    ("nfl/cle", include_str!("../../assets/logos/nfl/cle.ans")),
    ("nfl/dal", include_str!("../../assets/logos/nfl/dal.ans")),
    ("nfl/den", include_str!("../../assets/logos/nfl/den.ans")),
    ("nfl/det", include_str!("../../assets/logos/nfl/det.ans")),
    ("nfl/gb", include_str!("../../assets/logos/nfl/gb.ans")),
    ("nfl/hou", include_str!("../../assets/logos/nfl/hou.ans")),
    ("nfl/ind", include_str!("../../assets/logos/nfl/ind.ans")),
    ("nfl/jax", include_str!("../../assets/logos/nfl/jax.ans")),
    ("nfl/kc", include_str!("../../assets/logos/nfl/kc.ans")),
    ("nfl/lv", include_str!("../../assets/logos/nfl/lv.ans")),
    ("nfl/lac", include_str!("../../assets/logos/nfl/lac.ans")),
    ("nfl/lar", include_str!("../../assets/logos/nfl/lar.ans")),
    ("nfl/mia", include_str!("../../assets/logos/nfl/mia.ans")),
    ("nfl/min", include_str!("../../assets/logos/nfl/min.ans")),
    ("nfl/ne", include_str!("../../assets/logos/nfl/ne.ans")),
    ("nfl/no", include_str!("../../assets/logos/nfl/no.ans")),
    ("nfl/nyg", include_str!("../../assets/logos/nfl/nyg.ans")),
    ("nfl/nyj", include_str!("../../assets/logos/nfl/nyj.ans")),
    ("nfl/phi", include_str!("../../assets/logos/nfl/phi.ans")),
    ("nfl/pit", include_str!("../../assets/logos/nfl/pit.ans")),
    ("nfl/sea", include_str!("../../assets/logos/nfl/sea.ans")),
    ("nfl/sf", include_str!("../../assets/logos/nfl/sf.ans")),
    ("nfl/tb", include_str!("../../assets/logos/nfl/tb.ans")),
    ("nfl/ten", include_str!("../../assets/logos/nfl/ten.ans")),
    ("nfl/wsh", include_str!("../../assets/logos/nfl/wsh.ans")),
    ("nba/den", include_str!("../../assets/logos/nba/den.ans")),
    ("nba/bos", include_str!("../../assets/logos/nba/bos.ans")),
    ("mlb/nyy", include_str!("../../assets/logos/mlb/nyy.ans")),
    ("mlb/tor", include_str!("../../assets/logos/mlb/tor.ans")),
    ("nhl/edm", include_str!("../../assets/logos/nhl/edm.ans")),
    ("nhl/dal", include_str!("../../assets/logos/nhl/dal.ans")),
];

/// The parsed marks, built on first use. `draw_logo` runs twice per tile at
/// up to 10 frames a second, and re-running the SGR parser over every one of
/// them each time was the mosaic's largest per-frame cost.
static ART: OnceLock<HashMap<&'static str, AnsiArt>> = OnceLock::new();

pub fn load_logo(key: &str) -> Option<&'static AnsiArt> {
    ART.get_or_init(|| {
        LOGO_SOURCES
            .iter()
            .filter_map(|(key, raw)| Some((*key, parse_ansi_art(raw)?)))
            .collect()
    })
    .get(key)
}

pub fn draw_logo(frame: &mut Frame, area: Rect, team: &Team) {
    let th = theme::current();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(art) = load_logo(&team.logo_key) else {
        draw_abbr_mark(frame, area, team);
        return;
    };
    let w = art.width.min(area.width);
    let h = (art.cells.len() as u16).min(area.height);
    let x0 = area.x + (area.width - w) / 2;
    let y0 = area.y + (area.height - h) / 2;
    let buf = frame.buffer_mut();
    for (y, row) in art.cells.iter().take(h as usize).enumerate() {
        for (x, art_cell) in row.iter().take(w as usize).enumerate() {
            if !cell_visible(art_cell) {
                continue;
            }
            let cell = &mut buf[(x0 + x as u16, y0 + y as u16)];
            cell.set_char(art_cell.ch);
            cell.set_fg(art_cell.fg.map_or(th.fg, |(r, g, b)| th.art_color([r, g, b])));
            cell.set_bg(art_cell.bg.map_or(th.bg, |(r, g, b)| th.art_color([r, g, b])));
        }
    }
}

/// Missing mark: team abbreviation in team color, never a hole.
fn draw_abbr_mark(frame: &mut Frame, area: Rect, team: &Team) {
    let th = theme::current();
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
                    .fg(th.team_mark_color(team.color, team.alt_color))
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
            let art = load_logo(key).unwrap_or_else(|| panic!("mark {key} failed to parse"));
            assert!(art.width >= 8, "{key} width {} < 8", art.width);
            assert!(
                (3..=8).contains(&art.cells.len()),
                "{key} rows {} outside 3..=8",
                art.cells.len()
            );
        }
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(load_logo("nfl/xyz").is_none());
    }

    #[test]
    fn parses_truecolor_and_reset() {
        let art = parse_ansi_art("\x1b[38;2;10;20;30mA\x1b[0mB\n").unwrap();
        assert_eq!(art.cells[0][0].ch, 'A');
        assert_eq!(art.cells[0][0].fg, Some((10, 20, 30)));
        assert_eq!(art.cells[0][1].fg, None);
    }

    #[test]
    fn reverse_video_swaps_colors() {
        let art = parse_ansi_art("\x1b[7m\x1b[38;2;1;2;3mX\n").unwrap();
        assert_eq!(art.cells[0][0].fg, None);
        assert_eq!(art.cells[0][0].bg, Some((1, 2, 3)));
    }

    #[test]
    fn blank_padding_rows_are_dropped() {
        let art = parse_ansi_art(" \x1b[38;2;0;0;0m  \x1b[0m\n\x1b[38;2;9;9;9m#\n").unwrap();
        assert_eq!(art.cells.len(), 1);
        assert_eq!(art.cells[0][0].ch, '#');
    }
}
