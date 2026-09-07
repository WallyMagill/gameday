use gameday::app::App;
use gameday::demo;
use gameday::domain::*;
use gameday::once::{self, Opts};
use gameday::provider::{ProviderError, SportsProvider};

fn demo_once() -> App {
    let dir = std::env::temp_dir().join(format!("gd-once-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let boards: Vec<(League, Vec<Game>, bool)> = demo::demo_boards()
        .into_iter()
        .map(|(l, g)| (l, g, false))
        .collect();
    once::build_app(
        demo::demo_config(),
        demo::demo_pins(),
        dir,
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
        time::macros::datetime!(2026-08-31 21:30:01 -4),
        boards,
    )
}

fn opts() -> Opts {
    Opts {
        json: false,
        leagues: vec![],
        live: false,
        top: None,
        color: false,
        width: once::ONCE_WIDTH,
    }
}

/// Compare to the committed golden; `UPDATE_GOLDEN=1 cargo test --test once` rewrites it.
fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — run with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    assert_eq!(
        actual,
        want,
        "{} differs from the golden; if the change is intended: UPDATE_GOLDEN=1 cargo test --test once",
        path.display()
    );
}

#[test]
fn the_text_form_matches_the_golden_and_is_the_boards_own_grid() {
    let mut app = demo_once();
    let text = once::render_text(&mut app, &opts());
    golden("once-demo.txt", &text);
    assert!(text.contains("IN PLAY") && text.contains("LATER"), "{text}");
    assert!(!text.contains('\x1b'), "no color without --color: {text}");
    // The same columns the board draws: the clock column sits where rows.rs puts it.
    let live = text.lines().find(|l| l.contains("Q4")).expect("a live row");
    assert_eq!(
        live.find("Q4").map(|b| live[..b].chars().count()),
        Some(23),
        "CLOCK_X\n{text}"
    );
}

#[test]
fn top_live_and_league_narrow_the_text() {
    let mut app = demo_once();
    let top = once::render_text(
        &mut app,
        &Opts {
            top: Some(3),
            ..opts()
        },
    );
    let rows = top
        .lines()
        .filter(|l| l.contains(" @ ") || l.contains("Q") || l.contains("FINAL"))
        .count();
    assert!(rows <= 3, "--top 3 keeps three game rows:\n{top}");
    let live = once::render_text(
        &mut app,
        &Opts {
            live: true,
            ..opts()
        },
    );
    assert!(
        !live.contains("LATER") && !live.contains("FINAL ─"),
        "--live drops the other sections:\n{live}"
    );
}

#[test]
fn the_json_form_matches_the_golden_and_pins_the_schema() {
    let mut app = demo_once();
    let v = once::render_json(&mut app, &opts(), false);
    golden("once-demo.json", &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["generated_at"], "2026-08-31T21:30:01-04:00");
    assert_eq!(v["stale"], false);
    let games = v["games"].as_array().expect("games");
    assert!(!games.is_empty());
    let keys: Vec<&str> = vec![
        "league",
        "id",
        "status",
        "period",
        "clock",
        "start",
        "away",
        "home",
        "watch",
        "situation",
        "last_play",
        "pinned",
    ];
    for g in games {
        let mut have: Vec<&str> = g.as_object().unwrap().keys().map(String::as_str).collect();
        have.sort();
        let mut want = keys.clone();
        want.sort();
        assert_eq!(have, want, "schema drift in {g}");
        assert!(["live", "pre", "final"].contains(&g["status"].as_str().unwrap()));
        for side in ["away", "home"] {
            for k in ["abbr", "name", "score", "record", "rank"] {
                assert!(g[side].get(k).is_some(), "{side}.{k} missing in {g}");
            }
        }
        for k in ["score", "chip", "why"] {
            assert!(g["watch"].get(k).is_some(), "watch.{k} missing in {g}");
        }
    }
    let pinned = games.iter().filter(|g| g["pinned"] == true).count();
    assert_eq!(pinned, demo::demo_pins().len(), "pins are marked");
}

struct Failing;
impl SportsProvider for Failing {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        Err(ProviderError::Http {
            status: 0,
            key: format!("{}-scoreboard", league.slug()),
            url: String::new(),
            detail: "connection refused".into(),
        })
    }
    fn scoreboard_on(&self, l: League, _: time::Date) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard(l)
    }
    fn summary(&self, _: League, _: &str) -> Result<(Summary, bool), ProviderError> {
        unreachable!()
    }
    fn stats(&self, _: League, _: &str) -> Result<(GameStats, bool), ProviderError> {
        unreachable!()
    }
    fn standings(&self, _: League) -> Result<(StandingsTable, bool), ProviderError> {
        unreachable!()
    }
}

struct Slow;
impl SportsProvider for Slow {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        Ok((
            demo::demo_boards().remove(&league).unwrap_or_default(),
            false,
        ))
    }
    fn scoreboard_on(&self, l: League, _: time::Date) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard(l)
    }
    fn summary(&self, _: League, _: &str) -> Result<(Summary, bool), ProviderError> {
        unreachable!()
    }
    fn stats(&self, _: League, _: &str) -> Result<(GameStats, bool), ProviderError> {
        unreachable!()
    }
    fn standings(&self, _: League) -> Result<(StandingsTable, bool), ProviderError> {
        unreachable!()
    }
}

#[test]
fn every_league_fails_with_nothing_cached_is_all_errors_and_the_fetch_runs_in_parallel() {
    let (boards, errors) = once::fetch(&Failing, &League::ALL);
    assert!(boards.is_empty());
    assert_eq!(errors.len(), League::ALL.len());
    assert!(errors[0].1.contains("ESPN unreachable"), "{:?}", errors[0]);
    let t = std::time::Instant::now();
    let (boards, errors) = once::fetch(&Slow, &League::ALL);
    assert!(errors.is_empty());
    assert_eq!(boards.len(), League::ALL.len());
    assert!(
        t.elapsed() < std::time::Duration::from_millis(900),
        "nine 300 ms fetches took {:?}: not parallel",
        t.elapsed()
    );
}
