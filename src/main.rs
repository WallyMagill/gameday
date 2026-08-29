use std::io::stdout;
use std::sync::mpsc;
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

struct Args {
    demo: bool,
    dump: bool,
}

fn parse_args(args: &[String]) -> Args {
    Args {
        demo: args.iter().any(|a| a == "--demo"),
        dump: args.iter().any(|a| a == "dump" || a == "--dump"),
    }
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
    let args = parse_args(&std::env::args().collect::<Vec<_>>());

    if args.dump {
        return gameday::dump::run(std::path::Path::new("out"));
    }

    if args.demo {
        // Demo state lives in a scratch dir so it never touches real pins/config.
        let dir = std::env::temp_dir().join(format!("gameday-demo-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        return run_ui(gameday::dump::demo_app(dir), None);
    }

    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gameday");
    std::fs::create_dir_all(dir.join("cache"))?;
    let config = Config::load_from(&dir).unwrap_or_else(|_| Config::default_nfl());
    let pins = load_pins(&dir).unwrap_or_default();
    let enabled_tabs = config.enabled_tabs.clone();
    let app = App::new(config, pins, dir.clone());

    let provider = EspnProvider::new(dir.join("cache"));
    let (tx, rx) = mpsc::channel::<Msg>();
    let tx_plan = tx.clone();
    thread::spawn(move || poll_loop(provider, tx_plan, enabled_tabs));
    run_ui(app, Some(rx))
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

fn poll_loop(provider: EspnProvider, tx: mpsc::Sender<Msg>, leagues: Vec<League>) {
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
        if last_board.elapsed() >= every {
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

fn run_ui(mut app: App, rx: Option<mpsc::Receiver<Msg>>) -> std::io::Result<()> {
    enable_raw_mode()?;
    let _restore = RestoreTerminal;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut last_err: Option<std::io::Error> = None;
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
            }
        }
        if let Err(e) = terminal.draw(|f| app.draw(f)) {
            last_err = Some(e);
            break;
        }
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.on_key(k.code);
                }
            }
        }
        if app.refresh_now {
            app.refresh_now = false;
            // next poll_loop tick is soon; no extra channel needed in v1
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
    use super::{merge_live_ids, parse_args};
    use gameday::domain::{Game, League, Status, Team};

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

    #[test]
    fn demo_flag() {
        assert!(parse_args(&["gameday".into(), "--demo".into()]).demo);
        assert!(!parse_args(&["gameday".into()]).demo);
        assert!(parse_args(&["gameday".into(), "dump".into()]).dump);
        assert!(!parse_args(&["gameday".into(), "--demo".into()]).dump);
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
