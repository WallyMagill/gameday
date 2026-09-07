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
use gameday::config::{load_config, load_pins_outcome, resolve_dir, Config, Pin};
use gameday::domain::*;
use gameday::frame;
use gameday::once;
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
    /// Whether `--tick` was actually given. `dump` defaults to 0; `frame`
    /// defaults to the scenario's own beat, and only an explicit flag overrides it.
    tick_given: bool,
    /// `frame ...`: one parameterized capture, fully specified on the command line.
    frame: Option<gameday::frame::Spec>,
    /// `probe <league>`: fetch + map one real scoreboard and print it. Dev-only.
    probe: Option<String>,
    help: bool,
    version: bool,
    config_dir: Option<PathBuf>,
    /// `--once`: fetch every enabled/named league once, print the ranked
    /// board, exit. The flags below only mean anything under it.
    once: bool,
    json: bool,
    leagues: Vec<League>,
    live: bool,
    top: Option<usize>,
    color: bool,
}

const HELP: &str = "\
gameday — terminal sports board. Pin games, they tile.

USAGE
  gameday                 live board (needs a terminal)
  gameday --demo          scripted demo slate, no network
  gameday --once [--json] [--league L]... [--live] [--top N] [--color]
                          fetch once, print the ranked board (text, or JSON for scripts), exit
  gameday --config-dir P  use P instead of ~/.config/gameday
  gameday -h, --help      this text
  gameday -V, --version   version

KEYS  space pin · enter/z zoom · j/k move · tab league · [ ] date · / filter · : command · ? all keys · q quit
CONFIG  ~/.config/gameday/config.toml (or $XDG_CONFIG_HOME/gameday); pins.json, gameday.log, cache/ and themes/ beside it
DATA  unofficial ESPN JSON, polled; the last good payload is kept on disk and shown as STALE when the network fails

