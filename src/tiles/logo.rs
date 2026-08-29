use crate::domain::Team;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub type Glyph = [[char; 8]; 5];

pub fn parse_logo(raw: &str) -> Option<Glyph> {
    let mut lines: Vec<&str> = raw.lines().collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.len() != 5 {
        return None;
    }
    let mut glyph = [['.'; 8]; 5];
    for (r, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() != 8 || !chars.iter().all(|c| matches!(c, '#' | '+' | '.')) {
            return None;
        }
        for (c, ch) in chars.into_iter().enumerate() {
            glyph[r][c] = ch;
        }
    }
    Some(glyph)
}

pub fn load_logo(key: &str) -> Option<Glyph> {
    parse_logo(match key {
        "nfl/ari" => include_str!("../../assets/logos/nfl/ari.logo"),
        "nfl/atl" => include_str!("../../assets/logos/nfl/atl.logo"),
        "nfl/bal" => include_str!("../../assets/logos/nfl/bal.logo"),
        "nfl/buf" => include_str!("../../assets/logos/nfl/buf.logo"),
        "nfl/car" => include_str!("../../assets/logos/nfl/car.logo"),
        "nfl/chi" => include_str!("../../assets/logos/nfl/chi.logo"),
        "nfl/cin" => include_str!("../../assets/logos/nfl/cin.logo"),
        "nfl/cle" => include_str!("../../assets/logos/nfl/cle.logo"),
        "nfl/dal" => include_str!("../../assets/logos/nfl/dal.logo"),
        "nfl/den" => include_str!("../../assets/logos/nfl/den.logo"),
        "nfl/det" => include_str!("../../assets/logos/nfl/det.logo"),
        "nfl/gb" => include_str!("../../assets/logos/nfl/gb.logo"),
        "nfl/hou" => include_str!("../../assets/logos/nfl/hou.logo"),
        "nfl/ind" => include_str!("../../assets/logos/nfl/ind.logo"),
        "nfl/jax" => include_str!("../../assets/logos/nfl/jax.logo"),
        "nfl/kc" => include_str!("../../assets/logos/nfl/kc.logo"),
        "nfl/lv" => include_str!("../../assets/logos/nfl/lv.logo"),
        "nfl/lac" => include_str!("../../assets/logos/nfl/lac.logo"),
        "nfl/lar" => include_str!("../../assets/logos/nfl/lar.logo"),
        "nfl/mia" => include_str!("../../assets/logos/nfl/mia.logo"),
        "nfl/min" => include_str!("../../assets/logos/nfl/min.logo"),
        "nfl/ne" => include_str!("../../assets/logos/nfl/ne.logo"),
        "nfl/no" => include_str!("../../assets/logos/nfl/no.logo"),
        "nfl/nyg" => include_str!("../../assets/logos/nfl/nyg.logo"),
        "nfl/nyj" => include_str!("../../assets/logos/nfl/nyj.logo"),
        "nfl/phi" => include_str!("../../assets/logos/nfl/phi.logo"),
        "nfl/pit" => include_str!("../../assets/logos/nfl/pit.logo"),
        "nfl/sea" => include_str!("../../assets/logos/nfl/sea.logo"),
        "nfl/sf" => include_str!("../../assets/logos/nfl/sf.logo"),
        "nfl/tb" => include_str!("../../assets/logos/nfl/tb.logo"),
        "nfl/ten" => include_str!("../../assets/logos/nfl/ten.logo"),
        "nfl/wsh" => include_str!("../../assets/logos/nfl/wsh.logo"),
        _ => return None,
    })
}

fn glyph(c: char) -> [&'static str; 5] {
    match c.to_ascii_uppercase() {
        'A' => [".#.", "#.#", "###", "#.#", "#.#"],
        'B' => ["##.", "#.#", "##.", "#.#", "##."],
        'C' => [".##", "#..", "#..", "#..", ".##"],
        'D' => ["##.", "#.#", "#.#", "#.#", "##."],
        'E' => ["###", "#..", "##.", "#..", "###"],
        'F' => ["###", "#..", "##.", "#..", "#.."],
        'G' => [".##", "#..", "#.#", "#.#", ".##"],
        'H' => ["#.#", "#.#", "###", "#.#", "#.#"],
        'I' => ["###", ".#.", ".#.", ".#.", "###"],
        'J' => ["###", "..#", "..#", "#.#", ".#."],
        'K' => ["#.#", "#.#", "##.", "#.#", "#.#"],
        'L' => ["#..", "#..", "#..", "#..", "###"],
        'M' => ["#.#", "###", "###", "#.#", "#.#"],
        'N' => ["#.#", "###", "###", "###", "#.#"],
        'O' => [".#.", "#.#", "#.#", "#.#", ".#."],
        'P' => ["##.", "#.#", "##.", "#..", "#.."],
        'Q' => [".#.", "#.#", "#.#", ".##", "..#"],
        'R' => ["##.", "#.#", "##.", "#.#", "#.#"],
        'S' => [".##", "#..", ".#.", "..#", "##."],
        'T' => ["###", ".#.", ".#.", ".#.", ".#."],
        'U' => ["#.#", "#.#", "#.#", "#.#", "###"],
        'V' => ["#.#", "#.#", "#.#", "#.#", ".#."],
        'W' => ["#.#", "#.#", "###", "###", "#.#"],
        'X' => ["#.#", "#.#", ".#.", "#.#", "#.#"],
        'Y' => ["#.#", "#.#", ".#.", ".#.", ".#."],
        'Z' => ["###", "..#", ".#.", "#..", "###"],
        _ => ["...", "...", ".#.", "...", "..."],
    }
}

pub fn letterform(abbr: &str) -> String {
    let chars: Vec<char> = abbr.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let (a, b) = match chars.len() {
        0 => ('?', '?'),
        1 => (chars[0], ' '),
        2 => (chars[0], chars[1]),
        _ => (chars[0], chars[chars.len() - 1]),
    };
    let left = glyph(a);
    let right = glyph(b);
    (0..5)
        .map(|r| format!(".{}.{}", left[r], right[r])) // pad + 3 + gap + 3 = 8
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub fn draw_logo(frame: &mut Frame, area: Rect, team: &Team) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if let Some(mark) = load_logo(&team.logo_key) {
        let buf = frame.buffer_mut();
        let primary = theme::rgb(team.color);
        let alt = theme::rgb(team.alt_color);
        for (r, row) in mark.iter().enumerate() {
            let y = area.y + r as u16;
            if y >= area.y + area.height {
                break;
            }
            for (c, ch) in row.iter().enumerate() {
                let x = area.x + c as u16;
                if x >= area.x + area.width {
                    break;
                }
                let fg = match ch {
                    '#' => Some(primary),
                    '+' => Some(alt),
                    _ => None,
                };
                if let Some(fg) = fg {
                    let cell = &mut buf[(x, y)];
                    cell.set_char(*ch);
                    cell.set_fg(fg);
                    cell.set_bg(theme::BG);
                }
            }
        }
    } else {
        let w = team.abbr.chars().count() as u16;
        let slot = Rect {
            x: area.x + area.width.saturating_sub(w) / 2,
            y: area.y + area.height.saturating_sub(1) / 2,
            width: w.min(area.width),
            height: area.height.min(1),
        };
        frame.render_widget(
            Paragraph::new(team.abbr.as_str()).style(Style::default().fg(theme::rgb(team.color))),
            slot,
        );
    }
}
