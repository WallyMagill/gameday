//! Smoke for `dump --style-lab`: the three NEW ticker directions (d BottomLine
//! two-lane, e LED ribbon, f split-flap) render fully offline, each on the
//! broadcast theme and once on gruvbox, and f gets a mid-flip frame driven by
//! the sim. The directions are judged by eye from the PNGs; these tests pin
//! that each exists at the promised size/theme, is honestly distinct from its
//! siblings, and that the flip is a real animation (old ≠ mid ≠ settled).

use gameday::domain::League;
use gameday::sim::KC_TD_TICK;
use gameday::style_lab::{captures, ticker_d, ticker_e, ticker_f, write_pages, LabCapture, FLIP_TICKS};
use gameday::theme;
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

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area().width).map(|x| buf[(x, y)].symbol().to_string()).collect()
}

/// Rows that carry at least one letter or digit.
fn text_rows(buf: &Buffer) -> Vec<u16> {
    (0..buf.area().height)
        .filter(|&y| row(buf, y).chars().any(|c| c.is_ascii_alphanumeric()))
        .collect()
}

fn count_cells(buf: &Buffer, y: u16, pred: impl Fn(&ratatui::buffer::Cell) -> bool) -> usize {
    (0..buf.area().width).filter(|&x| pred(&buf[(x, y)])).count()
}

#[test]
fn lab_stems_sizes_and_themes_are_the_promised_set() {
    let caps = captures(0);
    let got: Vec<(&str, u16, u16, &str)> =
        caps.iter().map(|c| (c.stem, c.cols, c.rows, c.theme)).collect();
    assert_eq!(
        got,
        [
            ("ticker-d", 120, 6, "broadcast"),
            ("ticker-e", 120, 6, "broadcast"),
            ("ticker-f", 120, 6, "broadcast"),
            ("ticker-f-flip", 120, 6, "broadcast"),
            ("ticker-d-gruvbox", 120, 6, "gruvbox"),
            ("ticker-e-gruvbox", 120, 6, "gruvbox"),
            ("ticker-f-gruvbox", 120, 6, "gruvbox"),
        ],
        "style-lab stems/sizes/themes are the contract the dump flag promises (a/b/c deleted)"
    );
    for c in &caps {
        assert_eq!(
            (c.buf.area().width, c.buf.area().height),
            (c.cols, c.rows),
            "{} buffer size",
            c.stem
        );
        assert_eq!(c.buf[(0, 0)].bg, theme::builtin(c.theme).bg, "{} page ground", c.stem);
    }
    assert_eq!(theme::current_name(), "broadcast", "the lab must restore the thread theme");
}

