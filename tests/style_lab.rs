//! Task 11 smoke: `dump --style-lab` renders nine throwaway style variants —
//! calm-1/2/3 (color-discipline levels of the broadcast board), meter-a/b/c
//! (meter-column redesigns), ticker-a/b/c (ticker redesigns) — fully offline.
//! The variants are judged by eye from the PNGs; these tests pin that each
//! one exists, is the promised size, and is honestly distinct from its
//! siblings (not nine copies of the same frame).

use gameday::style_lab::{captures, write_pages, LabCapture};
use gameday::theme::{self, Theme};
use ratatui::buffer::Buffer;
use ratatui::style::Color;

fn capture(stem: &str) -> LabCapture {
    captures(0)
        .into_iter()
        .find(|c| c.stem == stem)
        .unwrap_or_else(|| panic!("no style-lab capture named {stem:?}"))
}

fn text_of(buf: &Buffer) -> String {
    let area = *buf.area();
    let mut text = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

/// Cells whose fg is `color` and whose symbol satisfies `pred`.
fn count_cells(buf: &Buffer, color: Color, pred: fn(char) -> bool) -> usize {
    let area = *buf.area();
    let mut n = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            if cell.fg == color && cell.symbol().chars().next().is_some_and(pred) {
                n += 1;
            }
        }
    }
    n
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}
fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}
/// Sextant / block-element glyphs: logo art and big score digits.
fn is_glyph(c: char) -> bool {
    matches!(c as u32, 0x2580..=0x259F | 0x1FB00..=0x1FBFF)
}

#[test]
fn lab_stems_and_sizes_are_the_promised_nine() {
    let caps = captures(0);
    let got: Vec<(&str, u16, u16)> = caps.iter().map(|c| (c.stem, c.cols, c.rows)).collect();
    assert_eq!(
        got,
        [
            ("calm-1", 120, 36),
            ("calm-2", 120, 36),
            ("calm-3", 120, 36),
            ("meter-a", 120, 18),
            ("meter-b", 120, 18),
            ("meter-c", 120, 18),
            ("ticker-a", 120, 6),
            ("ticker-b", 120, 6),
            ("ticker-c", 120, 6),
        ],
        "style-lab stems/sizes are the contract the dump flag promises"
    );
    for c in &caps {
        assert_eq!(
            (c.buf.area().width, c.buf.area().height),
            (c.cols, c.rows),
            "{} buffer size",
            c.stem
        );
    }
}

#[test]
fn style_lab_writes_all_nine_pages_offline() {
    let dir = std::env::temp_dir().join(format!("gameday-style-lab-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let caps = captures(0);
    write_pages(&dir, &caps).unwrap();
    for c in &caps {
        for ext in ["html", "ansi"] {
            let path = dir.join(format!("{}.{ext}", c.stem));
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            assert!(len > 0, "{} missing or empty ({len} bytes)", path.display());
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn calm_2_grays_clocks_and_shrinks_league_accents_to_the_chips() {
    let th = Theme::broadcast();
    let calm1 = capture("calm-1").buf;
    let calm2 = capture("calm-2").buf;
    // Clocks (header, sidebar, ticker) render cyan digits on calm-1; calm-2
    // grays every clock, so no cyan digit cell survives.
    assert!(count_cells(&calm1, th.cyan, is_digit) > 0, "calm-1 has cyan clock digits");
    assert_eq!(count_cells(&calm2, th.cyan, is_digit), 0, "calm-2 clocks must be gray");
    // NFL accent survives only on the [NFL] chip: fewer accent cells than
    // calm-1 (which also accents LAST PLAYS + sidebar play text), never zero.
    let accent = th.league_accent(gameday::League::Nfl);
    let a1 = count_cells(&calm1, accent, is_letter);
    let a2 = count_cells(&calm2, accent, is_letter);
    assert!(a2 > 0, "the [NFL] chip keeps its accent");
    assert!(a2 < a1, "accents must shrink to the chip: calm-1 {a1} vs calm-2 {a2}");
    // Sidebar headers collapse to one accent family (star).
    assert!(text_of(&calm2).contains("TOP PLAYS"), "sidebar still renders");
}

#[test]
fn calm_3_grays_team_colored_text_but_keeps_scores_and_logos() {
    let calm2 = capture("calm-2").buf;
    let calm3 = capture("calm-3").buf;
    let kc = theme::rgb([227, 24, 55]); // demo KC red
    assert!(
        count_cells(&calm2, kc, is_letter) > 0,
        "calm-2 still team-colors abbrs/names"
    );
    assert_eq!(
        count_cells(&calm3, kc, is_letter),
        0,
        "calm-3 grays every team-colored text cell"
    );
    assert!(
        count_cells(&calm3, kc, is_glyph) > 0,
        "score digits / logo art keep the team color"
    );
}

#[test]
fn meter_variants_are_three_different_gauges() {
    let a = text_of(&capture("meter-a").buf);
    let b = text_of(&capture("meter-b").buf);
    let c = text_of(&capture("meter-c").buf);
    // a: the current vertical right-column gauge ("┃" track, "──█──" marker).
    assert!(a.contains("RED ZONE") && a.contains("┃"), "meter-a is the current column:\n{a}");
    // b: borderless inline gauge under the identity block — a single row
    // carrying the label, the track, and the ball; no vertical track at all.
    assert!(!b.contains("┃"), "meter-b drops the vertical column:\n{b}");
    assert!(
        b.lines().any(|l| l.contains("RED ZONE") && l.contains("●") && l.contains("━")),
        "meter-b renders an inline horizontal gauge:\n{b}"
    );
    // c: framed single-glyph gauge ("┆" track), label stacked (RED over ZONE).
    assert!(
        c.lines().any(|l| l.contains("ZONE") && !l.contains("RED ZONE")),
        "meter-c stacks the label:\n{c}"
    );
    assert!(c.contains("┆") && !c.contains("┃"), "meter-c uses the single-glyph track:\n{c}");
}

#[test]
fn ticker_variants_are_boxed_pill_and_ruled() {
    let a = text_of(&capture("ticker-a").buf);
    let b = text_of(&capture("ticker-b").buf);
    let c = text_of(&capture("ticker-c").buf);
    assert!(a.contains("┌") && a.contains("TICKER"), "ticker-a keeps the current box:\n{a}");
    assert!(b.contains("▌TICKER▐"), "ticker-b uses the solid label pill:\n{b}");
    assert!(!b.contains("┌") && !b.contains("│"), "ticker-b is unboxed:\n{b}");
    assert!(!c.contains("┌") && !c.contains("│"), "ticker-c has no box:\n{c}");
    assert!(
        c.lines().any(|l| l.chars().filter(|&ch| ch == '─').count() > 100),
        "ticker-c frames with dim rules:\n{c}"
    );
}
