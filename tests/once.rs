use gameday::app::App;
use gameday::demo;
use gameday::domain::*;
use gameday::once::{self, OnceSource, Opts};
use gameday::provider::{ProviderError, SportsProvider};
use std::collections::HashMap;
use std::sync::Mutex;

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
    let text = once::render_text(&mut app, &opts(), false);
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

/// A game row's first cell is the mark: `▌` (tier 2, live) or `·` (tier 3,
/// pre/final) — see `rows::draw_tier2`/`draw_tier3`. A section rule row
/// (`board::draw_rule`) starts with neither, so counting on the mark counts
/// exactly the game rows, never a header.
fn game_row_marks(text: &str) -> Vec<char> {
    text.lines()
        .filter_map(|l| l.chars().next().filter(|c| *c == '▌' || *c == '·'))
        .collect()
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
        false,
    );
    assert_eq!(
        game_row_marks(&top).len(),
        3,
        "--top 3 keeps exactly three game rows:\n{top}"
    );
    let live = once::render_text(
        &mut app,
        &Opts {
            live: true,
            ..opts()
        },
        false,
    );
    let marks = game_row_marks(&live);
    assert!(!marks.is_empty(), "--live still has live rows:\n{live}");
    assert!(
        marks.iter().all(|c| *c == '▌'),
        "--live leaves only ▌ rows:\n{live}"
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
impl OnceSource for Failing {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard(league)
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
impl OnceSource for Slow {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard(league)
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

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gd-once-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// M2: `run` returns the outcome instead of printing/exiting — a total
/// fetch failure (nothing cached, nothing fetched) is code 1, one stderr
/// line, and empty stdout.
#[test]
fn a_total_fetch_failure_is_code_1_one_stderr_line_and_empty_stdout() {
    let mut config = demo::demo_config();
    config.enabled_tabs = vec![League::Nfl];
    let out = once::run(
        config,
        vec![],
        scratch("fail"),
        time::UtcOffset::UTC,
        &Failing,
        opts(),
    );
    assert_eq!(out.code, 1);
    assert_eq!(
        out.stderr,
        vec!["ESPN unreachable nfl scoreboard".to_string()]
    );
    assert_eq!(out.stdout, "");
}

struct Fresh;
impl OnceSource for Fresh {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        Ok((
            demo::demo_boards().remove(&league).unwrap_or_default(),
            false,
        ))
    }
}

/// M7: an emptied-out render (`--top 0` truncates every section to
/// nothing) must print nothing at all, not a bare blank line.
#[test]
fn top_0_prints_nothing_and_still_exits_0() {
    let mut config = demo::demo_config();
    config.enabled_tabs = vec![League::Nfl];
    let out = once::run(
        config,
        vec![],
        scratch("top0"),
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
        &Fresh,
        Opts {
            top: Some(0),
            ..opts()
        },
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "", "an emptied-out render must print nothing");
}

struct StaleOnce;
impl OnceSource for StaleOnce {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        Ok((
            demo::demo_boards().remove(&league).unwrap_or_default(),
            true,
        ))
    }
}

/// M5: `stale` (any league served from cache after a failed fetch) names
/// itself as the text form's first line — a script has no JSON `stale`
/// field to check.
#[test]
fn stale_output_names_itself_first_in_the_text_form() {
    let mut config = demo::demo_config();
    config.enabled_tabs = vec![League::Nfl];
    let out = once::run(
        config,
        demo::demo_pins(),
        scratch("stale"),
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
        &StaleOnce,
        opts(),
    );
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stdout.lines().next(),
        Some("STALE · cached scores, the network failed")
    );
}

/// A source whose boards are fixed, recording which leagues it was asked
/// for — I1's fetch-narrowing test doubles as the `--league`-means-it test.
struct Counting {
    boards: HashMap<League, Vec<Game>>,
    seen: Mutex<Vec<League>>,
}
impl OnceSource for Counting {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        self.seen.lock().unwrap().push(league);
        Ok((self.boards.get(&league).cloned().unwrap_or_default(), false))
    }
}

/// I1: with no `--league`, `--once` fetches (and renders) `config.
/// enabled_tabs`, never the whole league set — a disabled league must not
/// cost a request. `--league` means it: it both narrows the fetch and
/// becomes what renders, even for a league the config didn't enable.
#[test]
fn once_respects_enabled_tabs_and_league_narrows_both_fetch_and_render() {
    let mut boards = HashMap::new();
    boards.insert(
        League::Nfl,
        demo::demo_boards().remove(&League::Nfl).unwrap(),
    );
    boards.insert(
        League::Mlb,
        demo::demo_boards().remove(&League::Mlb).unwrap(),
    );
    let source = Counting {
        boards,
        seen: Mutex::new(Vec::new()),
    };
    let mut config = demo::demo_config();
    config.enabled_tabs = vec![League::Nfl];
    let offset = time::UtcOffset::from_hms(-4, 0, 0).unwrap();
    let dir = scratch("i1");

    let out = once::run(
        config.clone(),
        vec![],
        dir.clone(),
        offset,
        &source,
        Opts {
            json: true,
            ..opts()
        },
    );
    assert_eq!(out.code, 0, "{:?}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let leagues: Vec<&str> = v["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["league"].as_str().unwrap())
        .collect();
    assert!(
        !leagues.contains(&"mlb"),
        "no --league: enabled_tabs is [nfl], MLB must not render: {leagues:?}"
    );
    assert_eq!(
        source.seen.lock().unwrap().as_slice(),
        &[League::Nfl],
        "only NFL was fetched"
    );

    source.seen.lock().unwrap().clear();
    let out = once::run(
        config,
        vec![],
        dir,
        offset,
        &source,
        Opts {
            json: true,
            leagues: vec![League::Mlb],
            ..opts()
        },
    );
    assert_eq!(out.code, 0, "{:?}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let leagues: Vec<&str> = v["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["league"].as_str().unwrap())
        .collect();
    assert!(
        !leagues.is_empty() && leagues.iter().all(|l| *l == "mlb"),
        "--league mlb: only MLB rows render: {leagues:?}"
    );
    assert_eq!(
        source.seen.lock().unwrap().as_slice(),
        &[League::Mlb],
        "only MLB was fetched"
    );
}