dev:
  gameday dump [--tick N]   write the capture gallery to out/ (no network)
  gameday frame [flags]     render ONE surface to --out (design loop; see docs/design-loop.md)
      --view V      board|tv|zoom|cut-full|cut-band|standings|plays|config|help|theme-picker|filter
      --theme T     a loaded theme name, or a path to a theme .toml
      --size WxH    default 120x36
      --scenario S  full-slate|redzone|thin-slate|finals-only|empty|nudge-resort
      --tick N      sim tick (default: the scenario's own beat)
      --out PATH    writes PATH plus .ansi/.html beside it
  gameday probe <league>    fetch + map one live scoreboard and print it
";

fn parse_args(args: &[String]) -> Result<Args, String> {
    const VALID: &str =
        "--demo|--help|-h|--version|-V|--config-dir <path>|--once [--json] [--league L]... [--live] [--top N] [--color]|dump [--tick N]|frame [--view V --theme T --size WxH --scenario S --tick N --out PATH]|probe <league>";
    let mut a = Args {
        demo: false,
        dump: false,
        tick: 0,
        tick_given: false,
        frame: None,
        probe: None,
        help: false,
        version: false,
        config_dir: None,
        once: false,
        json: false,
        leagues: Vec::new(),
        live: false,
        top: None,
        color: false,
    };
    let mut it = args.iter().skip(1);
    // `frame`'s flags are collected raw and validated together after the
    // loop, so `--size` can be checked whether or not `frame` came first.
    let (mut frame, mut view, mut theme, mut size, mut scenario, mut out) =
        (false, None, None, None, None, None);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--demo" => a.demo = true,
            "--help" | "-h" => a.help = true,
            "--version" | "-V" => a.version = true,
            "dump" | "--dump" => a.dump = true,
            "frame" => frame = true,
            "--view" => view = Some(next_value(&mut it, "--view")?),
            "--theme" => theme = Some(next_value(&mut it, "--theme")?),
            "--size" => size = Some(next_value(&mut it, "--size")?),
            "--scenario" => scenario = Some(next_value(&mut it, "--scenario")?),
            "--out" => out = Some(next_value(&mut it, "--out")?),
            "--tick" => {
                let raw = it.next().map(String::as_str).unwrap_or("");
                a.tick = raw
                    .parse()
                    .map_err(|_| format!("--tick expects a non-negative integer, got {raw:?}"))?;
                a.tick_given = true;
            }
            "probe" => a.probe = Some(it.next().cloned().unwrap_or_default()),
            "--config-dir" => {
                let p = it
                    .next()
                    .ok_or_else(|| "--config-dir expects a path".to_string())?;
                a.config_dir = Some(PathBuf::from(p));
            }
            "--once" => a.once = true,
            "--json" => a.json = true,
            "--league" => {
                let slug = next_value(&mut it, "--league")?;
                match League::from_slug(&slug) {
                    Some(l) => a.leagues.push(l),
                    None => {
                        return Err(format!(
                            "--league {slug:?} is not a league, valid: {}",
                            League::ALL.map(League::slug).join("|")
                        ))
                    }
                }
            }
            "--live" => a.live = true,
            "--top" => {
                let raw = it.next().map(String::as_str).unwrap_or("");
                a.top =
                    Some(raw.parse().map_err(|_| {
                        format!("--top expects a non-negative integer, got {raw:?}")
                    })?);
            }
            "--color" => a.color = true,
            other => return Err(format!("unknown argument {other:?}, valid: {VALID}")),
        }
    }
    // The frame flags only mean anything under `frame`: naming one without it
    // is a typo worth reporting, not a flag to swallow.
    if !frame {
        for (flag, given) in [
            ("--view", &view),
            ("--size", &size),
            ("--scenario", &scenario),
            ("--out", &out),
            ("--theme", &theme),
        ] {
            if given.is_some() {
                return Err(format!("{flag} is a `gameday frame` flag; valid: {VALID}"));
            }
        }
    } else {
        let (cols, rows) = match size {
            Some(s) => frame::parse_size(&s)?,
            None => frame::DEFAULT_SIZE,
        };
        a.frame = Some(frame::Spec {
            view: frame::FrameView::parse(view.as_deref().unwrap_or("board"))?,
            scenario: frame::Scenario::parse(scenario.as_deref().unwrap_or("full-slate"))?,
            theme: theme.unwrap_or_else(|| "broadcast".to_string()),
            cols,
            rows,
            tick: a.tick_given.then_some(a.tick),
            out: PathBuf::from(out.ok_or_else(|| {
                "frame expects --out PATH (e.g. --out out/design/tv-studio-80.png)".to_string()
            })?),
        });
    }
    // Same shape as the frame check above: these flags only mean anything
    // under `--once`, so naming one without it is a typo worth reporting.
    if !a.once {
        for (flag, given) in [
            ("--json", a.json),
            ("--live", a.live),
            ("--color", a.color),
            ("--top", a.top.is_some()),
            ("--league", !a.leagues.is_empty()),
        ] {
            if given {
                return Err(format!("{flag} is a `gameday --once` flag; valid: {VALID}"));
            }
        }
    }
    Ok(a)
}

