use gameday::app::{App, Tab};
use gameday::config::{Config, Pin};
use gameday::domain::*;
use ratatui::{backend::TestBackend, Terminal};

fn team(abbr: &str) -> Team {
    Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color: [200, 16, 46],
        alt_color: [255, 184, 28],
        logo_key: format!("nfl/{}", abbr.to_lowercase()),
    }
}

fn g(id: &str, away: &str, home: &str, live: bool) -> Game {
    Game {
        id: id.into(),
        league: League::Nfl,
        away: team(away),
        home: team(home),
        away_score: 27,
        home_score: 24,
        status: if live { Status::Live } else { Status::Pre },
        period: "Q4".into(),
        clock: "1:27".into(),
        situation: Some(Situation {
            down_distance: "1st & Goal".into(),
            possession: Some(away.into()),
            ball_on: Some("TB 3".into()),
        }),
        last_plays: vec![Play {
            clock: "1:27".into(),
            text: "Mahomes pass to Kelce for 3 yards".into(),
            scoring: false,
        }],
        meter: None,
        start_time: Some("8:20 PM".into()),
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

fn mk() -> App {
    let dir = std::env::temp_dir().join(format!("gd-draw-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    App::new(Config::default_nfl(), vec![], dir)
}

#[test]
fn header_and_tabs_render() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.to_lowercase().contains("gameday"), "{s}");
    assert!(s.to_lowercase().contains("home"), "{s}");
    assert!(s.to_lowercase().contains("nfl"), "{s}");
}

#[test]
fn footer_shows_chords() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("quit") || s.contains("q "), "{s}");
    assert!(s.contains("pin") || s.contains("space"), "{s}");
}

#[test]
fn empty_home_prompt() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("pin a game"), "{s}");
}

#[test]
fn too_small_message() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(30, 10)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("need more columns"), "{s}");
}

#[test]
fn nfl_tab_draws_live_score() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("27"), "{s}");
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains("LIVE"), "{s}");
}

#[test]
fn stale_flag_in_header() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], true);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("stale"), "{s}");
}
