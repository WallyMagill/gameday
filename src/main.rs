use std::io::stdout;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use gameday::app::App;
use gameday::config::{load_pins, Config};
use gameday::domain::*;
use gameday::provider::espn::EspnProvider;
use gameday::provider::memory::MemoryProvider;
use gameday::provider::SportsProvider;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

struct Args { demo: bool }

fn parse_args(args: &[String]) -> Args {
    Args { demo: args.iter().any(|a| a == "--demo") }
}

enum Msg {
    Boards { league: League, games: Vec<Game>, stale: bool },
    Summary { id: String, summary: Summary },
}

fn demo_games() -> Vec<Game> {
    let t = |abbr: &str, c: [u8; 3]| Team {
        id: abbr.into(), abbr: abbr.into(), name: abbr.into(),
        color: c, alt_color: [180, 180, 180],
        logo_key: format!("nfl/{}", abbr.to_lowercase()),
    };
    vec![
        Game {
            id: "d1".into(), league: League::Nfl,
            away: t("KC", [227, 24, 55]), home: t("TB", [213, 10, 10]),
            away_score: 27, home_score: 24, status: Status::Live,
            period: "Q4".into(), clock: "1:27".into(),
            situation: Some(Situation { down_distance: "1st & Goal".into(), possession: Some("KC".into()), ball_on: Some("TB 3".into()) }),
            last_plays: vec![Play { clock: "1:27".into(), text: "Mahomes pass to Kelce for 3 yards".into(), scoring: false }],
            meter: Some(Meter::RedZone { yards_to_goal: 3 }),
            start_time: None, broadcast: Some("CBS".into()),
        },
        Game {
            id: "d2".into(), league: League::Nfl,
            away: t("PHI", [0, 76, 84]), home: t("DAL", [0, 34, 68]),
            away_score: 14, home_score: 14, status: Status::Live,
            period: "Q2".into(), clock: "2:03".into(),
            situation: Some(Situation { down_distance: "3rd & 4".into(), possession: Some("PHI".into()), ball_on: Some("DAL 28".into()) }),
            last_plays: vec![Play { clock: "2:10".into(), text: "Hurts incomplete to Brown".into(), scoring: false }],
            meter: None, start_time: None, broadcast: Some("FOX".into()),
        },
        Game {
            id: "d3".into(), league: League::Nfl,
            away: t("SF", [170, 0, 0]), home: t("SEA", [105, 190, 40]),
            away_score: 0, home_score: 0, status: Status::Pre,
            period: "".into(), clock: "".into(), situation: None, last_plays: vec![],
            meter: None, start_time: Some("8:20 PM".into()), broadcast: Some("NBC".into()),
        },
    ]
}

fn main() -> std::io::Result<()> {
    let args = parse_args(&std::env::args().collect::<Vec<_>>());
    let dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("gameday");
    std::fs::create_dir_all(dir.join("cache"))?;
    let config = Config::load_from(&dir).unwrap_or_else(|_| Config::default_nfl());
    let pins = load_pins(&dir).unwrap_or_default();
    let mut app = App::new(config, pins, dir.clone());

    if args.demo {
        let mut mem = MemoryProvider::new();
        mem.insert_board(League::Nfl, demo_games());
        let games = mem.scoreboard(League::Nfl).unwrap();
        app.apply_boards(League::Nfl, games, false);
        return run_ui(app, None);
    }

    let provider = EspnProvider::new(dir.join("cache"));
    let (tx, rx) = mpsc::channel::<Msg>();
    let tx_plan = tx.clone();
    thread::spawn(move || poll_loop(provider, tx_plan));
    run_ui(app, Some(rx))
}

fn poll_loop(provider: EspnProvider, tx: mpsc::Sender<Msg>) {
    let mut last_board = Instant::now() - Duration::from_secs(999);
    let mut last_sum = Instant::now() - Duration::from_secs(999);
    let mut attempt = 0u32;
    // The UI thread does not send plans; poll NFL + whatever we last knew.
    // Simpler v1: always poll League::Nfl scoreboard on the interval, and
    // summaries for ids last seen live. The UI applies whatever arrives.
    let mut live_ids: Vec<String> = vec![];
    loop {
        let every = if live_ids.is_empty() { Duration::from_secs(60) } else { Duration::from_secs(20) };
        if last_board.elapsed() >= every {
            match provider.scoreboard(League::Nfl) {
                Ok(games) => {
                    live_ids = games.iter().filter(|g| g.status == Status::Live).map(|g| g.id.clone()).collect();
                    attempt = 0;
                    let _ = tx.send(Msg::Boards { league: League::Nfl, games, stale: false });
                }
                Err(_) => {
                    attempt = attempt.saturating_add(1);
                    thread::sleep(Duration::from_secs(gameday::provider::espn::backoff_secs(attempt)));
                }
            }
            last_board = Instant::now();
        }
        if last_sum.elapsed() >= Duration::from_secs(15) {
            for id in live_ids.iter().take(4) {
                if let Ok(s) = provider.summary(League::Nfl, id) {
                    let _ = tx.send(Msg::Summary { id: id.clone(), summary: s });
                }
            }
            last_sum = Instant::now();
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn run_ui(mut app: App, rx: Option<mpsc::Receiver<Msg>>) -> std::io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut last_err: Option<std::io::Error> = None;
    'ui: loop {
        if let Some(rx) = &rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    Msg::Boards { league, games, stale } => app.apply_boards(league, games, stale),
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
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    #[test]
    fn demo_flag() {
        assert!(parse_args(&["gameday".into(), "--demo".into()]).demo);
        assert!(!parse_args(&["gameday".into()]).demo);
    }
}
