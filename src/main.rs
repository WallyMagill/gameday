use std::io::{stdout, IsTerminal};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
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
    /// `dump`: write the fixed-name capture gallery (board-<theme> for every
    /// built-in theme, board-compact, tab-nfl, focus, help, narrow, …) into out/.
    dump: bool,
    /// `dump --tick N`: capture the demo simulation at tick N (default 0).
    tick: u64,
    /// `probe <league>`: fetch + map one real scoreboard and print it. Dev-only.
    probe: Option<String>,
    help: bool,
    version: bool,
    config_dir: Option<PathBuf>,
}

const HELP: &str = "\
gameday — terminal sports board. Pin games, they tile.

USAGE
  gameday                 live board (needs a terminal)
  gameday --demo          scripted demo slate, no network
  gameday --config-dir P  use P instead of ~/.config/gameday
  gameday -h, --help      this text
  gameday -V, --version   version

KEYS  space pin · enter/z zoom · j/k move · tab league · [ ] date · / filter · : command · ? all keys · q quit
CONFIG  ~/.config/gameday/config.toml (or $XDG_CONFIG_HOME/gameday); pins.json, cache/ and themes/ beside it
DATA  unofficial ESPN JSON, polled; the last good payload is kept on disk and shown as STALE when the network fails

dev:
  gameday dump [--tick N]   write the capture gallery to out/ (no network)
  gameday probe <league>    fetch + map one live scoreboard and print it
";

fn parse_args(args: &[String]) -> Result<Args, String> {
    const VALID: &str =
        "--demo|--help|-h|--version|-V|--config-dir <path>|dump [--tick N]|probe <league>";
    let mut a = Args {
        demo: false,
        dump: false,
        tick: 0,
        probe: None,
        help: false,
        version: false,
        config_dir: None,
    };
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--demo" => a.demo = true,
            "--help" | "-h" => a.help = true,
            "--version" | "-V" => a.version = true,
            "dump" | "--dump" => a.dump = true,
            "--tick" => {
                let raw = it.next().map(String::as_str).unwrap_or("");
                a.tick = raw
                    .parse()
                    .map_err(|_| format!("--tick expects a non-negative integer, got {raw:?}"))?;
            }
            "probe" => a.probe = Some(it.next().cloned().unwrap_or_default()),
            "--config-dir" => {
                let p = it
                    .next()
                    .ok_or_else(|| "--config-dir expects a path".to_string())?;
                a.config_dir = Some(PathBuf::from(p));
            }
            other => return Err(format!("unknown argument {other:?}, valid: {VALID}")),
        }
    }
    Ok(a)
}

enum Msg {
    Boards {
        league: League,
        games: Vec<Game>,
        stale: bool,
    },
    /// A non-today slate for the date-traveled board ([`Msg::Boards`] stays
    /// the live path; dated slates never touch flash/score state).
    DatedBoards {
        league: League,
        date: time::Date,
        games: Vec<Game>,
    },
    Summary {
        id: String,
        summary: Summary,
    },
    Stats {
        id: String,
        stats: GameStats,
    },
    Standings(StandingsTable),
    /// A scoreboard fetch that failed: the provider's short error and how
    /// long the scheduler will wait before retrying that league.
    Failed {
        league: League,
        error: String,
        retry_in: Option<Duration>,
    },
}

/// What the UI wants polled right now, published by the UI thread and read
/// by the poll thread each tick. One snapshot replaces the old per-target
/// handshakes (zoom, standings, dated slate, league list, refresh flag).
type WantsShared = Arc<Mutex<gameday::poll::Wants>>;

/// R in the UI sets this; the poll thread swaps it false once per pass and
/// treats every scoreboard as due. Its own one-shot atomic, not a field of
/// [`WantsShared`]: a whole-struct republish (a game going live, a zoom
/// opening) would otherwise overwrite a pending request before the poll
/// thread ever saw it.
type RefreshFlag = Arc<AtomicBool>;