#[test]
fn style_lab_writes_every_page_offline() {
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
    // The gruvbox pages are serialized under gruvbox (page background).
    let html = std::fs::read_to_string(dir.join("ticker-d-gruvbox.html")).unwrap();
    assert!(html.contains("background:#282828"), "gruvbox page ground missing:\n{html}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ticker_d_is_two_lanes_under_a_thin_rule_with_no_box() {
    let buf = capture("ticker-d").buf;
    let text = text_of(&buf);
    assert!(!text.contains('┌') && !text.contains('└'), "d has no box:\n{text}");
    let rows = text_rows(&buf);
    assert_eq!(rows.len(), 2, "d is exactly two lanes:\n{text}");
    let rule_y = rows[0] - 1;
    assert!(
        row(&buf, rule_y).chars().filter(|&c| c == '─').count() >= 118,
        "a thin rule sits directly above lane 1:\n{text}"
    );
    let lane1 = row(&buf, rows[0]);
    let lane2 = row(&buf, rows[1]);
    // Lane 1: every game in play as a compact score, league-tagged, pipe-separated.
    for needle in ["NFL", "KC 27 TB 24", "Q4 1:27", "NBA", "DEN 88 BOS 81", "MLB", "NHL", " │ "] {
        assert!(lane1.contains(needle), "lane 1 missing {needle:?}:\n{lane1}");
    }
    assert!(!lane1.contains("TOUCHDOWN"), "lane 1 is scores only:\n{lane1}");
    // Lane 2: scoring alerts.
    assert!(lane2.contains("TOUCHDOWN!"), "lane 2 carries the alerts:\n{lane2}");
    assert!(!lane2.contains("Q4 1:27"), "lane 2 is alerts only:\n{lane2}");
}

#[test]
fn ticker_d_scores_lane_shows_whole_games_and_rotates_the_rest_in() {
    // ticker-d.png ended "… │ EPL" with its score off the edge: a dangling
    // league tag reads as a missing game. Lane 1 is whole segments only, and
    // the games that don't fit rotate in over time instead of crawling.
    let slugs: Vec<String> = League::ALL.iter().map(|l| l.slug().to_uppercase()).collect();
    let mut seen_first: std::collections::BTreeSet<String> = Default::default();
    for tick in (0..300).step_by(30) {
        let buf = ticker_d(tick);
        let y = text_rows(&buf)[0];
        let lane1 = row(&buf, y);
        let trimmed = lane1.trim_end();
        for s in &slugs {
            assert!(!trimmed.ends_with(s.as_str()), "tick {tick}: lane 1 ends on a bare {s} chip: {lane1:?}");
            assert!(!trimmed.ends_with(&format!("{s} ")), "tick {tick}: {lane1:?}");
        }
        assert!(!trimmed.ends_with('│'), "tick {tick}: no trailing separator: {lane1:?}");
        // Every visible segment is complete: a league tag is always followed
        // by two abbr/score pairs and a clock before the next separator/end.
        for seg in trimmed.trim_start_matches(" SCORES ").split(" │ ") {
            let words: Vec<&str> = seg.split_whitespace().collect();
            assert!(words.len() >= 6, "tick {tick}: partial segment {seg:?} in {lane1:?}");
            assert!(slugs.iter().any(|s| s == words[0]), "tick {tick}: segment {seg:?} starts with a league tag");
        }
        let first = trimmed.trim_start_matches(" SCORES ").split_whitespace().next().unwrap().to_string();
        seen_first.insert(first);
    }
    assert!(seen_first.len() > 1, "the lane must rotate when not every game fits: only saw {seen_first:?}");
}

#[test]
fn ticker_e_is_one_band_of_team_and_league_chips() {
    let th = theme::builtin("broadcast");
    let buf = capture("ticker-e").buf;
    let text = text_of(&buf);
    let rows = text_rows(&buf);
    assert_eq!(rows.len(), 1, "e is a single row:\n{text}");
    let y = rows[0];
    assert!(!text.contains('│') && !text.contains('┌'), "e has no box or pipes:\n{text}");
    // The band: the row is painted in `dim`, not the page ground.
    assert!(
        count_cells(&buf, y, |c| c.bg == th.dim) > 60,
        "the ribbon row sits on a dim band:\n{text}"
    );
    // Team chip: KC on a Chiefs-red block; league chip: NFL on the NFL accent.
    let kc = Color::Rgb(227, 24, 55);
    assert!(
        count_cells(&buf, y, |c| c.bg == kc && (c.symbol() == "K" || c.symbol() == "C")) == 2,
        "KC abbr must sit on a team-colored block:\n{text}"
    );
    assert!(
        count_cells(&buf, y, |c| c.bg == th.chip(League::Nfl) && c.symbol() == "N") >= 1,
        "the NFL league chip must render as a block:\n{text}"
    );
    assert!(row(&buf, y).contains("TOUCHDOWN!"), "scoring word missing:\n{text}");
}

#[test]
fn ticker_f_is_four_fixed_width_cells_per_row() {
    let buf = capture("ticker-f").buf;
    let text = text_of(&buf);
    let rows = text_rows(&buf);
    assert_eq!(rows.len(), 2, "f is a two-row board of cells:\n{text}");
    for &y in &rows {
        let r = row(&buf, y);
        let pipes: Vec<usize> = r.chars().enumerate().filter(|(_, c)| *c == '│').map(|(i, _)| i).collect();
        assert_eq!(pipes, [0, 30, 60, 90], "cells are 30 wide, four per row:\n{r}");
    }
    let top = row(&buf, rows[0]);
    for needle in ["3:21", "KC", "TD", "27-24"] {
        assert!(top.contains(needle), "newest cell missing {needle:?}:\n{top}");
    }
    assert!(!text.contains('┌') && !text.contains('─'), "f has no frame rules:\n{text}");
}

#[test]
fn ticker_f_flip_is_a_real_animation_between_old_and_settled() {
    let th = theme::builtin("broadcast");
    let before = ticker_f(KC_TD_TICK - 1);
    let mid = ticker_f(KC_TD_TICK);
    let after = ticker_f(KC_TD_TICK + FLIP_TICKS);
    // The board as a list of 30-wide cells (both rows).
    let cells = |buf: &Buffer| -> Vec<String> {
        text_rows(buf)
            .iter()
            .flat_map(|&y| {
                let r = row(buf, y);
                (0..4).map(move |i| r.chars().skip(i * 30).take(30).collect::<String>())
            })
            .collect()
    };
    let (b, m, a) = (cells(&before), cells(&mid), cells(&after));
    assert_eq!(b.len(), 8, "eight cells");
    // Settled: the KC TD took one module, with the score it produced.
    let landed = a
        .iter()
        .position(|c| ["1:12", "KC", "TD", "33-24"].iter().all(|n| c.contains(n)))
        .unwrap_or_else(|| panic!("settled board has no KC TD cell:\n{a:#?}"));
    // One event landing flips ONE module; every other cell holds still in
    // both the mid-flip and the settled frame (ticker-f-flip.png had all
    // eight scrambled at once and read as corruption).
    let changed: Vec<usize> = (0..8).filter(|&i| b[i] != a[i]).collect();
    assert_eq!(changed, vec![landed], "exactly the landing cell changes between before and settled:\n{b:#?}\n{a:#?}");
    let mid_changed: Vec<usize> = (0..8).filter(|&i| b[i] != m[i]).collect();
    assert_eq!(mid_changed, vec![landed], "mid-flip touches only the landing cell:\n{m:#?}");
    // Mid-flip: not the old cell, not the new one, and some glyphs are
    // still rolling (drawn muted).
    assert_ne!(m[landed], b[landed], "mid-flip must differ from the pre-event cell");
    assert_ne!(m[landed], a[landed], "mid-flip must differ from the settled cell");
    let y = text_rows(&mid)[landed / 4];
    let rolling_mid = count_cells(&mid, y, |c| c.fg == th.muted && c.symbol() != " ");
    let rolling_after = count_cells(&after, y, |c| c.fg == th.muted && c.symbol() != " ");
    assert!(rolling_mid > rolling_after, "mid-flip shows rolling glyphs ({rolling_mid} vs {rolling_after}):\n{}", m[landed]);
    // Every rolling glyph is a single-width printable — terminal-legal.
    for ch in m[landed].chars() {
        assert!(!ch.is_control() && ch != '\t', "illegal glyph {ch:?} in flip cell:\n{}", m[landed]);
    }
    // The capture the dump writes is that mid-flip frame, whatever --tick was.
    assert_eq!(text_of(&capture("ticker-f-flip").buf), text_of(&mid));
    // Pure: same tick, same frame.
    assert_eq!(text_of(&ticker_f(KC_TD_TICK)), text_of(&mid));
}

#[test]
fn directions_are_honestly_distinct_on_both_themes() {
    for th in ["broadcast", "gruvbox"] {
        theme::set_current(th).unwrap();
        let d = text_of(&ticker_d(0));
        let e = text_of(&ticker_e(0));
        let f = text_of(&ticker_f(0));
        assert_ne!(d, e);
        assert_ne!(e, f);
        assert_ne!(d, f);
        theme::set_current("broadcast").unwrap();
    }
    let g = capture("ticker-e-gruvbox").buf;
    let y = text_rows(&g)[0];
    let gruv = theme::builtin("gruvbox");
    assert!(count_cells(&g, y, |c| c.bg == gruv.dim) > 60, "gruvbox band uses the gruvbox dim");
}
