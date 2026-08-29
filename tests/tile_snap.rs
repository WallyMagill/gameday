use gameday::domain::*;
use gameday::tiles::{render_tile, Density};
use ratatui::{backend::TestBackend, Terminal};

fn live_kc() -> Game {
    Game {
        id: "401".into(),
        league: League::Nfl,
        away: Team {
            id: "12".into(),
            abbr: "KC".into(),
            name: "Kansas City Chiefs".into(),
            color: [227, 24, 55],
            alt_color: [255, 184, 28],
            logo_key: "nfl/kc".into(),
        },
        home: Team {
            id: "27".into(),
            abbr: "TB".into(),
            name: "Tampa Bay Buccaneers".into(),
            color: [213, 10, 10],
            alt_color: [52, 48, 43],
            logo_key: "nfl/tb".into(),
        },
        away_score: 27,
        home_score: 24,
        status: Status::Live,
        period: "Q4".into(),
        clock: "1:27".into(),
        situation: Some(Situation {
            down_distance: "1st & Goal".into(),
            possession: Some("KC".into()),
            ball_on: Some("TB 3".into()),
        }),
        last_plays: vec![Play {
            clock: "1:27".into(),
            text: "Mahomes pass to Kelce for 3 yards".into(),
            scoring: false,
        }],
        meter: Some(Meter::RedZone { yards_to_goal: 3 }),
        start_time: None,
        broadcast: Some("CBS".into()),
    }
}

fn buf_text(term: &Terminal<TestBackend>) -> String {
    let b = term.backend().buffer();
    let area = b.area();
    let mut s = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            s.push_str(b[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

#[test]
fn standard_tile_shows_score_and_live() {
    let mut t = Terminal::new(TestBackend::new(40, 12)).unwrap();
    t.draw(|f| render_tile(f, f.area(), &live_kc(), Density::Standard, true)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("27"), "{s}");
    assert!(s.contains("24"), "{s}");
    assert!(s.contains("LIVE"), "{s}");
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains(" - ") || s.contains("-"), "{s}");
}

#[test]
fn compact_tile_has_no_last_play_text() {
    let mut t = Terminal::new(TestBackend::new(40, 4)).unwrap();
    t.draw(|f| render_tile(f, f.area(), &live_kc(), Density::Compact, false)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("27"), "{s}");
    assert!(!s.contains("Kelce"), "{s}");
}

#[test]
fn full_tile_includes_last_play_and_situation() {
    let mut t = Terminal::new(TestBackend::new(60, 20)).unwrap();
    t.draw(|f| render_tile(f, f.area(), &live_kc(), Density::Full, false)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("Kelce"), "{s}");
    assert!(s.contains("1st & Goal"), "{s}");
}
