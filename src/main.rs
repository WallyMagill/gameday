use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use gameday::app::App;
use gameday::config::{load_pins, Config};
use gameday::domain::*;
use gameday::provider::espn::EspnProvider;
use gameday::provider::SportsProvider;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

#[derive(Debug)]
struct Args {
    demo: bool,
    /// `dump`: write the fixed-name capture gallery (board-broadcast/-ceefax/
    /// -phosphor, board-compact, tab-nfl, focus, help, narrow) into out/.
    dump: bool,
    /// `dump --tick N`: capture the demo simulation at tick N (default 0).
    tick: u64,
    /// `probe <league>`: fetch + map one real scoreboard and print it. Dev-only.
    probe: Option<String>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let probe = args
        .iter()
        .position(|a| a == "probe")
        .map(|i| args.get(i + 1).cloned().unwrap_or_default());
    let tick = match args.iter().position(|a| a == "--tick") {
        None => 0,
        Some(i) => {
            let raw = args.get(i + 1).map(String::as_str).unwrap_or("");
            raw.parse::<u64>()
                .map_err(|_| format!("--tick expects a non-negative integer, got {raw:?}"))?
        }
    };
    Ok(Args {
        demo: args.iter().any(|a| a == "--demo"),
        dump: args.iter().any(|a| a == "dump" || a == "--dump"),
        tick,
        probe,
    })
}

enum Msg {
    Boards {
        league: League,
        games: Vec<Game>,
        stale: bool,
    },
    Summary {
        id: String,
        summary: Summary,
    },
}

