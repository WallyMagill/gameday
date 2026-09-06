//! Mid-size score digits drawn from the quadrant blocks (U+2580–259F).
//!
//! Why a second digit renderer exists at all: a coverage spike found that
//! Terminal.app has no sextant coverage — `tui-big-text`'s `PixelSize::Sextant`
//! (the hero's mid rung before this one, 4×3) renders as tofu
//! there — and that at 80×24 the sextant digits do not resolve into a readable
//! number even where the font *does* cover them. The quadrant block set is the
//! oldest, widest-covered half/quarter-cell run in Unicode; a digit built from
//! it draws on every terminal that draws `█`.
//!
//! The form: each digit is a 6×8 pixel grid folded 2×2 into 3 columns × 4
//! rows of quadrant cells. That is one row TALLER than the sextant rung it
//! replaces and one column narrower per digit, so a two-digit score is 7×4
//! instead of 8×3 — the same footprint, spent on height, which is where a
//! digit's identity lives.
//!
//! This is the ladder's mid rung: `hero::score_block`
//! draws it wherever a bracket asked for digits but cannot hold the 8-row
//! `PixelSize::Full` form, which is every terminal from 60 to 99 columns.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Frame;

/// Cells one digit occupies: 3 wide, 4 tall. Set by the glyph table below —
/// a 6×8 pixel design folded 2×2. Not a tunable: [`GLYPHS`] is drawn to it.
pub const QUAD_COLS: u16 = 3;
pub const QUAD_ROWS: u16 = 4;
/// One blank column between digits. Zero ran `4` and `1` into a single shape
/// (the `4`'s open right side meets the `1`'s stem); one column separates
/// every pair in the table and two wastes a third of a digit's width.
pub const QUAD_GAP: u16 = 1;

/// 0–9, four rows of three quadrant cells each, folded from a 6×8 pixel grid.
///
/// The pixel design is a fat seven-segment: 2-px vertical stems at pixel
/// columns 0–1 and 4–5, 1-px horizontal bars. Bars land on pixel rows 0
/// (top), 3 (waist) and 7 (foot), so the waist folds to the *bottom* half of
/// cell row 1 (`▄`) and the foot to the bottom half of cell row 3 — a waist
/// above center, which is what keeps `8` from reading as `0` at a squint.
///
/// The danger pairs, and what separates them here:
/// - `0` vs `8`: `8`'s waist is 2 px thick, so it shows in *two* cell rows
///   (`█▄█` then `█▀█`) — two stacked bowls where `0` has one open well. A
///   1-px waist put the pair one cell apart, which is one squint from wrong.
/// - `6` vs `8`: `6`'s top-right is open (`█▀▀`/`█▄▄`), `8`'s is closed.
/// - `9` vs `8`: `9` drops the lower-left stem (`  █` vs `█ █`).
/// - `1` vs `7`: `1` is a centered stem with a flag and a full foot serif,
///   `7` is a top bar over a right-hand stem — they share no cell.
/// - `3` vs `5`: `3` has no upper-left stem at all; `5`'s row 0 is `█▀▀`.
const GLYPHS: [[&str; 4]; 10] = [
    // 0
    ["█▀█", "█ █", "█ █", "█▄█"],
    // 1
    ["▝█ ", " █ ", " █ ", "▄█▄"],
    // 2
    ["▀▀█", "▄▄█", "█  ", "█▄▄"],
    // 3
    ["▀▀█", " ▄█", "  █", "▄▄█"],
    // 4
    ["█ █", "█ █", "▀▀█", "  █"],
    // 5
    ["█▀▀", "█▄▄", "  █", "▄▄█"],
    // 6
    ["█▀▀", "█▄▄", "█ █", "█▄█"],
    // 7
    ["▀▀█", "  █", "  █", "  █"],
    // 8
    ["█▀█", "█▄█", "█▀█", "█▄█"],
    // 9
    ["█▀█", "█▄█", "  █", "▄▄█"],
];

/// Cells `value` needs at this size: `n` digits plus the gaps between them.
pub fn quad_size(value: u32) -> (u16, u16) {
    let n = value.to_string().len() as u16;
    (n * QUAD_COLS + n.saturating_sub(1) * QUAD_GAP, QUAD_ROWS)
}

