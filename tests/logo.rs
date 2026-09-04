use gameday::board::logo::{draw_abbr_mark, draw_hero_mark, hero_mark, HeroMark};
use gameday::domain::Team;
use ratatui::{backend::TestBackend, Terminal};

fn team(abbr: &str, logo_key: &str) -> Team {
    Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color: [227, 24, 55],
        alt_color: [255, 184, 28],
        logo_key: logo_key.into(),
        ..Default::default()
    }
}

fn draw_to_text(team: &Team, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| match hero_mark(&team.logo_key) {
        Some(mark) => draw_hero_mark(f, f.area(), mark),
        None => draw_abbr_mark(f, f.area(), team),
    })
    .unwrap();
    let b = t.backend().buffer();
    let mut s = String::new();
    for y in 0..h {
        for x in 0..w {
            s.push_str(b[(x, y)].symbol());
        }
    }
    s
}

#[test]
fn every_committed_mark_parses_at_hero_size() {
    // The build embeds the same set logo.rs lists; spot-check the demo set.
    for key in ["nfl/kc", "nfl/buf", "nfl/dal", "mlb/nyy", "nba/bos", "nhl/edm"] {
        let m: &HeroMark = hero_mark(key).unwrap_or_else(|| panic!("{key} missing"));
        assert!(m.height >= 6 && m.height <= 10, "{key}: 16x10 regeneration, got {}", m.height);
        assert!(m.width >= 10 && m.width <= 16, "{key}: width {}", m.width);
    }
    // v3.4 §7 completed the pro leagues, so "a team with no art" is no longer
    // a pro team — it's an unranked school.
    assert!(hero_mark("ncaa/999999").is_none(), "missing art is None, the caller falls back");
}

#[test]
fn no_mark_exceeds_the_hero_slot() {
    for key in gameday::board::logo::committed_keys() {
        let m = hero_mark(key).unwrap_or_else(|| panic!("{key} missing"));
        assert!(m.width <= 16, "{key} wider than the 16x10 hero slot: {}", m.width);
        assert!(m.height <= 10, "{key} taller than the 16x10 hero slot: {}", m.height);
    }
}

#[test]
fn hero_mark_draws_into_a_clipped_area() {
    // The tile grammar still hands 8x5 boxes to marks that are now up to
    // 16x10; the blit must clip rather than panic or bleed.
    let mut t = Terminal::new(TestBackend::new(8, 5)).unwrap();
    let mark = hero_mark("nfl/kc").unwrap();
    t.draw(|f| draw_hero_mark(f, f.area(), mark)).unwrap();
    let b = t.backend().buffer();
    let painted = (0..5)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .filter(|&(x, y)| b[(x, y)].symbol() != " ")
        .count();
    assert!(painted > 0, "clipped blit painted nothing");
}

#[test]
fn missing_logo_draws_abbr() {
    let s = draw_to_text(&team("KC", "nfl/does-not-exist"), 10, 6);
    assert!(s.contains("KC"), "{s}");
}

#[test]
fn bundled_mark_paints_cells() {
    let s = draw_to_text(&team("KC", "nfl/kc"), 16, 10);
    let non_blank = s.chars().filter(|c| *c != ' ').count();
    assert!(non_blank >= 20, "mark painted only {non_blank} cells: {s:?}");
}

#[test]
fn demo_marks_all_load() {
    for key in [
        "nfl/kc", "nfl/tb", "nba/den", "nba/bos", "mlb/nyy", "mlb/tor", "nhl/edm", "nhl/dal",
    ] {
        assert!(hero_mark(key).is_some(), "missing {key}");
    }
}

#[test]
fn all_thirty_two_nfl_marks_load() {
    for abbr in [
        "ari", "atl", "bal", "buf", "car", "chi", "cin", "cle", "dal", "den", "det", "gb", "hou",
        "ind", "jax", "kc", "lv", "lac", "lar", "mia", "min", "ne", "no", "nyg", "nyj", "phi",
        "pit", "sea", "sf", "tb", "ten", "wsh",
    ] {
        assert!(hero_mark(&format!("nfl/{abbr}")).is_some(), "missing nfl/{abbr}");
    }
}

#[test]
fn a_covered_pro_team_resolves_a_mark_and_an_uncovered_college_team_falls_back() {
    // spec v3.4 §7: the pro leagues are complete, so every one of them
    // resolves — including the teams the v3.3 demo set never covered.
    for key in ["nba/bos", "nba/mem", "mlb/sea", "nhl/sea", "wnba/lv", "soccer/364"] {
        assert!(hero_mark(key).is_some(), "missing {key}");
    }
    // College ships the ranked top 25 only; an unranked school has no mark
    // and takes the abbreviation fallback — honestly, not as a hole.
    assert!(hero_mark("ncaa/999999").is_none(), "an unranked school has no mark");
    let s = draw_to_text(&team("SIE", "ncaa/999999"), 10, 6);
    assert!(s.contains("SIE"), "the uncovered team falls back to its abbreviation: {s}");
}

/// The pro leagues are complete as of v3.4 §7 — the counts are the research's
/// receipts off `/teams?limit=1000` (NFL 32, NHL 32, NBA 30, MLB 30, MLS 30,
/// EPL 20, WNBA 15).
#[test]
fn every_pro_league_is_complete() {
    let mut per_ns = std::collections::HashMap::<&str, usize>::new();
    for key in gameday::board::logo::committed_keys() {
        *per_ns.entry(key.split('/').next().unwrap()).or_default() += 1;
    }
    for (ns, want) in [("nfl", 32), ("nba", 30), ("mlb", 30), ("nhl", 32), ("wnba", 15)] {
        assert_eq!(per_ns.get(ns).copied().unwrap_or(0), want, "{ns} marks");
    }
    // EPL 20 + MLS 30 share the `soccer` bucket.
    assert_eq!(per_ns.get("soccer").copied().unwrap_or(0), 50, "soccer marks");
    // College is the ranked top 25 of each poll, deduped where a school is
    // ranked in both — so somewhere in 25..=50.
    let ncaa = per_ns.get("ncaa").copied().unwrap_or(0);
    assert!((25..=50).contains(&ncaa), "college marks: {ncaa} outside 25..=50");
}
