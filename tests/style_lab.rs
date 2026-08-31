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

/// Every cell's (fg, bg) compared between two same-size buffers.
fn styled_cells_differing(a: &Buffer, b: &Buffer) -> usize {
    let area = *a.area();
    let mut n = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if a[(x, y)].fg != b[(x, y)].fg || a[(x, y)].bg != b[(x, y)].bg {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn calm_3_leaves_only_scores_logos_and_live_colored() {
    // Level 3's rule, checked cell by cell: apart from logo/score glyphs and
    // the league chips, no text cell carries any color but the gray family
    // or the earned live red — no amber chrome, no cyan clocks, no team
    // colors on names/abbrs (the RECORDS rail included), no green date.
    let th = Theme::broadcast();
    let calm3 = capture("calm-3").buf;
    let allowed = [th.bg, th.fg, th.bright, th.muted, th.dim, th.border, th.live];
    let accents: Vec<Color> = gameday::League::ALL.iter().map(|l| th.league_accent(*l)).collect();
    let area = *calm3.area();
    let mut offenders = Vec::new();
    for y in 0..area.height {
        let row: Vec<char> = (0..area.width)
            .map(|x| calm3[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect();
        for x in 0..area.width {
            let cell = &calm3[(x, y)];
            let ch = row[x as usize];
            if ch == ' ' || is_glyph(ch) || allowed.contains(&cell.fg) {
                continue;
            }
            // A league accent survives only inside a [CHIP] (5 cells wide).
            if accents.contains(&cell.fg) {
                let x = x as usize;
                let open = row[..=x].iter().rposition(|&c| c == '[');
                let close = row[x..].iter().position(|&c| c == ']');
                if open.is_some_and(|o| x - o <= 5) && close.is_some_and(|c| c <= 5) {
                    continue;
                }
            }
            let line: String = row.iter().collect();
            offenders.push(format!("({x},{y}) {ch:?} fg={:?} in {:?}", cell.fg, line.trim_end()));
        }
    }
    assert!(offenders.is_empty(), "colored text left on calm-3:\n{}", offenders.join("\n"));
    // Chrome that was amber-on-black or black-on-amber is now white/black.
    let star_bg = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| calm3[(x, y)].bg == th.star)
        .count();
    assert_eq!(star_bg, 0, "amber chip backgrounds must go with the amber chrome");
    // Scores and logos stay in team colors — the board is not monochrome.
    assert!(count_cells(&calm3, theme::rgb([227, 24, 55]), is_glyph) > 0);
}

#[test]
fn calm_levels_are_visibly_distinct_steps() {
    // Review finding: the first cut of the deck changed ~240 styled cells
    // per step — small chrome text only, so 2→3 was invisible at a glance.
    // Measured after the rework (tick 0, 120x36 = 4320 cells): 1→2 = 255,
    // 2→3 = 388 (level 3 now also strips the amber chrome, chip backgrounds
    // and the RECORDS rail's team colors). Floors sit under those so a
    // regression back to near-identical boards fails here.
    let c1 = capture("calm-1").buf;
    let c2 = capture("calm-2").buf;
    let c3 = capture("calm-3").buf;
    let d12 = styled_cells_differing(&c1, &c2);
    let d23 = styled_cells_differing(&c2, &c3);
    assert!(d12 >= 200, "calm-1 → calm-2 changed only {d12} cells (measured 255)");
    assert!(d23 >= 300, "calm-2 → calm-3 changed only {d23} cells (measured 388)");
    // And the text is identical: the levels are color discipline, not layout.
    assert_eq!(text_of(&c1), text_of(&c2));
    assert_eq!(text_of(&c2), text_of(&c3));
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
