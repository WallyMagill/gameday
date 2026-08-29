use gameday::domain::Team;
use gameday::tiles::logo::{draw_logo, load_logo, parse_logo};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn parse_eight_by_five() {
    let raw = "\
##....##\n\
#.#..#.#\n\
#..##..#\n\
#.#..#.#\n\
##....##\n";
    let l = parse_logo(raw).unwrap();
    assert_eq!(l[0][0], '#');
    assert_eq!(l[4][7], '#');
}

#[test]
fn parse_rejects_wrong_size() {
    assert!(parse_logo("##\n##\n").is_none());
}

#[test]
fn missing_logo_draws_abbr() {
    let mut t = Terminal::new(TestBackend::new(8, 5)).unwrap();
    let team = Team {
        id: "x".into(), abbr: "KC".into(), name: "Chiefs".into(),
        color: [227, 24, 55], alt_color: [255, 184, 28],
        logo_key: "nfl/does-not-exist".into(),
    };
    t.draw(|f| draw_logo(f, f.area(), &team)).unwrap();
    let b = t.backend().buffer();
    let mut s = String::new();
    for y in 0..5 {
        for x in 0..8 {
            s.push_str(b[(x, y)].symbol());
        }
    }
    assert!(s.contains("KC"), "{s}");
}

#[test]
fn all_thirty_two_nfl_keys_load() {
    const KEYS: &[&str] = &[
        "nfl/ari", "nfl/atl", "nfl/bal", "nfl/buf", "nfl/car", "nfl/chi",
        "nfl/cin", "nfl/cle", "nfl/dal", "nfl/den", "nfl/det", "nfl/gb",
        "nfl/hou", "nfl/ind", "nfl/jax", "nfl/kc", "nfl/lv", "nfl/lac",
        "nfl/lar", "nfl/mia", "nfl/min", "nfl/ne", "nfl/no", "nfl/nyg",
        "nfl/nyj", "nfl/phi", "nfl/pit", "nfl/sea", "nfl/sf", "nfl/tb",
        "nfl/ten", "nfl/wsh",
    ];
    for k in KEYS {
        assert!(load_logo(k).is_some(), "missing {k}");
    }
}
