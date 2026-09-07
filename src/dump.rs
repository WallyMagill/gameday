//! `gameday dump`: render every surface of the demo board offscreen and write
//! a fixed-name gallery into out/ — HTML + ANSI always, plus a PNG per capture
//! when headless Chrome is available. This is the visual iteration loop:
//! compare out/board-broadcast.png to the reference image. The gallery:
//!
//!   board-broadcast/-studio/-gruvbox/-daygame — the ranked board, one per
//!       BUILT-IN theme (selected programmatically, not via env). Four
//!       identities, not eleven palettes.
//!   board-narrow  — the same board at 80x24
//!   board-sixty   — and at 60x40: the tall, narrow end of the ladder
//!   tv            — `:tv`, the jumbotron hero and the ALSO LIVE strip
//!   cut-full      — the scoring takeover
//!   cut-band      — the quiet two-row band a score you don't follow gets
//!   zoom          — the zoomed game: hero, linescore, matchup line, feed
//!   plays-feed    — the global scoring feed (:plays)
//!   standings     — the NFL standings table from the committed fixture
//!   config        — the in-app config editor (:config)
//!   filter        — the NFL tab narrowed by a committed /kc filter
//!   theme-picker  — the `:theme` picker panel over the home board
//!   help          — the '?' overlay over the dimmed board
//!   home-live     — the first-boot Home frame, every live demo game on it
//!   offline       — no board at all, a named fetch failure and its retry
//!   stale         — a board served from cache, backdated to read STALE 4m
//!   config-error  — an unparseable config.toml, named line and valid values
//!   nudge-seq-1/-2/-3 — three frames bracketing `sim::NUDGE_TICK`, the one
//!       scripted beat that re-sorts the live list: before, the tick the
//!       bases load, and the tick after — the `↑n` gutter appearing and
//!       holding while the row does not move a cell.
//!
//! Every capture is the sim state at a fixed tick (`--tick N`, default 0;
//! the `nudge-seq` stems pin their own), so repeated runs are
//! pixel-deterministic. No timestamps in file names.

use crate::app::{App, Tab};
use crate::demo;
use crate::domain::League;
use crate::provider::memory::MemoryProvider;
use crate::provider::{map, SportsProvider};
use crate::theme;
use crate::views::{View, ZoomTab};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DUMP_COLS: u16 = 120;
pub const DUMP_ROWS: u16 = 36;
/// Whole-gallery runtime budget. Rendering the buffers is milliseconds; the
/// cost is Chrome. At [`SHOT_BATCH`] = 4 the 23-page gallery is 6 batches of
/// a measured ~16 s, so ~100 s is the expected run and 150 s is the line past
/// which something is wrong. Overruns print actual vs budget naming the phase.
const BUDGET: Duration = Duration::from_secs(150);

/// One gallery capture: which surface, at what size, in which theme.
pub struct Variant {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    /// A built-in theme name (`theme::BUILTIN_NAMES`).
    pub theme: &'static str,
    /// Pins this capture to one sim tick regardless of `--tick`. Only the
    /// `nudge-seq` frames use it: they are a sequence, and a sequence whose
    /// frames all moved with a flag would stop being one.
    pub tick: Option<u64>,
    setup: Setup,
}

/// What a capture does to the demo app before it is drawn. Fallible because
/// [`frame`](crate::frame) runs these same functions against scenarios that
/// may have removed the game a view is about — in the gallery, where the
/// slate is always the full demo slate, none of them ever fails.
pub type Setup = fn(&mut App) -> Result<(), String>;

/// `board-<theme>` stems, one per built-in, in `BUILTIN_NAMES` order. Static
/// strings because stems are the fixed-name contract other tasks read; a test
/// pins this list to `BUILTIN_NAMES` so a new theme can't ship without a board.
pub const BOARD_STEMS: [(&str, &str); 4] = [
    ("broadcast", "board-broadcast"),
    ("studio", "board-studio"),
    ("gruvbox", "board-gruvbox"),
    ("daygame", "board-daygame"),
];

/// Every capture's "put the app in this state" step, one function per
/// surface. Module-level and public because `gameday frame` renders the same
/// surfaces on demand: the gallery and a one-off design frame run the SAME
/// setup, so a `frame --view tv` and the `tv` stem can never drift apart.
pub mod setup {
    use super::{demo, map, App, League, MemoryProvider, SportsProvider, Tab, View, ZoomTab};
    use std::time::{Duration, Instant};

