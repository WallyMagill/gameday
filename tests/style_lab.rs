//! Task 11 smoke: `dump --style-lab` renders six throwaway style variants —
//! meter-a/b/c (meter-column redesigns), ticker-a/b/c (ticker redesigns) —
//! fully offline. (The calm-1/2/3 deck moved into the themes: discipline is
//! now a per-theme property, and `board-broadcast` / `board-studio` in the
//! gallery are levels 1 and 3.) The variants are judged by eye from the PNGs;
//! these tests pin that each one exists, is the promised size, and is
//! honestly distinct from its siblings.

use gameday::style_lab::{captures, write_pages, LabCapture};
use ratatui::buffer::Buffer;

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

#[test]
fn lab_stems_and_sizes_are_the_promised_six() {
    let caps = captures(0);
    let got: Vec<(&str, u16, u16)> = caps.iter().map(|c| (c.stem, c.cols, c.rows)).collect();
    assert_eq!(
        got,
        [
            ("meter-a", 120, 18),
            ("meter-b", 120, 18),
            ("meter-c", 120, 18),
            ("ticker-a", 120, 6),
            ("ticker-b", 120, 6),
            ("ticker-c", 120, 6),
        ],
        "style-lab stems/sizes are the contract the dump flag promises (calm-* deleted)"
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
fn style_lab_writes_all_six_pages_offline() {
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