fn main() -> std::io::Result<()> {
    let args = match parse_args(&std::env::args().collect::<Vec<_>>()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("gameday: {e}");
            std::process::exit(2);
        }
    };

    if args.help {
        print!("{HELP}");
        return Ok(());
    }
    if args.version {
        println!("gameday {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.dump {
        let out = std::path::Path::new("out");
        return gameday::dump::run(out, args.tick);
    }

    if let Some(slug) = args.probe {
        return probe(&slug);
    }

    if args.demo {
        if let Err(e) = require_tty() {
            eprintln!("gameday: {e}");
            std::process::exit(1);
        }
        // Demo state lives in a scratch dir so it never touches real pins/config.
        let dir = std::env::temp_dir().join(format!("gameday-demo-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // Seed at tick 0, then the simulator drives the board over the same
        // Msg channel the live provider uses — the UI path is identical.
        let app = gameday::dump::demo_app(dir, 0);
        let (tx, rx) = mpsc::channel::<Msg>();
        thread::spawn(move || sim_loop(tx));
        // The simulator ignores what the UI wants polled; the snapshot is
        // published and simply never read.
        return run_ui(app, Some(rx), WantsShared::default(), RefreshFlag::default());
    }

    if let Err(e) = require_tty() {
        eprintln!("gameday: {e}");
        std::process::exit(1);
    }

    // Task 12's config::resolve_dir replaces this line; the flag works now.
    let dir = args.config_dir.clone().unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("gameday")
    });
    std::fs::create_dir_all(dir.join("cache"))?;
    let mut config = Config::load_from(&dir).unwrap_or_else(|_| Config::default_all());
    // User theme files first, so config.theme may name one of them; an
    // unknown name falls back to broadcast with a stderr note.
    gameday::theme::install_user_themes(&dir);
    config.theme = gameday::theme::select_or_default(&config.theme);
    let pins = load_pins(&dir).unwrap_or_default();
    // Read the local offset here, on the main thread, before the poll thread
    // exists — `time` refuses the TZ database once the process is threaded.
    // The app and the mapper share this one value.
    let offset = gameday::text::startup_offset();
    let app = App::new(config, pins, dir.clone(), offset);

    let provider = EspnProvider::new(dir.join("cache"), offset);
    let (tx, rx) = mpsc::channel::<Msg>();
    // The single UI -> poll handshake: enabled leagues, liveness, the zoomed
    // game, the traveled slate, the standings league, and R. The UI
    // republishes it only when it changes.
    let wants = WantsShared::default();
    let wants_poll = wants.clone();
    let refresh = RefreshFlag::default();
    let refresh_poll = refresh.clone();
    thread::spawn(move || poll_loop(provider, tx, wants_poll, refresh_poll));
    run_ui(app, Some(rx), wants, refresh)
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
    let provider = EspnProvider::new(cache.clone(), gameday::text::startup_offset());
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
                    "{:>10}  {:?}  {:<3} {:>3} @ {:<3} {:>3}  [{} {}]  meter={:?}  rec={}/{}  sit={:?}  odds={:?}",
                    g.id, g.status, g.away.abbr, g.away_score, g.home.abbr, g.home_score,
                    g.period, g.clock, g.meter, g.away.record, g.home.record, sit, g.odds,
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

fn poll_loop(
    provider: EspnProvider,
    tx: mpsc::Sender<Msg>,
    wants: WantsShared,
    refresh: RefreshFlag,
) {
    use gameday::poll::{Request, Scheduler};
    // Seeded per process so two gameday instances on one machine don't line
    // their fetches up on the same instant.
    let mut sched = Scheduler::new(std::process::id() as u64);
    loop {
        let w = wants.lock().map(|w| w.clone()).unwrap_or_default();
        // One R, one forced round: consumed here, so a second R that lands
        // mid-round is still honoured on the next pass.
        let refresh_now = refresh.swap(false, Ordering::Relaxed);
        let now = Instant::now();
        for req in sched.due(&w, refresh_now, now) {
            match &req {
                // Reported before the message is sent: `retry_in` is the
                // delay this failure just produced, which only exists once
                // report() has bumped the attempt count.
                Request::Scoreboard(league) => match provider.scoreboard(*league) {
                    Ok((games, stale)) => {
                        sched.report(&req, true, Instant::now());
                        let _ = tx.send(Msg::Boards {
                            league: *league,
                            games,
                            stale,
                        });
                    }
                    Err(e) => {
                        let done = Instant::now();
                        sched.report(&req, false, done);
                        let _ = tx.send(Msg::Failed {
                            league: *league,
                            error: e.short(),
                            retry_in: sched.next_retry(*league, done),
                        });
                    }
                },
                // Only scoreboards carry per-league backoff, so the rest
                // report nothing — the freshness windows in `due` pace them.
                Request::Summary(league, id) => {
                    if let Ok((summary, _)) = provider.summary(*league, id) {
                        let _ = tx.send(Msg::Summary {
                            id: id.clone(),
                            summary,
                        });
                    }
                }
                Request::Stats(league, id) => {
                    if let Ok((stats, _)) = provider.stats(*league, id) {
                        let _ = tx.send(Msg::Stats {
                            id: id.clone(),
                            stats,
                        });
                    }
                }
                Request::Dated(league, date) => {
                    if let Ok((games, _)) = provider.scoreboard_on(*league, *date) {
                        let _ = tx.send(Msg::DatedBoards {
                            league: *league,
                            date: *date,
                            games,
                        });
                    }
                }
                Request::Standings(league) => {
                    if let Ok((table, _)) = provider.standings(*league) {
                        let _ = tx.send(Msg::Standings(table));
                    }
                }
            }
        }
        thread::sleep(gameday::poll::TICK);
    }
}

/// Live and `--demo` need a real terminal; `dump`, `probe`, `--help`, and
/// `--version` must keep working piped (scripts, CI, `| head`).
fn require_tty() -> Result<(), String> {
    if std::io::stdout().is_terminal() {
        Ok(())
    } else {
        Err("gameday needs a terminal (stdout is not a tty); try --help".to_string())
    }
}

/// Installed at the top of `run_ui`: on panic, restore the terminal (leave
/// the alternate screen, disable raw mode) before the default hook prints,
/// so the backtrace lands on a normal scrollback instead of a wrecked TUI.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        eprintln!(
            "\ngameday {} crashed — please file this with the lines below:",
            env!("CARGO_PKG_VERSION")
        );
        default(info);
    }));
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
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
    wants: WantsShared,
    refresh: RefreshFlag,
) -> std::io::Result<()> {
    install_panic_hook();
    let mut published = gameday::poll::Wants::default();
    enable_raw_mode()?;
    let _restore = RestoreTerminal;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
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
                    Msg::DatedBoards {
                        league,
                        date,
                        games,
                    } => app.merge_dated_board(league, date, games),
                    Msg::Summary { id, summary } => app.merge_summary(&id, summary),
                    Msg::Stats { id, stats } => app.merge_stats(&id, stats),
                    Msg::Standings(table) => app.merge_standings(table),
                    Msg::Failed {
                        league,
                        error,
                        retry_in,
                    } => app.note_failure(league, error, retry_in),
                }
                needs_draw = true;
            }
        }
        // A banner that just started rings the terminal bell once. Raw byte
        // to stdout — BEL never disturbs the alternate-screen buffer.
        if app.bell_pending {
            app.bell_pending = false;
            use std::io::Write;
            let mut out = stdout();
            let _ = out.write_all(b"\x07");
            let _ = out.flush();
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
                    gameday::input::handle_key(&mut app, k.code, k.modifiers);
                    needs_draw = true;
                }
                Event::Mouse(m) => {
                    gameday::keymap::on_mouse(&mut app, m);
                    needs_draw = true;
                }
                Event::Resize(_, _) => needs_draw = true,
                _ => {}
            }
        }
        if app.refresh_now {
            // Hand the request to poll_loop, which swaps it each ~200ms tick.
            refresh.store(true, Ordering::Relaxed);
            app.refresh_now = false;
        }
        // One snapshot of what the poll thread should be fetching. Written
        // under the lock only when it differs from the last publish, so the
        // mutex isn't touched every 50ms input poll.
        let next = gameday::poll::Wants {
            leagues: app.config.enabled_tabs.clone(),
            any_live: app.any_live(),
            zoomed: app.stats_target(),
            dated: app.dated_target(),
            standings: app.standings_target(),
        };
        if next != published {
            if let Ok(mut w) = wants.lock() {
                *w = next.clone();
                published = next;
            }
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
    use super::parse_args;

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
    fn help_and_version_flags_parse_and_unknown_flags_name_the_valid_set() {
        assert!(parsed(&["gameday", "--help"]).help);
        assert!(parsed(&["gameday", "-h"]).help);
        assert!(parsed(&["gameday", "--version"]).version);
        assert!(parsed(&["gameday", "-V"]).version);
        assert_eq!(parsed(&["gameday", "--config-dir", "/tmp/x"]).config_dir.as_deref(), Some(std::path::Path::new("/tmp/x")));
        let owned: Vec<String> = ["gameday", "--nonsense"].iter().map(|s| s.to_string()).collect();
        let err = parse_args(&owned).unwrap_err();
        assert!(err.contains("--nonsense") && err.contains("--demo") && err.contains("--help"), "{err}");
        let owned: Vec<String> = ["gameday", "nonsense"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&owned).is_err(), "bare unknown words are errors too");
        let owned: Vec<String> = ["gameday", "--config-dir"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&owned).unwrap_err().contains("--config-dir expects a path"));
    }

    #[test]
    fn help_text_lists_every_flag_and_the_dev_commands_under_their_own_heading() {
        for needle in ["--demo", "--help", "--version", "--config-dir", "dev:", "dump", "probe", "--tick", "~/.config/gameday"] {
            assert!(super::HELP.contains(needle), "HELP missing {needle}");
        }
        assert!(super::HELP.lines().count() < 30, "help must fit a small terminal");
    }
}