/// Draw `value` as quadrant-block digits at the top-left of `rect`, in
/// `color`. Returns false — drawing *nothing* — when the digits don't fit,
/// the same contract [`super::digit_glyphs`] has: every caller steps down a
/// size rather than clip a digit in half. Blank cells inside a glyph are left
/// untouched, so the form composites over whatever is already there.
pub fn quad_digits(frame: &mut Frame, rect: Rect, value: u32, color: Color) -> bool {
    let (w, h) = quad_size(value);
    if w > rect.width || h > rect.height {
        return false;
    }
    let style = Style::default().fg(color);
    let buf = frame.buffer_mut();
    for (i, digit) in value
        .to_string()
        .bytes()
        .map(|b| usize::from(b - b'0'))
        .enumerate()
    {
        let x0 = rect.x + i as u16 * (QUAD_COLS + QUAD_GAP);
        for (dy, row) in GLYPHS[digit].iter().enumerate() {
            for (dx, ch) in row.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let pos = (x0 + dx as u16, rect.y + dy as u16);
                if let Some(cell) = buf.cell_mut(pos) {
                    cell.set_char(ch).set_style(style);
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn render(w: u16, h: u16, rect: Rect, value: u32, color: Color) -> (Buffer, bool) {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut fit = false;
        term.draw(|f| fit = quad_digits(f, rect, value, color))
            .unwrap();
        (term.backend().buffer().clone(), fit)
    }

    fn rows_of(buf: &Buffer) -> Vec<String> {
        let area = *buf.area();
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn quad_digits_draw_readable_numerals_in_four_rows() {
        let rect = Rect {
            x: 1,
            y: 1,
            width: 8,
            height: 5,
        };
        let (buf, fit) = render(12, 8, rect, 87, Color::Red);
        assert!(fit, "87 needs 7x4, rect is {}x{}", rect.width, rect.height);
        let rows = rows_of(&buf);
        // Exactly four rows carry ink, and they are the rect's four.
        let inked: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.trim() != "")
            .map(|(y, _)| y)
            .collect();
        assert_eq!(
            inked,
            vec![1, 2, 3, 4],
            "glyph rows, got:\n{}",
            rows.join("\n")
        );
        // Cell-level: the exact table for 8 then a gap then 7.
        assert_eq!(
            &rows[1][..],
            " █▀█ ▀▀█    ",
            "row 0 of `87`, got:\n{}",
            rows.join("\n")
        );
        assert_eq!(&rows[2][..], " █▄█   █    ");
        assert_eq!(&rows[3][..], " █▀█   █    ");
        assert_eq!(&rows[4][..], " █▄█   █    ");
        // The per-digit column mask differs between 8 and 7: 8 fills its
        // left stem on every row, 7 only its top bar.
        let (eight, seven) = ((1..=3u16), (5..=7u16));
        let ink_in = |cols: std::ops::RangeInclusive<u16>| -> usize {
            cols.flat_map(|x| (1..=4u16).map(move |y| (x, y)))
                .filter(|p| buf[*p].symbol() != " ")
                .count()
        };
        assert_ne!(
            ink_in(eight.clone()),
            ink_in(seven.clone()),
            "8 and 7 must not share a mask"
        );
        assert_eq!(ink_in(eight), 12, "8 fills all 12 of its cells");
        assert_eq!(ink_in(seven), 6, "7 fills 6 of its 12 cells");
        // fg is the passed color on every glyph cell, and nowhere else.
        for y in 1..=4u16 {
            for x in 1..=8u16 {
                let cell = &buf[(x, y)];
                if cell.symbol() != " " {
                    assert_eq!(
                        cell.fg,
                        Color::Red,
                        "glyph cell ({x},{y}) wears the passed color"
                    );
                }
            }
        }
    }

    #[test]
    fn quad_digits_refuse_too_small_and_report_it() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let (buf, fit) = render(12, 8, rect, 87, Color::Red);
        assert!(!fit, "a 3-row rect cannot hold a {QUAD_ROWS}-row digit");
        let rows = rows_of(&buf);
        assert!(
            rows.iter().all(|r| r.trim().is_empty()),
            "a refused draw leaves the buffer untouched, got:\n{}",
            rows.join("\n")
        );
        // Narrow refuses too: 87 wants 7 columns.
        let (buf, fit) = render(
            12,
            8,
            Rect {
                x: 0,
                y: 0,
                width: 6,
                height: 4,
            },
            87,
            Color::Red,
        );
        assert!(!fit, "87 needs {} columns, rect gave 6", quad_size(87).0);
        assert!(rows_of(&buf).iter().all(|r| r.trim().is_empty()));
    }

    #[test]
    fn every_digit_is_distinct_and_the_danger_pairs_differ_in_two_rows() {
        // A glyph table's whole job is that no two digits render the same.
        for (a, ga) in GLYPHS.iter().enumerate() {
            for (b, gb) in GLYPHS.iter().enumerate().skip(a + 1) {
                assert_ne!(ga, gb, "digits {a} and {b} render identically");
                // The pairs a reader actually confuses need more than one
                // row of difference — one differing row is one squint away.
                let differing = (0..4).filter(|r| ga[*r] != gb[*r]).count();
                if [
                    (0, 8),
                    (6, 8),
                    (8, 9),
                    (0, 6),
                    (0, 9),
                    (1, 7),
                    (3, 5),
                    (5, 6),
                    (3, 9),
                ]
                .contains(&(a, b))
                {
                    assert!(
                        differing >= 2,
                        "danger pair {a}/{b} differs in only {differing} of 4 rows"
                    );
                }
            }
        }
    }

    #[test]
    fn the_table_is_only_quadrant_blocks_and_is_rectangular() {
        for (d, glyph) in GLYPHS.iter().enumerate() {
            for (r, row) in glyph.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    QUAD_COLS as usize,
                    "digit {d} row {r} is not {QUAD_COLS} cells: {row:?}"
                );
                for ch in row.chars() {
                    assert!(
                        ch == ' ' || ('\u{2580}'..='\u{259F}').contains(&ch),
                        "digit {d} row {r} uses {ch:?} (U+{:04X}), outside U+2580–259F",
                        ch as u32
                    );
                }
            }
        }
    }
}