/// The value after a flag that requires one; the error names the flag.
fn next_value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next()
        .cloned()
        .ok_or_else(|| format!("{flag} expects a value"))
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
    /// An on-demand fetch that failed (standings, a dated slate, the zoomed
    /// game's summary/stats). Unlike a scoreboard failure these have no
    /// header chip, so the view that asked for the data shows the error.
    /// `what` is the request kind, matching `App::aux_errors`' key.
    AuxFailed {
        league: League,
        what: &'static str,
        error: String,
    },
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

    if let Some(spec) = &args.frame {
        return gameday::frame::run(spec);
    }

    if args.dump {
        let out = std::path::Path::new("out");
        return gameday::dump::run(out, args.tick);
    }

    if let Some(slug) = args.probe {
        return probe(&slug);
    }

    if args.once {
        let (dir, config, pins, _config_error) = load_state(args.config_dir.clone())?;
        // Read the local offset here, on the main thread, before any fetch
        // thread exists — `time` refuses the TZ database once the process
        // is threaded.
        let offset = gameday::text::startup_offset();
        // No alternate screen on this path: notes go to the log, not
        // stdout, so a script reading `--once`'s own stdout never sees one.
        gameday::log::set_file(dir.join("gameday.log"));
        let provider = EspnProvider::new(dir.join("cache"), offset);
        // A real terminal sizes the board to fit it; a pipe (a script, a
        // status bar, `| head`) gets the fixed ONCE_WIDTH instead of
        // whatever width a redirected stdout would misreport.
        let width = if stdout().is_terminal() {
            crossterm::terminal::size()
                .map(|(w, _)| w)
                .unwrap_or(once::ONCE_WIDTH)
        } else {
            once::ONCE_WIDTH
        };
        let opts = once::Opts {
            json: args.json,
            leagues: args.leagues,
            live: args.live,
            top: args.top,
            color: args.color,
            width,
        };
        let outcome = once::run(config, pins, dir, offset, &provider, opts);
        // Nothing prints when there's nothing to print — a `--top 0`
        // (or any other narrowing that empties every section) must not
        // leave a bare blank line on stdout.
        if !outcome.stdout.is_empty() {
            println!("{}", outcome.stdout);
        }
        for line in &outcome.stderr {
            eprintln!("gameday: {line}");
        }
        std::process::exit(outcome.code);
    }

    if args.demo {
        if let Err(e) = require_tty() {
            eprintln!("gameday: {e}");
            std::process::exit(1);
        }
        // Demo state lives in a scratch dir so it never touches real
        // pins/config — unless --config-dir names one, which always wins.
        let dir = args.config_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("gameday-demo-{}", std::process::id()))
        });
        std::fs::create_dir_all(&dir)?;
        // Past this point the alternate screen owns the terminal, so notes go
        // to a file instead of over the board.
        gameday::log::set_file(dir.join("gameday.log"));
        // Seed at tick 0, then the simulator drives the board over the same
        // Msg channel the live provider uses — the UI path is identical.
        let app = gameday::dump::demo_app(dir, 0);
        let (tx, rx) = mpsc::channel::<Msg>();
        thread::spawn(move || sim_loop(tx));
        // The simulator ignores what the UI wants polled; the snapshot is
        // published and simply never read.
        return run_ui(
            app,
            Some(rx),
            WantsShared::default(),
            RefreshFlag::default(),
        );
    }

    if let Err(e) = require_tty() {
        eprintln!("gameday: {e}");
        std::process::exit(1);
    }

    let (dir, mut config, pins, config_error) = load_state(args.config_dir.clone())?;
    // User theme files first, so config.theme may name one of them; an
    // unknown name falls back to broadcast with a stderr note.
    gameday::theme::install_user_themes(&dir);
    let (theme_name, theme_note) = gameday::theme::select_or_default_noting(&config.theme);
    if let Some(note) = &theme_note {
        eprintln!("gameday: {note}");
    }
    config.theme = theme_name;
    // Read the local offset here, on the main thread, before the poll thread
    // exists — `time` refuses the TZ database once the process is threaded.
    // The app and the mapper share this one value.
    let offset = gameday::text::startup_offset();
    let mut app = App::new(config, pins, dir.clone(), offset);
    app.set_notifier(gameday::notify::os_backend());
    // Also lands in the footer: the alternate screen swallows the stderr note
    // the moment the UI starts.
    app.set_config_error(config_error);
    // A config parse error takes priority over the theme note — it already
    // landed in the footer above, and a broken config is the more urgent
    // thing to fix.
    if app.status_line.is_none() {
        if let Some(note) = theme_note {
            app.sticky_status(note);
        }
    }

    // Last stderr note before the alternate screen: from here on, the mapper's
    // skip lines go to gameday.log rather than over the board.
    gameday::log::set_file(dir.join("gameday.log"));

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