    pub fn home(_: &mut App) -> Result<(), String> {
        Ok(())
    }
    pub fn tv(app: &mut App) -> Result<(), String> {
        app.open_tv();
        Ok(())
    }
    // The zoomed game is the baseball one on purpose: MLB is the only demo
    // sport whose zoom exercises all three rows under the hero at once — the
    // linescore with H/E, the `P: … AB: … DUE UP` matchup line, and
    // an inning-stamped feed (`[B7]`).
    pub fn zoom(app: &mut App) -> Result<(), String> {
        let mut p = MemoryProvider::new();
        p.stats.insert("nfl-live".into(), demo::demo_stats());
        let _ = p.stats(League::Nfl, "nfl-live");
        app.view = View::Zoom {
            game_id: "mlb-live".into(),
            tab: ZoomTab::Overview,
        };
        Ok(())
    }
    /// The newest scoring play of a demo game, for the cut captures. Missing
    /// is only possible under a `frame` scenario that filtered the game away,
    /// and the error names the game so the caller knows which one to keep.
    fn scoring_play(app: &App, id: &str) -> Result<crate::domain::Play, String> {
        app.boards
            .values()
            .flatten()
            .find(|g| g.id == id)
            .and_then(|g| g.scoring_plays.last().or_else(|| g.last_plays.first()))
            .cloned()
            .ok_or_else(|| format!("no game {id} with a scoring play is on this board"))
    }
    // The takeover: a game you follow scored. `full = true` is the caller's
    // judgment in the live app (pinned/favorited/TV) — here it is stated
    // outright, because the capture's subject IS the full size.
    pub fn cut_full(app: &mut App) -> Result<(), String> {
        let play = scoring_play(app, "nfl-live")?;
        app.cuts.fire("nfl-live", &play, true, app.tick);
        Ok(())
    }
    // The band: someone else scored. Same formatter, two rows, board intact.
    pub fn cut_band(app: &mut App) -> Result<(), String> {
        let play = scoring_play(app, "nhl-live")?;
        app.cuts.fire("nhl-live", &play, false, app.tick);
        Ok(())
    }
    pub fn help(app: &mut App) -> Result<(), String> {
        app.help_open = true;
        Ok(())
    }
    pub fn plays_feed(app: &mut App) -> Result<(), String> {
        app.view = View::PlaysFeed;
        Ok(())
    }
    pub fn standings(app: &mut App) -> Result<(), String> {
        let mut p = MemoryProvider::new();
        p.standings.insert(
            League::Nfl,
            map::map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json"))
                .expect("nfl_standings.json fixture must map"),
        );
        let (table, _) = p.standings(League::Nfl).expect("seeded standings");
        app.merge_standings(table);
        app.view = View::Standings(League::Nfl);
        Ok(())
    }
    pub fn config(app: &mut App) -> Result<(), String> {
        // A seeded favorite so the FAVORITES section shows a real row.
        app.config.favorites.push(crate::config::Favorite {
            league: League::Nfl,
            team_abbr: "KC".into(),
        });
        app.view = View::ConfigView;
        Ok(())
    }
    pub fn filter(app: &mut App) -> Result<(), String> {
        app.tab = Tab::League(League::Nfl);
        app.filter = Some("kc".into());
        Ok(())
    }
    pub fn theme_picker(app: &mut App) -> Result<(), String> {
        app.open_theme_picker();
        Ok(())
    }
    // Home now shows every live demo game, so the plain Home tab IS the
    // first-boot frame — the capture other tasks compare the board against.
    pub fn home_live(app: &mut App) -> Result<(), String> {
        app.tab = Tab::Home;
        Ok(())
    }
    // Nothing on the board and a named failure: the empty-state message, the
    // OFFLINE chip with its retry, and the footer line all at once.
    pub fn offline(app: &mut App) -> Result<(), String> {
        app.boards.clear();
        // The demo seeds its boards through `apply_boards`, which records a
        // fresh OK — and a fresh OK outranks the failure behind it, so the
        // chip would still say Live. Clearing the boards means clearing the
        // apply that filled them: this is the boot that never got one.
        app.net = crate::app::net::NetStatus::default();
        app.note_failure(
            League::Nfl,
            "ESPN unreachable nfl scoreboard".into(),
            Some(Duration::from_secs(40)),
        );
        Ok(())
    }
    // `NetStatus` measures age against `Instant`, so staleness is seeded by
    // backdating the last OK apply four minutes — the chip reads "STALE 4m".
    pub fn stale(app: &mut App) -> Result<(), String> {
        app.net
            .ok(Instant::now() - Duration::from_secs(4 * 60), true);
        Ok(())
    }
    // A config.toml that doesn't parse: the banner names the line and the
    // valid values, and `set_config_error` writes the footer itself.
    pub fn config_error(app: &mut App) -> Result<(), String> {
        app.set_config_error(Some(
            "config.toml:7: unknown variant `NFLL` — valid leagues: nfl|cfb|cbb|nba|wnba|nhl|mlb|epl|mls".into(),
        ));
        Ok(())
    }
}

/// The fixed gallery, in write order. Stems are stable file names — other
/// tasks (themes, animation frames, tile polish, keyboard) verify against
/// these exact paths, so renames here are breaking.
pub fn gallery() -> Vec<Variant> {
    use setup::*;
    let full = |stem, theme, setup| Variant {
        stem,
        cols: DUMP_COLS,
        rows: DUMP_ROWS,
        theme,
        tick: None,
        setup,
    };
    let sized = |stem, cols, rows, setup| Variant {
        stem,
        cols,
        rows,
        theme: "broadcast",
        tick: None,
        setup,
    };
    let at_tick = |stem, tick| Variant {
        stem,
        cols: DUMP_COLS,
        rows: DUMP_ROWS,
        theme: "broadcast",
        tick: Some(tick),
        setup: home as Setup,
    };
    let mut out: Vec<Variant> = BOARD_STEMS
        .iter()
        .map(|(name, stem)| full(*stem, *name, home as Setup))
        .collect();
    out.extend([
        sized("board-narrow", 80, 24, home as Setup),
        sized("board-sixty", 60, 40, home),
        full("tv", "broadcast", tv),
        full("cut-full", "broadcast", cut_full),
        full("cut-band", "broadcast", cut_band),
        full("zoom", "broadcast", zoom),
        full("plays-feed", "broadcast", plays_feed),
        full("standings", "broadcast", standings),
        full("config", "broadcast", config),
        full("filter", "broadcast", filter),
        full("theme-picker", "broadcast", theme_picker),
        full("help", "broadcast", help),
        full("home-live", "broadcast", home_live),
        full("offline", "broadcast", offline),
        full("stale", "broadcast", stale),
        full("config-error", "broadcast", config_error),
        at_tick("nudge-seq-1", crate::sim::NUDGE_TICK - 1),
        at_tick("nudge-seq-2", crate::sim::NUDGE_TICK),
        at_tick("nudge-seq-3", crate::sim::NUDGE_TICK + 1),
    ]);
    out
}

