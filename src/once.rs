//! `gameday --once`: fetch, rank, print, exit. The same config, pins,
//! provider, cache and ranking as the board; no terminal, no poll thread,
//! no alternate screen. Text is the board's own tier rows rendered into a
//! buffer; JSON is a pinned schema for scripts and status bars.

use crate::app::{App, Tab};
use crate::board;
use crate::board::rows;
use crate::config::{Config, Pin};
use crate::domain::{Game, League, Status};
use crate::dump;
use crate::provider::espn::EspnProvider;
use crate::provider::ProviderError;
use crate::rank;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::thread;
use time::{OffsetDateTime, UtcOffset};

/// The board's own width when stdout isn't a terminal to measure (a pipe, a
/// cron job, a status-bar script): 100 columns holds the widest tier row
/// (`CLOCK_X` + `CLOCK_W` + the league tag + a headline) with room to spare —
/// the same neighborhood `dump::DUMP_COLS` (120) was measured against, just
/// narrow enough that a script piping to a normal terminal window never wraps.
pub const ONCE_WIDTH: u16 = 100;

pub struct Opts {
    pub json: bool,
    pub leagues: Vec<League>,
    pub live: bool,
    pub top: Option<usize>,
    pub color: bool,
    pub width: u16,
}

/// One league's scoreboard answer, boards or errors: `fetch`/`run`'s shared
/// currency, named once so the parallel-fetch plumbing below reads as plain
/// signatures rather than a wall of nested tuples.
type ScoreboardResult = Result<(Vec<Game>, bool), ProviderError>;
type Boards = Vec<(League, Vec<Game>, bool)>;
type Errors = Vec<(League, String)>;

/// What `--once` actually fetches with: the live board's `EspnProvider`
/// (cache-first, `SCOREBOARD_LIVE`-gated) or a test double. Kept separate
/// from `SportsProvider` — that trait's `scoreboard` always hits the
/// network-or-fallback path; `--once` specifically wants the cache-gated one.
pub trait OnceSource {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError>;
}

impl OnceSource for EspnProvider {
    fn once_scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard_cached_within(league, crate::poll::SCOREBOARD_LIVE)
    }
}

/// `run`'s result: the process exit code, the stdout it would print (empty
/// means print nothing — see `main`), and the stderr lines (each printed
/// `gameday: {line}`). Returned rather than printed/exited directly so a
/// test can inspect all three without touching a real process.
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: Vec<String>,
}

