use gameday::domain::Team;
use gameday::tiles::logo::{draw_logo, load_logo};
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
    t.draw(|f| draw_logo(f, f.area(), team)).unwrap();
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
fn missing_logo_draws_abbr() {
    let s = draw_to_text(&team("KC", "nfl/does-not-exist"), 10, 6);
    assert!(s.contains("KC"), "{s}");
}

#[test]
fn bundled_mark_paints_cells() {
    let s = draw_to_text(&team("KC", "nfl/kc"), 10, 6);
    let non_blank = s.chars().filter(|c| *c != ' ').count();
    assert!(non_blank >= 20, "mark painted only {non_blank} cells: {s:?}");
}

#[test]
fn demo_marks_all_load() {
    for key in [
        "nfl/kc", "nfl/tb", "nba/den", "nba/bos", "mlb/nyy", "mlb/tor", "nhl/edm", "nhl/dal",
    ] {
        assert!(load_logo(key).is_some(), "missing {key}");
    }
}

#[test]
fn mark_fits_identity_slot() {
    for key in ["nfl/kc", "nba/bos", "mlb/nyy", "nhl/dal"] {
        let art = load_logo(key).unwrap();
        assert!(art.width <= 10, "{key} wider than slot: {}", art.width);
        assert!(art.cells.len() <= 6, "{key} taller than slot: {}", art.cells.len());
    }
}

#[test]
fn all_thirty_two_nfl_marks_load() {
    for abbr in [
        "ari", "atl", "bal", "buf", "car", "chi", "cin", "cle", "dal", "den", "det", "gb",
        "hou", "ind", "jax", "kc", "lv", "lac", "lar", "mia", "min", "ne", "no", "nyg",
        "nyj", "phi", "pit", "sea", "sf", "tb", "ten", "wsh",
    ] {
        assert!(load_logo(&format!("nfl/{abbr}")).is_some(), "missing nfl/{abbr}");
    }
}
