//! Task 11 smoke: `dump --style-lab` renders the throwaway ticker variants
//! ticker-a/b/c fully offline. (The calm-1/2/3 deck moved into the themes —
//! discipline is a per-theme property, `board-broadcast` / `board-studio` are
//! levels 1 and 3 — and meter-a/b/c is gone: variant B is the tile's real
//! meter row.) The variants are judged by eye from the PNGs; these tests pin
//! that each one exists, is the promised size, and is honestly distinct from
//! its siblings.

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
fn lab_stems_and_sizes_are_the_promised_three() {
    let caps = captures(0);
    let got: Vec<(&str, u16, u16)> = caps.iter().map(|c| (c.stem, c.cols, c.rows)).collect();
    assert_eq!(
        got,
        [("ticker-a", 120, 6), ("ticker-b", 120, 6), ("ticker-c", 120, 6)],
        "style-lab stems/sizes are the contract the dump flag promises (calm-*, meter-* deleted)"
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
fn style_lab_writes_all_three_pages_offline() {
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