fn main() -> std::io::Result<()> {
    let args = match parse_args(&std::env::args().collect::<Vec<_>>()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("gameday: {e}");
            std::process::exit(2);
        }
    };

    if args.dump {
        return gameday::dump::run(std::path::Path::new("out"), args.tick);
    }

    if let Some(slug) = args.probe {
        return probe(&slug);
    }

    if args.demo {
        // Demo state lives in a scratch dir so it never touches real pins/config.
        let dir = std::env::temp_dir().join(format!("gameday-demo-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // Seed at tick 0, then the simulator drives the board over the same
        // Msg channel the live provider uses — the UI path is identical.
        let app = gameday::dump::demo_app(dir, 0);
        let (tx, rx) = mpsc::channel::<Msg>();
        thread::spawn(move || sim_loop(tx));
        // The simulator ignores refresh requests; the flag is just unread.
        return run_ui(app, Some(rx), Arc::new(AtomicBool::new(false)));
    }

    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gameday");
    std::fs::create_dir_all(dir.join("cache"))?;
    let config = Config::load_from(&dir).unwrap_or_else(|_| Config::default_all());
    gameday::theme::set_current(gameday::theme::parse_or_default(&config.theme));
    let pins = load_pins(&dir).unwrap_or_default();
    let enabled_tabs = config.enabled_tabs.clone();
    let app = App::new(config, pins, dir.clone());

    let provider = EspnProvider::new(dir.join("cache"));
    let (tx, rx) = mpsc::channel::<Msg>();
    let tx_plan = tx.clone();
    // R in the UI sets this; poll_loop checks it every ~200ms tick and
    // treats the board timer as expired, forcing an immediate refetch.
    let refresh = Arc::new(AtomicBool::new(false));
    let refresh_poll = refresh.clone();
    thread::spawn(move || poll_loop(provider, tx_plan, enabled_tabs, refresh_poll));
    run_ui(app, Some(rx), refresh)
}

/// Dev verification: fetch and map one league's real scoreboard, print one
/// line per game. Not part of the TUI.
fn probe(slug: &str) -> std::io::Result<()> {
    let Some(league) = League::from_slug(slug) else {
        eprintln!(
            "probe: unknown league {slug:?}, expected one of: {}",
            League::ALL.map(|l| l.slug()).join("|")
        );
        std::process::exit(2);
    };
    let cache = std::env::temp_dir().join(format!("gameday-probe-{}", std::process::id()));
    let provider = EspnProvider::new(cache.clone());
    let result = provider.scoreboard(league);
    let _ = std::fs::remove_dir_all(&cache);
    match result {
        Ok((games, stale)) => {
            println!("{} games={} stale={stale}", league.slug(), games.len());
            for g in &games {
                let sit = g
                    .situation
                    .as_ref()
                    .map(|s| s.down_distance.clone())
                    .unwrap_or_default();
                println!(
                    "{:>10}  {:?}  {:<3} {:>3} @ {:<3} {:>3}  [{} {}]  meter={:?}  rec={}/{}  sit={:?}",
                    g.id, g.status, g.away.abbr, g.away_score, g.home.abbr, g.home_score,
                    g.period, g.clock, g.meter, g.away.record, g.home.record, sit,
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("probe: fetch/map failed for league={} : {e}", league.slug());
            std::process::exit(1);
        }
    }
}

/// Replace `league`'s live ids from a successful scoreboard. `None` (fetch error)
/// leaves that league's existing ids in place so summaries keep running.
fn merge_live_ids(live: &mut Vec<(League, String)>, league: League, fetched: Option<&[Game]>) {
    let Some(games) = fetched else {
        return;
    };
    live.retain(|(l, _)| *l != league);
    live.extend(
        games
            .iter()
            .filter(|g| g.status == Status::Live)
            .map(|g| (league, g.id.clone())),
    );
}

/// Demo counterpart of `poll_loop`: the scripted simulator advances one tick
/// per second and each tick's boards flow through the same `Msg::Boards`
/// channel, so `App::apply_boards` and the draw path see exactly what a real
/// provider would send. Exits when the UI drops the receiver.
fn sim_loop(tx: mpsc::Sender<Msg>) {
    let mut sim = gameday::sim::Simulator::new();
    loop {
        for (league, games) in sim.boards() {
            let msg = Msg::Boards {
                league: *league,
                games: games.clone(),
                stale: false,
            };
            if tx.send(msg).is_err() {
                return;
            }
        }
        thread::sleep(Duration::from_secs(1));
        sim.step();
    }
}

/// The scoreboard refetch gate: due when the timer expired OR the UI asked
/// for an immediate refresh (R). Consumes the request flag.
fn board_due(last_board: Instant, every: Duration, refresh: &AtomicBool) -> bool {
    refresh.swap(false, Ordering::Relaxed) || last_board.elapsed() >= every
}

fn poll_loop(
    provider: EspnProvider,
    tx: mpsc::Sender<Msg>,
    leagues: Vec<League>,
    refresh: Arc<AtomicBool>,
) {
    let mut last_board = Instant::now() - Duration::from_secs(999);
    let mut last_sum = Instant::now() - Duration::from_secs(999);
    let mut attempt = 0u32;
    // Scoreboard every enabled tab (Nfl default). Summaries stay live-only.
    let mut live: Vec<(League, String)> = vec![];
    loop {
        let every = if live.is_empty() {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(20)
        };
        if board_due(last_board, every, &refresh) {
            let mut any_failed = false;
            for league in &leagues {
                match provider.scoreboard(*league) {
                    Ok((games, stale)) => {
                        merge_live_ids(&mut live, *league, Some(&games));
                        let _ = tx.send(Msg::Boards {
                            league: *league,
                            games,
                            stale,
                        });
                        if stale {
                            any_failed = true;
                        }
                    }
                    Err(_) => {
                        any_failed = true;
                    }
                }
            }
            if any_failed {
                attempt = attempt.saturating_add(1);
                thread::sleep(Duration::from_secs(gameday::provider::espn::backoff_secs(
                    attempt,
                )));
            } else {
                attempt = 0;
            }
            last_board = Instant::now();
        }
        if last_sum.elapsed() >= Duration::from_secs(15) {
            for (league, id) in live.iter().take(4) {
                if let Ok((s, _)) = provider.summary(*league, id) {
                    let _ = tx.send(Msg::Summary {
                        id: id.clone(),
                        summary: s,
                    });
                }
            }
            last_sum = Instant::now();
        }
        thread::sleep(Duration::from_millis(200));
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

/// Input poll: short so keypresses redraw within ~50ms regardless of the
/// render cadence.
const INPUT_POLL: Duration = Duration::from_millis(50);
/// Render tick while anything is live: ~10fps drives all animation frames.
const LIVE_TICK: Duration = Duration::from_millis(100);
/// Render tick with nothing live: ~1fps keeps the header clock honest with
/// near-zero work.
const IDLE_TICK: Duration = Duration::from_millis(1000);

/// Two-speed loop: input is polled every [`INPUT_POLL`]; the render tick
/// (which advances `app.tick` and thus every animation) fires at
/// [`LIVE_TICK`]/[`IDLE_TICK`]. Keys and data messages redraw immediately but
/// never advance the tick, so keyboard actions cannot animate anything.
fn run_ui(
    mut app: App,
    rx: Option<mpsc::Receiver<Msg>>,
    refresh: Arc<AtomicBool>,
) -> std::io::Result<()> {
    enable_raw_mode()?;
    let _restore = RestoreTerminal;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut last_err: Option<std::io::Error> = None;
    let mut last_tick = Instant::now();
    let mut needs_draw = true;
    'ui: loop {
        if let Some(rx) = &rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    Msg::Boards {
                        league,
                        games,
                        stale,
                    } => app.apply_boards(league, games, stale),
                    Msg::Summary { id, summary } => app.merge_summary(&id, summary),
                }
                needs_draw = true;
            }
        }
        let tick_every = if app.any_live() { LIVE_TICK } else { IDLE_TICK };
        if last_tick.elapsed() >= tick_every {
            app.advance_tick();
            last_tick = Instant::now();
            needs_draw = true;
        }
        if needs_draw {
            if let Err(e) = terminal.draw(|f| app.draw(f)) {
                last_err = Some(e);
                break;
            }
            needs_draw = false;
        }
        if event::poll(INPUT_POLL)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    app.on_key(k.code, k.modifiers);
                    needs_draw = true;
                }
                Event::Resize(_, _) => needs_draw = true,
                _ => {}
            }
        }
        if app.refresh_now {
            app.refresh_now = false;
            // Hand the request to poll_loop, which checks each ~200ms tick.
            refresh.store(true, Ordering::Relaxed);
        }
        if app.should_quit {
            break 'ui;
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{board_due, merge_live_ids, parse_args};
    use gameday::domain::{Game, League, Status, Team};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn live_game(id: &str, league: League) -> Game {
        Game {
            id: id.into(),
            league,
            away: Team {
                id: "A".into(),
                abbr: "A".into(),
                name: "A".into(),
                logo_key: "nfl/a".into(),
                ..Default::default()
            },
            home: Team {
                id: "B".into(),
                abbr: "B".into(),
                name: "B".into(),
                logo_key: "nfl/b".into(),
                ..Default::default()
            },
            away_score: 0,
            home_score: 0,
            status: Status::Live,
            period: "Q1".into(),
            clock: "15:00".into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            start_time: None,
            broadcast: None,
        }
    }

    fn parsed(args: &[&str]) -> super::Args {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&owned).expect("valid args")
    }

    #[test]
    fn demo_flag() {
        assert!(parsed(&["gameday", "--demo"]).demo);
        assert!(!parsed(&["gameday"]).demo);
        assert!(parsed(&["gameday", "dump"]).dump);
        assert!(!parsed(&["gameday", "--demo"]).dump);
    }

    #[test]
    fn tick_flag_parses_and_defaults_to_zero() {
        assert_eq!(parsed(&["gameday", "dump"]).tick, 0);
        assert_eq!(parsed(&["gameday", "dump", "--tick", "15"]).tick, 15);
        // Bad or missing value is a readable error, not a silent 0.
        let owned: Vec<String> = ["gameday", "dump", "--tick", "abc"].iter().map(|s| s.to_string()).collect();
        let err = parse_args(&owned).unwrap_err();
        assert!(err.contains("abc"), "{err}");
        let owned: Vec<String> = ["gameday", "dump", "--tick"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&owned).is_err());
    }

    #[test]
    fn probe_flag_takes_league_slug() {
        let a = parsed(&["gameday", "probe", "wnba"]);
        assert_eq!(a.probe.as_deref(), Some("wnba"));
        assert_eq!(parsed(&["gameday"]).probe, None);
        // Missing slug still enters probe mode so it can print the expected set.
        assert_eq!(parsed(&["gameday", "probe"]).probe.as_deref(), Some(""));
    }

    #[test]
    fn refresh_flag_forces_an_immediate_board_fetch() {
        let every = Duration::from_secs(60);
        let fresh = Instant::now();
        let refresh = AtomicBool::new(false);
        assert!(!board_due(fresh, every, &refresh), "timer fresh, no request");
        refresh.store(true, Ordering::Relaxed);
        assert!(board_due(fresh, every, &refresh), "R makes the fetch due NOW");
        assert!(
            !refresh.load(Ordering::Relaxed),
            "the request is consumed by the check"
        );
        assert!(!board_due(fresh, every, &refresh), "one R, one forced fetch");
        let expired = Instant::now() - Duration::from_secs(61);
        assert!(board_due(expired, every, &refresh), "timer still works alone");
    }

    #[test]
    fn ok_nfl_replaces_live_ids() {
        let mut live = vec![(League::Nfl, "old".into())];
        merge_live_ids(
            &mut live,
            League::Nfl,
            Some(&[live_game("new", League::Nfl)]),
        );
        assert_eq!(live, vec![(League::Nfl, "new".into())]);
    }

    #[test]
    fn err_nfl_keeps_previous_live_ids() {
        let mut live = vec![(League::Nfl, "keep".into())];
        merge_live_ids(&mut live, League::Nfl, None);
        assert_eq!(live, vec![(League::Nfl, "keep".into())]);
    }

    #[test]
    fn ok_cfb_does_not_drop_nfl_ids() {
        let mut live = vec![(League::Nfl, "n1".into())];
        merge_live_ids(
            &mut live,
            League::Cfb,
            Some(&[live_game("c1", League::Cfb)]),
        );
        assert_eq!(
            live,
            vec![(League::Nfl, "n1".into()), (League::Cfb, "c1".into())]
        );
    }
}