/// Run `f` with `name` as the current theme, restoring the caller's theme
/// after — variants can't leak palettes into each other (or into tests on
/// the same thread). Every gallery variant names a built-in, so a miss is a
/// bug. (Render-gate captures once installed `theme::CANDIDATE_NAMES`
/// entries here for one capture and uninstalled them after; those stems were
/// retired with the gates they served, so this only sets a loaded theme now.)
pub fn with_theme<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let prev = theme::current_name();
    theme::set_current(name).unwrap_or_else(|e| panic!("dump theme: {e}"));
    let out = f();
    theme::set_current(&prev).expect("the previous theme is still loaded");
    out
}

/// Sim ticks replayed before the captured one. Two would be enough for a
/// score flash; three is what a *nudge* needs, because the ↑n gutter is the
/// difference between two consecutive applies and the frame AFTER the re-sort
/// still has to show it (`OrderState` holds an arrow for 10 s). Replaying is
/// cheap — `boards_at` is a few dozen pure steps — so the number is set by
/// what the captures must be able to say, not by cost.
const REPLAY_TICKS: u64 = 3;

/// Demo app at simulation tick `tick` (0 = the seed board in demo.rs).
/// Advancing is pure — N scripted steps, no wall clock — so `dump --tick N`
/// always captures the same frame. The [`REPLAY_TICKS`] ticks before it are
/// applied first, each at its own `app.tick`, so a score that changes AT
/// `tick` is caught mid-flash and a re-sort a tick or two back still shows
/// its arrows — exactly like the live loop would (`--tick 15` captures the KC
/// TD flash; `--tick 41` still shows the ↑1 earned at 40).
pub fn demo_app(config_dir: PathBuf, tick: u64) -> App {
    // The demo data is Eastern, so captures render its clocks in Eastern too —
    // never the capturing machine's zone, which would make dumps unstable.
    let mut app = App::new(
        demo::demo_config(),
        demo::demo_pins(),
        config_dir,
        time::UtcOffset::from_hms(-4, 0, 0).expect("-04:00 is a valid offset"),
    );
    // The captures' wall clock is frozen too: a dump names a fixed instant so
    // "TODAY 8:20 PM" can't turn into "SEP 13 8:20 PM" between runs.
    app.now_override = Some(time::macros::datetime!(2026-08-31 21:30:01 -4));
    let mut sim = crate::sim::Simulator::new();
    for t in tick.saturating_sub(REPLAY_TICKS)..=tick {
        sim.advance_to(t);
        app.tick = t;
        for (league, games) in sim.boards().clone() {
            app.apply_boards(league, games, false);
        }
    }
    app
}

pub fn render_demo_buffer(cols: u16, rows: u16, tick: u64) -> std::io::Result<Buffer> {
    let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut app = demo_app(dir, tick);
    // TestBackend's Error is Infallible, so this unwrap cannot fire.
    let mut term = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    Ok(term.backend().buffer().clone())
}