/// One thread per league, joined together: the slowest league bounds the
/// whole call at one `espn::HTTP_TIMEOUT` (10s) instead of nine in a row —
/// nine sequential worst-case timeouts would be a 90s hang for a command
/// billed as "fetch once and exit".
fn fetch_with(
    leagues: &[League],
    f: impl Fn(League) -> ScoreboardResult + Sync,
) -> (Boards, Errors) {
    let results: Vec<(League, ScoreboardResult)> = thread::scope(|s| {
        let handles: Vec<_> = leagues
            .iter()
            .map(|&l| {
                let f = &f;
                s.spawn(move || (l, f(l)))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a fetch thread panicked"))
            .collect()
    });
    let mut boards = Vec::new();
    let mut errors = Vec::new();
    for (league, r) in results {
        match r {
            Ok((games, stale)) => boards.push((league, games, stale)),
            Err(e) => errors.push((league, e.short())),
        }
    }
    (boards, errors)
}

/// Every league in parallel; each answer is (league, games, stale). Errors
/// are the provider's short text.
pub fn fetch(source: &(impl OnceSource + Sync), leagues: &[League]) -> (Boards, Errors) {
    fetch_with(leagues, |l| source.once_scoreboard(l))
}

/// The same `App` the live board runs: one `apply_boards` per league (the
/// wave 3 first-sighting rule ranks a stale first apply, so IN PLAY comes
/// out sorted even when every league answered from a cold cache), the clock
/// frozen at `now` so a capture (or a script that diffs two runs) never
/// races the wall clock.
pub fn build_app(
    config: Config,
    pins: Vec<Pin>,
    dir: PathBuf,
    offset: UtcOffset,
    now: OffsetDateTime,
    boards: Vec<(League, Vec<Game>, bool)>,
) -> App {
    let mut app = App::new(config, pins, dir, offset);
    app.now_override = Some(now);
    for (league, games, stale) in boards {
        app.apply_boards(league, games, stale);
    }
    app.tab = Tab::Home;
    app
}

/// The board's own tier rows, rendered offscreen into a `TestBackend` and
/// serialized as plain text (or ANSI with `--color`) — the same
/// `rows::draw_tier2`/`draw_tier3` the live board draws, so a script reading
/// `--once` output sees exactly the grid a person watching the board would.
/// `stale` (any league served from cache after a failed fetch) prints a
/// first line naming it — a script piping this text has no `stale` JSON
/// field to check, so the text form has to say so itself.
pub fn render_text(app: &mut App, opts: &Opts, stale: bool) -> String {
    let d = app.derive();
    let now = app.now();
    let sort = app.config.sort;
    let pins = app.pins.clone();
    let mut sections: Vec<(&'static str, Vec<Game>)> = vec![
        ("MY GAMES", d.my_games),
        ("IN PLAY", d.in_play),
        ("FINAL", d.finals),
        ("LATER", d.later),
    ];
    let mixed = d.mixed;

    if opts.live {
        for (_, games) in &mut sections {
            games.retain(|g| g.status == Status::Live);
        }
    }
    if let Some(top) = opts.top {
        // A running budget across every section, in board order: a section
        // that would overrun it is truncated, and everything after it is
        // dropped whole (empty, so the section-empty check below drops the
        // rule too).
        let mut budget = top;
        for (_, games) in &mut sections {
            if games.len() > budget {
                games.truncate(budget);
            }
            budget = budget.saturating_sub(games.len());
        }
    }
    sections.retain(|(_, games)| !games.is_empty());

    let rows_needed: u16 = sections
        .iter()
        .map(|(_, games)| 1 + games.len() as u16)
        .sum();

    // --color paints in the user's own theme — the one their live board
    // actually uses — falling back to broadcast for an unloaded name (a
    // stale config, a deleted user theme file). Plain text never reads
    // color, so any loaded theme renders identical characters: the goldens
    // (colorless) don't move.
    let theme_name = if crate::theme::names().iter().any(|n| n == &app.config.theme) {
        app.config.theme.clone()
    } else {
        "broadcast".to_string()
    };
    let buf = dump::with_theme(&theme_name, || {
        let mut term = Terminal::new(TestBackend::new(opts.width, rows_needed.max(1)))
            .expect("TestBackend is infallible");
        term.draw(|f| {
            let mut y = 0u16;
            for (label, games) in &sections {
                let caption = if *label == "IN PLAY" {
                    format!("SORTED BY {}", board::sort_phrase(sort))
                } else {
                    String::new()
                };
                board::draw_rule(
                    f,
                    Rect {
                        x: 0,
                        y,
                        width: opts.width,
                        height: 1,
                    },
                    label,
                    &caption,
                );
                y += 1;
                for game in games {
                    let w = rank::watchability(game, now);
                    let ctx = rows::RowCtx {
                        hot: w.hot,
                        chip: w.chip,
                        nudge: None,
                        selected: false,
                        pinned: pins.iter().any(|p| p.game_id == game.id),
                        league_tag: mixed,
                        now,
                        leaders_line: None,
                    };
                    let rect = Rect {
                        x: 0,
                        y,
                        width: opts.width,
                        height: 1,
                    };
                    if game.status == Status::Live {
                        rows::draw_tier2(f, rect, game, &ctx);
                    } else {
                        rows::draw_tier3(f, rect, game, &ctx);
                    }
                    y += 1;
                }
            }
        })
        .expect("TestBackend draw is infallible");
        term.backend().buffer().clone()
    });

    let body = if opts.color {
        dump::buffer_to_ansi(&buf)
    } else {
        dump::buffer_to_text(&buf)
    };
    if stale {
        format!("STALE · cached scores, the network failed\n{body}")
    } else {
        body
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn team_json(team: &crate::domain::Team, score: u16) -> serde_json::Value {
    serde_json::json!({
        "abbr": team.abbr,
        "name": team.name,
        "score": score,
        "record": team.record,
        "rank": team.rank,
    })
}

/// The pinned JSON schema: `derive().selection` (board order) filtered the
/// same way `render_text` narrows its rows, one object per game.
pub fn render_json(app: &mut App, opts: &Opts, stale: bool) -> serde_json::Value {
    let d = app.derive();
    let now = app.now();
    let mut games = d.selection;
    if opts.live {
        games.retain(|g| g.status == Status::Live);
    }
    if let Some(top) = opts.top {
        games.truncate(top);
    }
    let pins = &app.pins;
    let arr: Vec<serde_json::Value> = games
        .iter()
        .map(|g| {
            let w = rank::watchability(g, now);
            let status = match g.status {
                Status::Live => "live",
                Status::Pre => "pre",
                Status::Final => "final",
            };
            serde_json::json!({
                "league": g.league.slug(),
                "id": g.id,
                "status": status,
                "period": g.period,
                "clock": g.clock,
                "start": g.start.map(rfc3339),
                "away": team_json(&g.away, g.away_score),
                "home": team_json(&g.home, g.home_score),
                "watch": { "score": w.score, "chip": w.chip, "why": w.why },
                "situation": rows::situation_summary(g),
                "last_play": g.last_plays.first().map(|p| p.text.clone()),
                "pinned": pins.iter().any(|p| p.game_id == g.id),
            })
        })
        .collect();
    serde_json::json!({
        "generated_at": rfc3339(now),
        "stale": stale,
        "games": arr,
    })
}

/// Fetch every requested league (cache-first: a scoreboard younger than
/// `poll::SCOREBOARD_LIVE` never touches the network), render the ranked
/// board. Returns the outcome rather than printing/exiting itself — `main`
/// does that (and so does a test). `code` is 0 once anything was fetched, 1
/// when nothing was and nothing was cached to fall back on.
///
/// No `--league`: fetches `config.enabled_tabs`, same as the live board —
/// never the whole `League::ALL`, which would poll a league the user
/// disabled. `--league` means it: it both narrows the fetch AND becomes the
/// enabled set the ranked board renders, so a requested league that isn't
/// normally enabled still shows up.
pub fn run<S: OnceSource + Sync>(
    mut config: Config,
    pins: Vec<Pin>,
    dir: PathBuf,
    offset: UtcOffset,
    source: &S,
    opts: Opts,
) -> Outcome {
    let leagues: Vec<League> = if opts.leagues.is_empty() {
        config.enabled_tabs.clone()
    } else {
        config.enabled_tabs = opts.leagues.clone();
        opts.leagues.clone()
    };
    let (boards, errors) = fetch(source, &leagues);
    if boards.is_empty() {
        let line = errors
            .first()
            .map(|(_, e)| e.clone())
            .unwrap_or_else(|| "nothing to fetch".to_string());
        return Outcome {
            code: 1,
            stdout: String::new(),
            stderr: vec![line],
        };
    }
    let stderr: Vec<String> = errors.iter().map(|(_, e)| e.clone()).collect();
    let stale = boards.iter().any(|(_, _, stale)| *stale);
    let now = OffsetDateTime::now_utc().to_offset(offset);
    let mut app = build_app(config, pins, dir, offset, now, boards);
    // --json is for scripts: never colored. --color only spends escapes
    // when stdout is actually a terminal to read them.
    let color = opts.color && !opts.json && std::io::stdout().is_terminal();
    let opts = Opts { color, ..opts };
    let stdout = if opts.json {
        serde_json::to_string_pretty(&render_json(&mut app, &opts, stale)).unwrap_or_default()
    } else {
        render_text(&mut app, &opts, stale)
    };
    Outcome {
        code: 0,
        stdout,
        stderr,
    }
}
