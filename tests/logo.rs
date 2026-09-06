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
    for key in [
        "nfl/kc", "nfl/buf", "nfl/dal", "mlb/nyy", "nba/bos", "nhl/edm",
    ] {
        let m: &HeroMark = hero_mark(key).unwrap_or_else(|| panic!("{key} missing"));
        assert!(
            m.height >= 6 && m.height <= 10,
            "{key}: 16x10 regeneration, got {}",
            m.height
        );
        assert!(m.width >= 10 && m.width <= 16, "{key}: width {}", m.width);
    }
    // The pro leagues are complete, so "a team with no art" is no longer
    // a pro team — it's an unranked school.
    assert!(
        hero_mark("ncaa/999999").is_none(),
        "missing art is None, the caller falls back"
    );
}

#[test]
fn no_mark_exceeds_the_hero_slot() {
    for key in gameday::board::logo::committed_keys() {
        let m = hero_mark(key).unwrap_or_else(|| panic!("{key} missing"));
        assert!(
            m.width <= 16,
            "{key} wider than the 16x10 hero slot: {}",
            m.width
        );
        assert!(
            m.height <= 10,
            "{key} taller than the 16x10 hero slot: {}",
            m.height
        );
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
    assert!(
        non_blank >= 20,
        "mark painted only {non_blank} cells: {s:?}"
    );
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
        assert!(
            hero_mark(&format!("nfl/{abbr}")).is_some(),
            "missing nfl/{abbr}"
        );
    }
}

#[test]
fn a_covered_pro_team_resolves_a_mark_and_an_uncovered_college_team_falls_back() {
    // The pro leagues are complete, so every one of them
    // resolves — including the teams the demo set never covered.
    for key in [
        "nba/bos",
        "nba/mem",
        "mlb/sea",
        "nhl/sea",
        "wnba/lv",
        "soccer/364",
    ] {
        assert!(hero_mark(key).is_some(), "missing {key}");
    }
    // College ships the ranked top 25 only; an unranked school has no mark
    // and takes the abbreviation fallback — honestly, not as a hole.
    assert!(
        hero_mark("ncaa/999999").is_none(),
        "an unranked school has no mark"
    );
    let s = draw_to_text(&team("SIE", "ncaa/999999"), 10, 6);
    assert!(
        s.contains("SIE"),
        "the uncovered team falls back to its abbreviation: {s}"
    );
}

/// The pro leagues are complete — the counts are receipts off
/// `/teams?limit=1000` (NFL 32, NHL 32, NBA 30, MLB 30, MLS 30,
/// EPL 20, WNBA 15).
#[test]
fn every_pro_league_is_complete() {
    let mut per_ns = std::collections::HashMap::<&str, usize>::new();
    for key in gameday::board::logo::committed_keys() {
        *per_ns.entry(key.split('/').next().unwrap()).or_default() += 1;
    }
    for (ns, want) in [
        ("nfl", 32),
        ("nba", 30),
        ("mlb", 30),
        ("nhl", 32),
        ("wnba", 15),
    ] {
        assert_eq!(per_ns.get(ns).copied().unwrap_or(0), want, "{ns} marks");
    }
    // EPL 20 + MLS 30 share the `soccer` bucket.
    assert_eq!(
        per_ns.get("soccer").copied().unwrap_or(0),
        50,
        "soccer marks"
    );
    // College is the ranked top 25 of each poll, deduped where a school is
    // ranked in both — so somewhere in 25..=50.
    let ncaa = per_ns.get("ncaa").copied().unwrap_or(0);
    assert!(
        (25..=50).contains(&ncaa),
        "college marks: {ncaa} outside 25..=50"
    );
}

/// The light set. Not a filter over the dark art — a second render,
/// composited on daygame's paper, with ESPN's on-white variant where the
/// standard mark is contrast-hostile there. `hero_mark` picks the set by the
/// active theme's ground, so a light theme never gets the black-boxed art
/// that once kept daygame out of the built-ins.
#[test]
fn light_ground_selects_the_light_set() {
    // Default theme is broadcast — black ground, the standard set.
    let dark: Vec<Vec<gameday::board::logo::ArtCell>> =
        hero_mark("mlb/pit").unwrap().rows().to_vec();

    gameday::theme::set_current("daygame").unwrap();
    let light = hero_mark("mlb/pit").unwrap().rows().to_vec();
    assert_ne!(
        dark, light,
        "a light ground must resolve different art than a dark one"
    );

    // And back: dark themes see byte-identical behaviour.
    gameday::theme::set_current("broadcast").unwrap();
    assert_eq!(hero_mark("mlb/pit").unwrap().rows(), dark.as_slice());
}