/// Render one gallery variant. Sets the variant's theme for the duration of
/// the render and restores the caller's theme after, so variants can't leak
/// palettes into each other (or into tests on the same thread).
pub fn render_variant(v: &Variant, tick: u64) -> std::io::Result<Buffer> {
    let tick = v.tick.unwrap_or(tick);
    with_theme(v.theme, || {
        let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut app = demo_app(dir, tick);
        (v.setup)(&mut app).unwrap_or_else(|e| panic!("dump variant {}: {e}", v.stem));
        // TestBackend's Error is Infallible, so this unwrap cannot fire.
        let mut term = Terminal::new(TestBackend::new(v.cols, v.rows)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        Ok(term.backend().buffer().clone())
    })
}

/// One rendered page ready for the shared write/screenshot/verify pipeline.
/// Owned strings, not `&'static str`: the gallery's stems and themes are
/// compile-time constants, but `gameday frame` names both at the command line.
pub struct Page {
    pub stem: String,
    pub cols: u16,
    pub rows: u16,
    /// The theme name the page was rendered under (page bg/fg).
    pub theme: String,
    pub buf: Buffer,
}

pub fn run(out_dir: &Path, tick: u64) -> std::io::Result<()> {
    let start = Instant::now();
    std::fs::create_dir_all(out_dir)?;
    let variants = gallery();
    let pages = variants
        .iter()
        .map(|v| {
            Ok(Page {
                stem: v.stem.to_string(),
                cols: v.cols,
                rows: v.rows,
                theme: v.theme.to_string(),
                buf: render_variant(v, tick)?,
            })
        })
        .collect::<std::io::Result<Vec<Page>>>()?;
    write_pages(out_dir, &pages)?;
    let render_done = start.elapsed();
    let chrome = screenshot_pages(out_dir, &pages);
    // The gallery is a verification artifact for other tasks: fail loudly if
    // any promised file is missing or empty instead of exiting green.
    verify_pages(out_dir, &pages, chrome)?;
    let elapsed = start.elapsed();
    if elapsed > BUDGET {
        eprintln!(
            "gameday dump: over budget — {:.1}s actual vs {}s budget \
             (render+write {:.1}s, screenshots {:.1}s across {} captures)",
            elapsed.as_secs_f32(),
            BUDGET.as_secs(),
            render_done.as_secs_f32(),
            (elapsed - render_done).as_secs_f32(),
            variants.len(),
        );
    }
    Ok(())
}

/// Phase 1 (one process, cheap): write HTML + ANSI for every page.
pub fn write_pages(out_dir: &Path, pages: &[Page]) -> std::io::Result<()> {
    for p in pages {
        // buffer_to_html reads theme::current() for the page bg/fg, so the
        // serialization happens under the page's theme.
        let html_path = out_dir.join(format!("{}.html", p.stem));
        with_theme(&p.theme, || {
            std::fs::write(&html_path, buffer_to_html(&p.buf)).and_then(|()| {
                std::fs::write(
                    out_dir.join(format!("{}.ansi", p.stem)),
                    buffer_to_ansi(&p.buf),
                )
            })
        })?;
        eprintln!("wrote {}", html_path.display());
    }
    Ok(())
}

/// Phase 2: one headless-Chrome instance per PNG, in batches of
/// [`SHOT_BATCH`] — Chrome startup dominates the runtime, so serial capture
/// would be glacial, but past a handful of cold instances they contend for
/// each other and none of them finish. Returns whether Chrome was available
/// (and thus whether PNGs should be expected).
pub fn screenshot_pages(out_dir: &Path, pages: &[Page]) -> bool {
    let chrome = Path::new(CHROME).exists();
    if !chrome {
        eprintln!("png skipped: Chrome not found at {CHROME} (open the .html files instead)");
        return false;
    }
    for batch in pages.chunks(SHOT_BATCH) {
        let mut shots: Vec<Shot> = batch.iter().map(|p| Shot::spawn(out_dir, p)).collect();
        wait_for_screenshots(&mut shots);
        for shot in shots {
            if shot.png_done {
                eprintln!("wrote {}", shot.png.display());
            } else {
                eprintln!(
                    "png skipped for {}: chrome produced no stable PNG within {}s (open {stem}.html instead)",
                    shot.stem,
                    SHOT_DEADLINE.as_secs(),
                    stem = shot.stem
                );
            }
        }
    }
    true
}

/// Every promised file must exist and be non-empty (PNGs only when Chrome is
/// available to produce them). Errors name the offending path.
pub fn verify_pages(out_dir: &Path, pages: &[Page], expect_png: bool) -> std::io::Result<()> {
    for p in pages {
        let mut exts = vec!["html", "ansi"];
        if expect_png {
            exts.push("png");
        }
        for ext in exts {
            let path = out_dir.join(format!("{}.{ext}", p.stem));
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len == 0 {
                return Err(std::io::Error::other(format!(
                    "dump gallery incomplete: {} is missing or empty (expected non-empty, got {len} bytes)",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

const CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
/// Cold Chrome instances in flight at once. Measured 2026-09-02 on the dev
/// machine against these very pages: 1 instance writes its PNG in ~9 s, 4 in
/// ~16 s, 8 in ~46 s, and the whole 22-page gallery at once produces *zero*
/// PNGs in 60 s — the instances starve each other. 4 is the last batch size
/// that stays close to a single shot's cost.
const SHOT_BATCH: usize = 4;
/// Per-BATCH screenshot deadline. A batch of [`SHOT_BATCH`] measured ~16 s;
/// 45 s is that with room for a loaded machine. (The old 15 s was written
/// when the cost was believed to be ~2 s, and expired every capture once the
/// gallery grew.)
const SHOT_DEADLINE: Duration = Duration::from_secs(45);

/// One in-flight Chrome screenshot. Chrome's new headless mode sometimes
/// never exits after writing the screenshot (observed here with fresh
/// `--user-data-dir` profiles), so completion is judged by the PNG appearing
/// with a stable size — never by process exit — and stragglers are killed.
struct Shot {
    stem: String,
    png: PathBuf,
    profile: PathBuf,
    child: Option<std::process::Child>,
    last_size: u64,
    png_done: bool,
}

impl Shot {
    fn spawn(out_dir: &Path, v: &Page) -> Shot {
        let png = out_dir.join(format!("{}.png", v.stem));
        let _ = std::fs::remove_file(&png); // never judge a stale PNG "done"
                                            // Parallel instances need distinct profiles or Chrome serializes on
                                            // the default user-data-dir lock.
        let profile =
            std::env::temp_dir().join(format!("gameday-chrome-{}-{}", std::process::id(), v.stem));
        // ~8px/col + 60px margins, 16px/row + 84px margins (120x36 ->
        // 1020x660, the window the original single-board dump was tuned to).
        let (w, h) = (u32::from(v.cols) * 8 + 60, u32::from(v.rows) * 16 + 84);
        let child = out_dir
            .join(format!("{}.html", v.stem))
            .canonicalize()
            .and_then(|html| {
                std::process::Command::new(CHROME)
                    .args([
                        &format!("--user-data-dir={}", profile.display()),
                        "--headless=new",
                        "--disable-gpu",
                        "--hide-scrollbars",
                        "--allow-file-access-from-files",
                        &format!("--screenshot={}", png.display()),
                        &format!("--window-size={w},{h}"),
                        &format!("file://{}", html.display()),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
            })
            .map_err(|e| eprintln!("png skipped for {}: chrome spawn failed: {e}", v.stem))
            .ok();
        Shot {
            stem: v.stem.clone(),
            png,
            profile,
            child,
            last_size: 0,
            png_done: false,
        }
    }

    /// Done once the PNG exists with the same non-zero size on two
    /// consecutive polls (guards against reading a half-written file).
    fn poll(&mut self) {
        if self.png_done {
            return;
        }
        let size = std::fs::metadata(&self.png).map(|m| m.len()).unwrap_or(0);
        self.png_done = size > 0 && size == self.last_size;
        self.last_size = size;
    }

    fn cleanup(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

fn wait_for_screenshots(shots: &mut [Shot]) {
    let deadline = Instant::now() + SHOT_DEADLINE;
    loop {
        for shot in shots.iter_mut() {
            shot.poll();
        }
        if shots.iter().all(|s| s.png_done || s.child.is_none()) || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    for shot in shots {
        shot.cleanup();
    }
}

fn color_css(c: Color) -> String {
    match c {
        Color::Reset => String::new(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

pub fn buffer_to_html(buf: &Buffer) -> String {
    // A font with full block-element coverage. Nothing the app draws needs
    // U+1FB00 any more — the hero digits moved to quadrant blocks, the logo
    // art was regenerated the same way, and the scoring word's sextant rung
    // was deleted — so this is now belt-and-braces for the
    // capture rather than the load-bearing requirement it was. Kept because a
    // headless Chrome with a thin default font still substitutes badly on the
    // box-drawing rules and meter tracks.
    let font_face = std::env::var("GAMEDAY_DUMP_FONT")
        .ok()
        .filter(|p| Path::new(p).exists())
        .map(|p| format!("@font-face{{font-family:'DumpMono';src:url('file://{p}');}}\n"))
        .unwrap_or_default();
    let th = theme::current();
    let (page_bg, page_fg) = (color_css(th.bg), color_css(th.fg));
    let mut html = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>gameday board</title>
<style>
  {font_face}html,body{{margin:0;background:{page_bg};}}
  pre{{font:13px/16px 'DumpMono',Menlo,"Cascadia Mono","SF Mono",ui-monospace,monospace;
      margin:16px;padding:10px 12px;background:{page_bg};color:{page_fg};
      display:inline-block;white-space:pre;}}
  pre span{{font:inherit;}}
</style></head><body><pre>"#,
    );
    let area = buf.area();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            if sym.is_empty() {
                continue;
            }
            let escaped;
            let ch = if sym == " " {
                " "
            } else {
                escaped = html_escape(sym);
                &escaped
            };
            let mut style = String::new();
            let fg = color_css(cell.fg);
            let bg = color_css(cell.bg);
            if !fg.is_empty() {
                style.push_str(&format!("color:{fg};"));
            }
            if !bg.is_empty() {
                style.push_str(&format!("background:{bg};"));
            }
            if cell.modifier.contains(Modifier::BOLD) {
                style.push_str("font-weight:700;");
            }
            if style.is_empty() {
                html.push_str(ch);
            } else {
                html.push_str(&format!("<span style=\"{style}\">{ch}</span>"));
            }
        }
        html.push('\n');
    }
    html.push_str("</pre></body></html>\n");
    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn buffer_to_ansi(buf: &Buffer) -> String {
    let mut out = String::from("\x1b[0m");
    let area = buf.area();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            if sym.is_empty() {
                continue;
            }
            out.push_str("\x1b[0m");
            if cell.modifier.contains(Modifier::BOLD) {
                out.push_str("\x1b[1m");
            }
            if let Color::Rgb(r, g, b) = cell.fg {
                out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            if let Color::Rgb(r, g, b) = cell.bg {
                out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
            }
            out.push_str(sym);
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(buf: &Buffer) -> String {
        let area = *buf.area();
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn variant(stem: &str) -> Variant {
        gallery()
            .into_iter()
            .find(|v| v.stem == stem)
            .unwrap_or_else(|| panic!("no gallery variant named {stem:?}"))
    }

    #[test]
    fn gallery_stems_are_the_promised_fixed_names() {
        let stems: Vec<&str> = gallery().iter().map(|v| v.stem).collect();
        assert_eq!(
            stems,
            [
                "board-broadcast",
                "board-studio",
                "board-gruvbox",
                "board-daygame",
                "board-narrow",
                "board-sixty",
                "tv",
                "cut-full",
                "cut-band",
                "zoom",
                "plays-feed",
                "standings",
                "config",
                "filter",
                "theme-picker",
                "help",
                "home-live",
                "offline",
                "stale",
                "config-error",
                "nudge-seq-1",
                "nudge-seq-2",
                "nudge-seq-3",
                // The `gate-*` stems joined this list only while the design
                // gates needed them. Those questions are decided (quadrant digits,
                // the band reservation, the rebuilt studio, gruvbox's ground), so
                // the frames and their dump-only overlay hook are gone and the
                // public gallery is gate-free again.
            ],
            "gallery stems are a stable contract for other tasks"
        );
    }

    #[test]
    fn theme_picker_variant_lists_every_builtin_over_the_board() {
        let text = text_of(&render_variant(&variant("theme-picker"), 0).unwrap());
        assert!(text.contains(" THEMES "), "picker panel missing:\n{text}");
        for name in theme::BUILTIN_NAMES {
            assert!(text.contains(name), "picker missing {name}:\n{text}");
        }
        // The board behind the picker is the ranked list, not tiles.
        assert!(
            text.contains("IN PLAY"),
            "board must still render behind the picker:\n{text}"
        );
    }

    #[test]
    fn every_builtin_theme_has_a_board_capture() {
        // A theme added to BUILTIN_NAMES without a BOARD_STEMS row would
        // silently miss the gallery; pin the two lists to each other.
        let names: Vec<&str> = BOARD_STEMS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, theme::BUILTIN_NAMES.to_vec());
        for (name, stem) in BOARD_STEMS {
            assert_eq!(stem, format!("board-{name}"));
        }
    }

    /// The zoom capture is the receipt for the matchup line: three fields the
    /// mapper had long filled and nothing drew until the zoom did.
    #[test]
    fn zoom_variant_shows_the_linescore_the_matchup_line_and_an_inning_stamped_feed() {
        let text = text_of(&render_variant(&variant("zoom"), 0).unwrap());
        assert!(
            text.contains("NYY") && text.contains("TOR"),
            "the zoomed game:\n{text}"
        );
        // The MLB linescore row carries R/H/E.
        assert!(
            text.contains(" H ") || text.contains("H  E"),
            "linescore H/E:\n{text}"
        );
        // The matchup line: pitcher, batter, due up.
        assert!(text.contains("C. Schmidt"), "pitcher missing:\n{text}");
        assert!(text.contains("A. Kirk"), "batter missing:\n{text}");
        assert!(text.contains("DUE UP"), "due-up block missing:\n{text}");
        // A baseball play stamps its half-inning, not an
        // invented game clock.
        assert!(
            text.contains("[B7]"),
            "inning-tagged play stamp missing:\n{text}"
        );
        assert!(
            !text.contains("[0:42]"),
            "a baseball play has no game clock:\n{text}"
        );
    }

    /// The two cut sizes, one formatter.
    #[test]
    fn cut_variants_are_a_takeover_and_a_two_row_band() {
        let full = text_of(&render_variant(&variant("cut-full"), 0).unwrap());
        assert!(
            full.contains("TOUCHDOWN"),
            "the takeover names the score:\n{full}"
        );
        // A takeover owns the frame: the board's sections are not behind it.
        assert!(
            !full.contains("IN PLAY"),
            "the takeover is a takeover:\n{full}"
        );

        let band = text_of(&render_variant(&variant("cut-band"), 0).unwrap());
        assert!(band.contains("GOAL"), "the band names the score:\n{band}");
        assert!(
            band.contains("IN PLAY"),
            "the board never moves for a band:\n{band}"
        );
    }

    #[test]
    fn tv_variant_is_the_jumbotron_with_its_also_live_strip() {
        let text = text_of(&render_variant(&variant("tv"), 0).unwrap());
        assert!(
            text.contains("ALSO LIVE"),
            "the ALSO LIVE strip missing:\n{text}"
        );
        assert!(
            !text.contains("IN PLAY"),
            ":tv is one game, not the list:\n{text}"
        );
    }

    #[test]
    fn plays_feed_variant_renders_the_global_feed() {
        let text = text_of(&render_variant(&variant("plays-feed"), 0).unwrap());
        assert!(text.contains("PLAYS"), "feed header missing:\n{text}");
        // Scoring plays from more than one demo league land in the feed.
        assert!(
            text.contains("TOUCHDOWN"),
            "NFL scoring play missing:\n{text}"
        );
        assert!(
            text.contains("GOAL"),
            "NHL/EPL scoring play missing:\n{text}"
        );
        // Every row is stamped, including the sports with no game clock: the
        // MLB line carries its half-inning, not an empty column.
        let mlb = text
            .lines()
            .find(|l| l.contains("[MLB]"))
            .unwrap_or_else(|| panic!("no MLB row in the feed:\n{text}"));
        assert!(
            mlb.contains("T7"),
            "the MLB row must carry its inning stamp: {mlb:?}"
        );
    }

    #[test]
    fn standings_variant_renders_the_fixture_table() {
        let text = text_of(&render_variant(&variant("standings"), 0).unwrap());
        assert!(
            text.contains("STANDINGS"),
            "standings header missing:\n{text}"
        );
        assert!(
            text.contains("AMERICAN FOOTBALL CONFERENCE"),
            "fixture group name missing:\n{text}"
        );
        assert!(text.contains("BUF"), "fixture team row missing:\n{text}");
    }

    /// The four state captures exist so a reviewer can see the states without
    /// unplugging a cable — each must actually SAY its state, not just be
    /// named after it.
    #[test]
    fn state_captures_each_show_the_state_they_are_named_for() {
        let home = text_of(&render_variant(&variant("home-live"), 0).unwrap());
        // Home is the ranked board — a MY GAMES band over IN PLAY,
        // not a grid of tiles with [NFL] headers.
        assert!(
            home.contains("MY GAMES") && home.contains("IN PLAY"),
            "the ranked board:\n{home}"
        );
        // The demo's one pin leads the band, so the hero carries the flag.
        let flagged = home.lines().filter(|l| l.contains("⚑")).count();
        assert_eq!(flagged, 1, "the pinned hero is the only flag:\n{home}");

        let offline = text_of(&render_variant(&variant("offline"), 0).unwrap());
        assert!(
            offline.contains("OFFLINE · retry 40s"),
            "offline chip:\n{offline}"
        );
        assert!(
            offline.contains("last error: ESPN unreachable nfl scoreboard"),
            "the empty board must name the outage, not read as 'no games':\n{offline}"
        );

        // Seeded 240s in the past; `short_age` floors to minutes, so the label
        // is "STALE 4m" for the whole 4:00–4:59 band — no flaky seconds.
        let stale = text_of(&render_variant(&variant("stale"), 0).unwrap());
        assert!(stale.contains("STALE 4m"), "stale chip:\n{stale}");
        assert!(
            stale.contains("IN PLAY"),
            "a stale board still shows its scores:\n{stale}"
        );

        let cfg = text_of(&render_variant(&variant("config-error"), 0).unwrap());
        assert!(
            cfg.contains("config error: config.toml:7"),
            "error line:\n{cfg}"
        );
        assert!(
            cfg.contains("unknown variant"),
            "the reason is named:\n{cfg}"
        );
    }

    #[test]
    fn config_variant_renders_every_section() {
        let text = text_of(&render_variant(&variant("config"), 0).unwrap());
        for needle in ["CONFIG", "TABS", "FAVORITES", "THEME", "SORT"] {
            assert!(
                text.contains(needle),
                "missing {needle:?} in config capture:\n{text}"
            );
        }
    }

    #[test]
    fn filter_variant_narrows_the_nfl_tab_and_shows_the_pattern() {
        let text = text_of(&render_variant(&variant("filter"), 0).unwrap());
        assert!(
            text.contains("/kc"),
            "committed filter missing from footer:\n{text}"
        );
        // Rows are abbrs, not "CHIEFS" nameplates.
        assert!(text.contains("KC"), "the matching game must stay:\n{text}");
        assert!(
            !text.contains("SEA") && !text.contains("DAL"),
            "non-matching games must be filtered out:\n{text}"
        );
    }

    #[test]
    fn every_gallery_file_is_written_nonempty() {
        // Full run() minus Chrome: write html+ansi for every variant into a
        // scratch dir and hold run()'s own completeness check against it.
        let dir = std::env::temp_dir().join(format!("gameday-gallery-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pages: Vec<Page> = gallery()
            .iter()
            .map(|v| Page {
                stem: v.stem.to_string(),
                cols: v.cols,
                rows: v.rows,
                theme: v.theme.to_string(),
                buf: render_variant(v, 0).unwrap(),
            })
            .collect();
        write_pages(&dir, &pages).unwrap();
        verify_pages(&dir, &pages, false).unwrap();
        // The check actually bites: truncate one file and it names the path.
        std::fs::write(dir.join("help.ansi"), "").unwrap();
        let err = verify_pages(&dir, &pages, false).unwrap_err().to_string();
        assert!(
            err.contains("help.ansi"),
            "error must name the empty file: {err}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn themed_boards_use_their_palette_and_restore_the_thread_theme() {
        assert_eq!(theme::current_name(), "broadcast");
        for (name, stem) in BOARD_STEMS {
            let buf = render_variant(&variant(stem), 0).unwrap();
            assert_eq!(buf[(0, 0)].bg, theme::builtin(name).bg, "{stem} background");
        }
        // Rendering the other ten must not leak into the thread's theme.
        assert_eq!(theme::current_name(), "broadcast");
    }

    #[test]
    fn help_variant_renders_the_overlay() {
        // The overlay's own panel is lowercase now too.
        let text = text_of(&render_variant(&variant("help"), 0).unwrap());
        assert!(
            text.contains(" keys "),
            "help overlay panel missing:\n{text}"
        );
    }

    /// The two size captures are the ladder's ends: the same
    /// ranked board, no sidebar at any width, and a score in some form at both.
    #[test]
    fn the_two_size_captures_are_the_same_board_at_their_own_sizes() {
        for (stem, want) in [("board-narrow", (80u16, 24u16)), ("board-sixty", (60, 40))] {
            let v = variant(stem);
            assert_eq!((v.cols, v.rows), want, "{stem} size");
            let buf = render_variant(&v, 0).unwrap();
            assert_eq!((buf.area().width, buf.area().height), want, "{stem} buffer");
            let text = text_of(&buf);
            // There is no sidebar at any width any more.
            assert!(
                !text.contains("GLOBAL ALERTS"),
                "{stem}: the sidebar is deleted:\n{text}"
            );
            assert!(
                text.contains("IN PLAY"),
                "{stem}: the ranked board:\n{text}"
            );
        }
    }

    /// The nudge sequence is the whole point of three stems instead of one:
    /// before the scripted re-sort there is no arrow, at it the risen game
    /// wears one, and a tick later it still does — while the row itself never
    /// moves a cell (A′ calls #6/#7).
    #[test]
    fn the_nudge_sequence_shows_an_arrow_appear_and_hold_without_moving_the_row() {
        let frames: Vec<String> = ["nudge-seq-1", "nudge-seq-2", "nudge-seq-3"]
            .iter()
            .map(|s| text_of(&render_variant(&variant(s), 0).unwrap()))
            .collect();
        // The nudge lives at column 2 of the 4-cell row gutter and nowhere
        // else — the footer's `↑↓ move` legend carries the same glyph one
        // column over and is not a nudge.
        let gutter_nudges = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|l| l.chars().nth(2) == Some('↑'))
                .map(|l| l.chars().take(4).collect::<String>().trim().to_string())
                .collect()
        };
        assert!(
            gutter_nudges(&frames[0]).is_empty(),
            "frame 1 is the quiet board:\n{}",
            frames[0]
        );
        // ↑1, not ↑2: situation bonuses scale with closeness now, so the NHL
        // power play no longer sits above the 8th-inning game to be passed.
        for (i, f) in frames.iter().enumerate().skip(1) {
            assert_eq!(
                gutter_nudges(f),
                vec!["▌ ↑1".to_string()],
                "frame {} must show the risen game's ↑1 and nothing else:\n{f}",
                i + 1
            );
        }
        // The bases loading is what re-sorted it, and the chip says so.
        assert!(
            frames[1].contains("BASES LOADED"),
            "the cause is on screen:\n{}",
            frames[1]
        );
        // The risen row keeps its columns: the NYY/TOR pair sits at the same
        // offset within its line in every frame.
        // Character columns, not byte offsets: `↑` is three bytes, so a byte
        // index would report the arrow itself as a shift.
        let col_of = |text: &str| -> Option<usize> {
            text.lines()
                .find(|l| l.contains("NYY"))
                .map(|l| l[..l.find("NYY").unwrap()].chars().count())
        };
        assert_eq!(
            col_of(&frames[0]),
            col_of(&frames[1]),
            "the row must not shift for a nudge"
        );
        assert_eq!(
            col_of(&frames[1]),
            col_of(&frames[2]),
            "nor for the arrow persisting"
        );
    }

    #[test]
    fn demo_board_renders_the_redzone_grammar() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0).unwrap();
        let mut text = String::new();
        for y in 0..DUMP_ROWS {
            for x in 0..DUMP_COLS {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        // The tile grammar (tile headers, text score rows, LAST PLAYS,
        // MOMENTUM, the sidebar) is deleted; what the demo board must show
        // now is the sections, the hero's chip and the chrome. The header
        // never shows "FILTER:" and the Board footer is the lowercase A′
        // legend, not the old "NAV:" chord list.
        for needle in [
            "GAMEDAY", "MY GAMES", "IN PLAY", "FINAL", "RED ZONE", "s sort", "q quit",
        ] {
            assert!(
                text.contains(needle),
                "missing {needle:?} in board:\n{text}"
            );
        }
        assert!(
            !text.contains("FILTER:"),
            "the FILTER: label is gone:\n{text}"
        );
        assert!(
            !text.contains("NAV:"),
            "the Board footer drops NAV\\::\n{text}"
        );
    }

    /// The four inline meters were a tile feature. Only the hero has room for
    /// state now, and at the 10-row bracket even its meter row yields to the
    /// fragment — the fragment is the last thing a shrinking hero gives up — so
    /// what every size must still show is the hero SAYING its state. The zoom's
    /// 12-row hero bracket is where the meter row itself still fits.
    #[test]
    fn tick_zero_board_names_the_heros_state_at_every_size() {
        // 2x2 at 120x36, the 80x24 narrow board, and the zoom overview all
        // carry the gauge row: label at the left, value tail intact.
        let wide = text_of(&render_variant(&variant("board-broadcast"), 0).unwrap());
        assert!(
            wide.lines().any(|l| l.contains("RED ZONE")),
            "120x36 board: the hero's state chip is missing:\n{wide}"
        );
        assert!(
            wide.lines().any(|l| l.contains("BALL ON TB 3")),
            "120x36 board: the hero's fragment line is missing:\n{wide}"
        );
        let narrow = text_of(&render_variant(&variant("board-narrow"), 0).unwrap());
        assert!(
            narrow.lines().any(|l| l.contains("RED ZONE")),
            "80x24 board: the hero still names its state:\n{narrow}"
        );
        // At 80×24 — the width where the old sextant digits stopped resolving
        // into readable numbers — the hero's score is drawn from the quadrant
        // table, not a text row. (This assertion is the surviving half of a
        // retired `gate-digits-*` contrast — the winner is the product, so
        // the product frame carries the receipt.)
        assert!(
            narrow.contains("▀▀█") || narrow.contains("█▀█"),
            "80x24 board: the hero must draw quad digits:\n{narrow}"
        );
        assert!(!wide.contains('┃'), "the meter column is gone");
    }

    #[test]
    fn default_dump_uses_big_scores_and_shows_the_shot_clock_chip() {
        let th = theme::current();
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0).unwrap();
        let mut text = String::new();
        let mut star_bg = 0;
        for y in 0..DUMP_ROWS {
            for x in 0..DUMP_COLS {
                text.push_str(buf[(x, y)].symbol());
                if buf[(x, y)].bg == th.star {
                    star_bg += 1;
                }
            }
            text.push('\n');
        }
        // Big style: glyph digits (quad at this bracket), so the single-row
        // score text is gone but the identity rows appear under them. The 4-up NFL tile can't fit
        // "BUCCANEERS 11-6", so both sides fall back to the abbr form rather
        // than losing the records.
        assert!(
            !text.contains("27 - 24"),
            "the board never prints a text score row:\n{text}"
        );
        // The hero's nameplates carry the identity the tile header
        // used to; the shot-clock chip was a tile chip and is gone with it.
        // The nameplates are mirrored: `KC 11-6` left, `11-6  TB` right.
        assert!(
            text.contains("KC 11-6"),
            "hero away nameplate missing:\n{text}"
        );
        assert!(
            text.contains("11-6"),
            "hero home nameplate missing:\n{text}"
        );
        let _ = star_bg;
    }

    #[test]
    fn dump_at_td_tick_renders_the_new_score() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, crate::sim::KC_TD_TICK).unwrap();
        let mut text = String::new();
        for y in 0..DUMP_ROWS {
            for x in 0..DUMP_COLS {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        // The hero's score is digit glyphs, so the capture's proof
        // that the TD landed is the play text (the digits themselves are
        // cell-tested in `board::hero`).
        assert!(text.contains("TOUCHDOWN"), "TD play missing:\n{text}");
    }

    // The tile's inverted score flash was deleted with the tile; the
    // board's answer to a score is the cut overlay, which is where
    // the "a score is visible in the capture" test belongs. Nothing here can
    // assert it in the meantime without asserting a feature that is gone.

    #[test]
    fn html_dump_contains_colored_cells() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0).unwrap();
        let html = buffer_to_html(&buf);
        assert!(html.contains("color:#"));
        // Cells are individually wrapped in spans; strip tags to check content.
        let text: String = {
            let mut out = String::new();
            let mut in_tag = false;
            for c in html.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    c if !in_tag => out.push(c),
                    _ => {}
                }
            }
            out
        };
        assert!(text.contains("GAMEDAY"), "{text}");
    }
}