/// The config, pins and resolved dir every real (non-demo) startup path
/// shares — the live TUI and `--once` both begin here. Stops short of theme
/// resolution: the TUI picks a theme after this (and folds a broken one into
/// its footer); `--once` has no footer and never touches themes at all.
fn load_state(
    config_dir: Option<PathBuf>,
) -> std::io::Result<(PathBuf, Config, Vec<Pin>, Option<String>)> {
    let resolved = resolve_dir(
        config_dir,
        &dirs::home_dir().unwrap_or_default(),
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .as_deref(),
        dirs::config_dir().map(|d| d.join("gameday")).as_deref(),
    );
    if let Some(l) = &resolved.legacy_read_from {
        eprintln!(
            "gameday: reading config from {} — gameday now writes to {}; move the folder to keep one copy",
            l.display(),
            resolved.dir.display()
        );
    }
    let dir = resolved.dir.clone();
    std::fs::create_dir_all(dir.join("cache"))?;
    let loaded = load_config(&resolved);
    let pins_loaded = load_pins_outcome(&resolved);
    // A file we could not parse is reported and left alone: the app runs on
    // defaults and every save is refused until the user fixes it.
    let config_error = loaded.error.clone().or_else(|| pins_loaded.error.clone());
    if let Some(err) = &config_error {
        eprintln!("gameday: {err} — running on defaults, not saving until it parses");
    }
    Ok((dir, loaded.value, pins_loaded.value, config_error))
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
    // GAMEDAY_LOG_REQUESTS=1 prints one stderr line per outgoing request
    // (seconds since loop start + the request), so a run can be measured
    // against the documented budget without changing any behaviour.
    let log_requests = std::env::var("GAMEDAY_LOG_REQUESTS").is_ok_and(|v| v == "1");
    let started = Instant::now();
    loop {
        let w = wants.lock().map(|w| w.clone()).unwrap_or_default();
        // One R, one forced round: consumed here, so a second R that lands
        // mid-round is still honoured on the next pass.
        let refresh_now = refresh.swap(false, Ordering::Relaxed);
        let now = Instant::now();
        for req in sched.due(&w, refresh_now, now) {
            if log_requests {
                eprintln!("{:9.3} {req:?}", started.elapsed().as_secs_f64());
            }
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
                // The rest carry no per-league backoff — the freshness
                // windows in `due` pace them — but a failure still has to be
                // both visible and retried soon, which is `report_aux`.
                Request::Summary(league, id) => match provider.summary(*league, id) {
                    Ok((summary, _)) => {
                        let _ = tx.send(Msg::Summary {
                            id: id.clone(),
                            summary,
                        });
                    }
                    Err(e) => aux_failed(&mut sched, &tx, &req, *league, "summary", e),
                },
                Request::Stats(league, id) => match provider.stats(*league, id) {
                    Ok((stats, _)) => {
                        let _ = tx.send(Msg::Stats {
                            id: id.clone(),
                            stats,
                        });
                    }
                    Err(e) => aux_failed(&mut sched, &tx, &req, *league, "stats", e),
                },
                Request::Dated(league, date) => match provider.scoreboard_on(*league, *date) {
                    Ok((games, _)) => {
                        let _ = tx.send(Msg::DatedBoards {
                            league: *league,
                            date: *date,
                            games,
                        });
                    }
                    Err(e) => aux_failed(&mut sched, &tx, &req, *league, "dated", e),
                },
                Request::Standings(league) => match provider.standings(*league) {
                    Ok((table, _)) => {
                        let _ = tx.send(Msg::Standings(table));
                    }
                    Err(e) => aux_failed(&mut sched, &tx, &req, *league, "standings", e),
                },
            }
        }
        thread::sleep(gameday::poll::TICK);
    }
}

/// One failed on-demand fetch: rewind the scheduler's freshness stamp so the
/// next pass past `AUX_RETRY` asks again (without it, a failed standings fetch
/// would sit behind the 10-minute TTL), and tell the UI which fetch is missing.
fn aux_failed(
    sched: &mut gameday::poll::Scheduler,
    tx: &mpsc::Sender<Msg>,
    req: &gameday::poll::Request,
    league: League,
    what: &'static str,
    error: gameday::provider::ProviderError,
) {
    sched.report_aux(req, false, Instant::now());
    let _ = tx.send(Msg::AuxFailed {
        league,
        what,
        error: error.short(),
    });
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

/// Leave the alternate screen and raw mode. The one place that does it, so
/// the panic hook and the RAII guard can never drift apart.
fn restore_terminal() {
    #[cfg(test)]
    RESTORE_CALLS.fetch_add(1, Ordering::Relaxed);
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// How many times [`restore_terminal`] ran — the only thing a test can
/// observe about a hook that otherwise just talks to the terminal.
#[cfg(test)]
static RESTORE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Does a panic on `panicking` justify tearing the terminal down? Only when
/// it is the thread that owns the screen. The poll thread panicking is bad,
/// but it does not end the process — dropping the UI out of the alternate
/// screen underneath a still-running board would turn a background failure
/// into a wrecked display.
fn should_restore(panicking: std::thread::ThreadId, ui: std::thread::ThreadId) -> bool {
    panicking == ui
}

/// Installed at the top of `run_ui`: on a panic *on this thread*, restore the
/// terminal (leave the alternate screen, disable raw mode) before the default
/// hook prints, so the backtrace lands on a normal scrollback instead of a
/// wrecked TUI. A panic on any other thread only prints.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    let ui_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if should_restore(std::thread::current().id(), ui_thread) {
            restore_terminal();
        }
        eprintln!(
            "\ngameday {} crashed — please file this with the lines below:",
            env!("CARGO_PKG_VERSION")
        );
        default(info);
    }));
}