/// The light set is not allowed to be thinner than the dark one: a theme
/// switch must never cost a team its mark.
#[test]
fn the_light_set_covers_every_committed_key() {
    let dark: Vec<&str> = gameday::board::logo::committed_keys().collect();
    let light: Vec<&str> = gameday::board::logo::light_keys().collect();
    assert_eq!(
        dark, light,
        "the two sets must carry the same keys in the same order"
    );
}

/// The quadrant-only rule again, on the new set: a light mark that draws
/// tofu on Terminal.app is no better than the black boxes it replaced.
#[test]
fn light_marks_are_quadrant_only_too() {
    gameday::theme::set_current("daygame").unwrap();
    for key in gameday::board::logo::light_keys() {
        let mark = hero_mark(key).unwrap_or_else(|| panic!("{key} missing from the light set"));
        for (y, row) in mark.rows().iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let o = cell.ch as u32;
                assert!(
                    cell.ch == ' ' || (0x2580..=0x259F).contains(&o),
                    "light {key} cell ({x},{y}) is U+{o:04X} — outside the quadrant range \
                     U+2580..=U+259F that every terminal draws"
                );
            }
        }
    }
}

/// WCAG contrast, against daygame's paper, of the colour a mark is mostly
/// made of — its most-repeated cell colour.
///
/// The dominant tone and not an average: art composited over black carries a
/// few very dark anti-aliasing cells that score 18:1 against paper, and an
/// average would let a mark's black halo vouch for the mark it surrounds. The
/// gold a Pirates P is drawn in is the thing that has to read.
fn dominant_contrast_on_paper(mark: &HeroMark) -> f64 {
    let lg = gameday::theme::rel_luma(gameday::theme::builtin("daygame").roles().ground);
    let mut counts = std::collections::HashMap::<(u8, u8, u8), usize>::new();
    for cell in mark.rows().iter().flatten() {
        for c in [cell.fg, cell.bg].into_iter().flatten() {
            *counts.entry(c).or_default() += 1;
        }
    }
    let Some((&(r, g, b), _)) = counts.iter().max_by_key(|(c, n)| (**n, **c)) else {
        return 0.0;
    };
    let l = gameday::theme::rel_luma(ratatui::style::Color::Rgb(r, g, b));
    (lg.max(l) + 0.05) / (lg.min(l) + 0.05)
}

/// The parking reason, measured on the committed art. `mlb/pit` is a
/// gold P: over black it is a perfect mark, and on daygame's paper the
/// dark-set art is a pale smudge — that gold reads 1.56:1 there, half the
/// WCAG graphics floor, where the light set's darkened gold reads 2.89:1.
/// These six are the marks the generator's lift had to rescue outright: each
/// one's dominant tone misses 3:1 on paper in the dark set, and each has to
/// come out of the light set visibly better, not marginally.
#[test]
fn light_art_reads_on_paper_where_the_dark_art_does_not() {
    let cases = [
        "mlb/pit",
        "mlb/sf",
        "ncaa/2633",
        "nfl/pit",
        "nfl/ten",
        "soccer/362",
    ];
    let mut dark = std::collections::HashMap::new();
    for key in cases {
        dark.insert(key, dominant_contrast_on_paper(hero_mark(key).unwrap()));
    }

    gameday::theme::set_current("daygame").unwrap();
    for key in cases {
        let light = dominant_contrast_on_paper(hero_mark(key).unwrap());
        assert!(
            dark[key] < 3.0,
            "{key}'s dominant tone already clears the 3:1 graphics floor on paper in the dark \
             set ({:.2}:1) — it is not a rescue case any more",
            dark[key]
        );
        assert!(
            light >= 1.4 * dark[key],
            "{key}'s dominant tone reads at {light:.2}:1 on daygame's paper against the dark \
             art's {:.2}:1 — the light set has to be visibly better, not marginally",
            dark[key]
        );
    }
}