/// SIGTERM / SIGHUP (a closed terminal, `kill`, a session manager) set this
/// flag; the UI loop treats it as `q`, so the one restore path runs and the
/// terminal is never left in raw mode. Ctrl-C arrives as a key event through
/// crossterm and is handled by the keymap, not here.
#[cfg(unix)]
fn install_signal_flag() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP] {
        // Registration only fails for signals that cannot be caught; TERM
        // and HUP can, and a failure here would just leave the old behavior.
        let _ = signal_hook::flag::register(sig, Arc::clone(&flag));
    }
    flag
}

#[cfg(not(unix))]
fn install_signal_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        restore_terminal();
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
    let term_signal = install_signal_flag();
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
                    Msg::AuxFailed {
                        league,
                        what,
                        error,
                    } => app.note_aux_failure(league, what, error),
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
            catchup: app.catchup_wants(),
        };
        if next != published {
            if let Ok(mut w) = wants.lock() {
                *w = next.clone();
                published = next;
            }
        }
        if term_signal.load(Ordering::Relaxed) {
            app.should_quit = true;
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
    use gameday::domain::League;
    use std::sync::atomic::Ordering;

    /// A panic on the poll thread must not drag the UI out of the alternate
    /// screen: the process is still running, and the board is still on screen.
    #[test]
    fn a_panic_off_the_ui_thread_does_not_restore_the_terminal() {
        super::install_panic_hook();
        let ui = std::thread::current().id();
        let before = super::RESTORE_CALLS.load(Ordering::Relaxed);
        let other = std::thread::spawn(move || {
            assert!(
                !super::should_restore(std::thread::current().id(), ui),
                "a background thread must not own the restore"
            );
            let _ = std::panic::catch_unwind(|| panic!("poll thread died"));
        });
        other
            .join()
            .expect("the spawned thread caught its own panic");
        assert_eq!(
            super::RESTORE_CALLS.load(Ordering::Relaxed),
            before,
            "the hook touched the terminal from a non-UI thread"
        );
        // The UI thread's own panic is the case that does restore.
        assert!(super::should_restore(ui, ui));
    }

    fn parsed(args: &[&str]) -> super::Args {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&owned).expect("valid args")
    }

    fn err(args: &[&str]) -> String {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&owned).expect_err("invalid args")
    }

    #[test]
    fn once_flags_parse_and_the_rest_need_once() {
        let a = parsed(&[
            "gameday", "--once", "--json", "--league", "nfl", "--league", "mlb", "--live", "--top",
            "3", "--color",
        ]);
        assert!(a.once && a.json && a.live && a.color);
        assert_eq!(a.leagues, vec![League::Nfl, League::Mlb]);
        assert_eq!(a.top, Some(3));
        assert!(!parsed(&["gameday", "--once"]).json);
        for bad in [
            vec!["gameday", "--json"],
            vec!["gameday", "--league", "nfl"],
            vec!["gameday", "--top", "3"],
            vec!["gameday", "--live"],
        ] {
            let owned: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            let e = parse_args(&owned).unwrap_err();
            assert!(e.contains("--once"), "{e}");
        }
        let e = err(&["gameday", "--once", "--league", "xfl"]);
        assert!(e.contains("xfl") && e.contains("nfl|cfb"), "{e}");
        let e = err(&["gameday", "--once", "--top", "many"]);
        assert!(e.contains("--top") && e.contains("many"), "{e}");
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
        let owned: Vec<String> = ["gameday", "dump", "--tick", "abc"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let err = parse_args(&owned).unwrap_err();
        assert!(err.contains("abc"), "{err}");
        let owned: Vec<String> = ["gameday", "dump", "--tick"]
            .iter()
            .map(|s| s.to_string())
            .collect();
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
        assert_eq!(
            parsed(&["gameday", "--config-dir", "/tmp/x"])
                .config_dir
                .as_deref(),
            Some(std::path::Path::new("/tmp/x"))
        );
        let owned: Vec<String> = ["gameday", "--nonsense"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let err = parse_args(&owned).unwrap_err();
        assert!(
            err.contains("--nonsense") && err.contains("--demo") && err.contains("--help"),
            "{err}"
        );
        let owned: Vec<String> = ["gameday", "nonsense"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(
            parse_args(&owned).is_err(),
            "bare unknown words are errors too"
        );
        let owned: Vec<String> = ["gameday", "--config-dir"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse_args(&owned)
            .unwrap_err()
            .contains("--config-dir expects a path"));
    }

    #[test]
    fn frame_parses_every_flag_and_defaults_the_rest() {
        use gameday::frame::{FrameView, Scenario, DEFAULT_SIZE};
        let a = parsed(&[
            "gameday",
            "frame",
            "--view",
            "tv",
            "--theme",
            "studio",
            "--size",
            "80x24",
            "--scenario",
            "redzone",
            "--tick",
            "40",
            "--out",
            "out/design/tv.png",
        ]);
        let f = a.frame.expect("frame spec");
        assert_eq!(f.view, FrameView::Tv);
        assert_eq!(f.theme, "studio");
        assert_eq!((f.cols, f.rows), (80, 24));
        assert_eq!(f.scenario, Scenario::Redzone);
        assert_eq!(f.tick, Some(40));
        assert_eq!(f.out, std::path::PathBuf::from("out/design/tv.png"));
        // Only --out is required; everything else has a default, and an
        // unflagged --tick means "the scenario's own beat", not tick 0.
        let f = parsed(&["gameday", "frame", "--out", "x.png"])
            .frame
            .expect("frame spec");
        assert_eq!(f.view, FrameView::Board);
        assert_eq!(f.scenario, Scenario::FullSlate);
        assert_eq!(f.theme, "broadcast");
        assert_eq!((f.cols, f.rows), DEFAULT_SIZE);
        assert_eq!(f.tick, None);
        assert!(parsed(&["gameday", "dump"]).frame.is_none());
    }

    #[test]
    fn frame_errors_name_the_bad_value_and_the_valid_set() {
        let err = |args: &[&str]| {
            let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            parse_args(&owned).unwrap_err()
        };
        let e = err(&["gameday", "frame", "--view", "jumbotron", "--out", "x.png"]);
        assert!(e.contains("jumbotron") && e.contains("theme-picker"), "{e}");
        let e = err(&[
            "gameday",
            "frame",
            "--scenario",
            "blowout",
            "--out",
            "x.png",
        ]);
        assert!(e.contains("blowout") && e.contains("nudge-resort"), "{e}");
        let e = err(&["gameday", "frame", "--size", "80", "--out", "x.png"]);
        assert!(e.contains("WxH") && e.contains("80"), "{e}");
        let e = err(&["gameday", "frame", "--size", "20x8", "--out", "x.png"]);
        assert!(e.contains("40x12"), "the floor is named: {e}");
        // --out is the one flag with no sensible default.
        let e = err(&["gameday", "frame", "--view", "tv"]);
        assert!(e.contains("--out"), "{e}");
        let e = err(&["gameday", "frame", "--view"]);
        assert!(e.contains("--view expects a value"), "{e}");
        // A frame flag outside `frame` is a typo, not a silent no-op.
        let e = err(&["gameday", "dump", "--view", "tv"]);
        assert!(e.contains("--view is a `gameday frame` flag"), "{e}");
    }

    #[test]
    fn help_text_lists_every_flag_and_the_dev_commands_under_their_own_heading() {
        for needle in [
            "--demo",
            "--help",
            "--version",
            "--config-dir",
            "dev:",
            "dump",
            "frame",
            "--view",
            "--scenario",
            "--out",
            "probe",
            "--tick",
            "~/.config/gameday",
        ] {
            assert!(super::HELP.contains(needle), "HELP missing {needle}");
        }
        assert!(
            super::HELP.lines().count() < 30,
            "help must fit a small terminal"
        );
    }
}
