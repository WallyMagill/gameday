//! `gameday dump`: render every surface of the demo board offscreen and write
//! a fixed-name gallery into out/ — HTML + ANSI always, plus a PNG per capture
//! when headless Chrome is available. This is the visual iteration loop:
//! compare out/board-broadcast.png to the reference image. The gallery:
//!
//!   board-<theme> — home board, big scores, one per BUILT-IN theme (eleven:
//!       board-broadcast … board-dracula, selected programmatically, not via env)
//!   board-compact — broadcast theme, compact score_style
//!   tab-nfl       — NFL league tab with the slate visible and a slate row selected
//!   focus         — a focused game view
//!   help          — the '?' overlay over the dimmed board
//!   narrow        — 80x24, the sidebar-less layout
//!   zoom-stats    — the Zoom STATS tab, box score from the committed fixture
//!   plays-feed    — the global scoring feed (:plays)
//!   standings     — the NFL standings table from the committed fixture
//!   config        — the in-app config editor (:config)
//!   filter        — the NFL tab narrowed by a committed /kc filter
//!   theme-picker  — the `:theme` picker panel over the home board
//!
//! Every capture is the sim state at a fixed tick (`--tick N`, default 0), so
//! repeated runs are pixel-deterministic. No timestamps in file names.
//!
//! `dump --style-lab` renders the throwaway style-variant set instead — see
//! `crate::style_lab`, which shares this module's page pipeline.

use crate::app::{App, Tab};
use crate::demo;
use crate::domain::League;
use crate::provider::memory::MemoryProvider;
use crate::provider::{map, SportsProvider};
use crate::theme;
use crate::tiles::ScoreStyle;
use crate::views::{View, ZoomTab};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DUMP_COLS: u16 = 120;
pub const DUMP_ROWS: u16 = 36;
/// Whole-gallery runtime budget from the task spec (a target, not a measured
/// number); overruns print actual vs budget naming the slow phase.
const BUDGET: Duration = Duration::from_secs(20);

/// One gallery capture: which surface, at what size, in which theme.
pub struct Variant {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    /// A built-in theme name (`theme::BUILTIN_NAMES`).
    pub theme: &'static str,
    pub style: ScoreStyle,
    setup: fn(&mut App),
}

/// `board-<theme>` stems, one per built-in, in `BUILTIN_NAMES` order. Static
/// strings because stems are the fixed-name contract other tasks read; a test
/// pins this list to `BUILTIN_NAMES` so a new theme can't ship without a board.
pub const BOARD_STEMS: [(&str, &str); 11] = [
    ("broadcast", "board-broadcast"),
    ("studio", "board-studio"),
    ("ceefax", "board-ceefax"),
    ("phosphor", "board-phosphor"),
    ("gruvbox", "board-gruvbox"),
    ("tokyo-night", "board-tokyo-night"),
    ("nord", "board-nord"),
    ("catppuccin-mocha", "board-catppuccin-mocha"),
    ("rose-pine", "board-rose-pine"),
    ("everforest", "board-everforest"),
    ("dracula", "board-dracula"),
];

/// The fixed gallery, in write order. Stems are stable file names — other
/// tasks (themes, animation frames, tile polish, keyboard) verify against
/// these exact paths, so renames here are breaking.
pub fn gallery() -> Vec<Variant> {
    fn home(_: &mut App) {}
    fn tab_nfl(app: &mut App) {
        app.tab = Tab::League(League::Nfl);
        // Land the selection past the live tiles, on the first slate row, so
        // the ▸ slate highlight is part of the capture.
        app.selected = app.live_games().len();
    }
    fn focus(app: &mut App) {
        // The demo NFL live game, zoomed: tab bar + expanded single-game view.
        app.view = View::Zoom {
            game_id: "nfl-live".into(),
            tab: ZoomTab::Overview,
        };
    }
    fn help(app: &mut App) {
        app.help_open = true;
    }
    // The stats/standings captures feed through MemoryProvider — the same
    // trait path the live poll uses, no network. Standings come from the
    // committed fixture; the box score is the demo game's own (the fixture
    // is a real TEN@SEA game, and its players under KC/TB columns made the
    // capture contradict itself).
    fn zoom_stats(app: &mut App) {
        let mut p = MemoryProvider::new();
        p.stats.insert("nfl-live".into(), demo::demo_stats());
        let (stats, _) = p.stats(League::Nfl, "nfl-live").expect("seeded stats");
        app.merge_stats("nfl-live", stats);
        app.view = View::Zoom {
            game_id: "nfl-live".into(),
            tab: ZoomTab::Stats,
        };
    }
    fn plays_feed(app: &mut App) {
        app.view = View::PlaysFeed;
    }
    fn standings(app: &mut App) {
        let mut p = MemoryProvider::new();
        p.standings.insert(
            League::Nfl,
            map::map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json"))
                .expect("nfl_standings.json fixture must map"),
        );
        let (table, _) = p.standings(League::Nfl).expect("seeded standings");
        app.merge_standings(table);
        app.view = View::Standings(League::Nfl);
    }
    fn config(app: &mut App) {
        // A seeded favorite so the FAVORITES section shows a real row.
        app.config.favorites.push(crate::config::Favorite {
            league: League::Nfl,
            team_abbr: "KC".into(),
        });
        app.view = View::ConfigView;
    }
    fn filter(app: &mut App) {
        app.tab = Tab::League(League::Nfl);
        app.filter = Some("kc".into());
    }
    fn theme_picker(app: &mut App) {
        app.open_theme_picker();
    }
    let full = |stem, theme, style, setup| Variant {
        stem,
        cols: DUMP_COLS,
        rows: DUMP_ROWS,
        theme,
        style,
        setup,
    };
    let mut out: Vec<Variant> = BOARD_STEMS
        .iter()
        .map(|(name, stem)| full(*stem, *name, ScoreStyle::Big, home as fn(&mut App)))
        .collect();
    out.extend([
        full("board-compact", "broadcast", ScoreStyle::Compact, home),
        full("tab-nfl", "broadcast", ScoreStyle::Big, tab_nfl),
        full("focus", "broadcast", ScoreStyle::Big, focus),
        full("help", "broadcast", ScoreStyle::Big, help),
        Variant {
            stem: "narrow",
            cols: 80,
            rows: 24,
            theme: "broadcast",
            style: ScoreStyle::Big,
            setup: home,
        },
        full("zoom-stats", "broadcast", ScoreStyle::Big, zoom_stats),
        full("plays-feed", "broadcast", ScoreStyle::Big, plays_feed),
        full("standings", "broadcast", ScoreStyle::Big, standings),
        full("config", "broadcast", ScoreStyle::Big, config),
        full("filter", "broadcast", ScoreStyle::Big, filter),
        full("theme-picker", "broadcast", ScoreStyle::Big, theme_picker),
    ]);
    out
}

/// Run `f` with `name` as the current theme, restoring the caller's theme
/// after — variants can't leak palettes into each other (or into tests on
/// the same thread). A dump asks only for built-ins, so a miss is a bug.
fn with_theme<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let prev = theme::current_name();
    theme::set_current(name).unwrap_or_else(|e| panic!("dump theme: {e}"));
    let out = f();
    theme::set_current(&prev).expect("the previous theme is still loaded");
    out
}

/// Demo app at simulation tick `tick` (0 = the seed board in demo.rs).
/// Advancing is pure — N scripted steps, no wall clock — so `dump --tick N`
/// always captures the same frame. Boards for tick-1 are applied first so a
/// score that changes AT `tick` is caught mid-flash, exactly like the live
/// loop would show it (`--tick 15` captures the KC TD flash).
pub fn demo_app(config_dir: PathBuf, tick: u64) -> App {
    let mut app = App::new(demo::demo_config(), demo::demo_pins(), config_dir);
    if tick > 0 {
        for (league, games) in crate::sim::Simulator::boards_at(tick - 1) {
            app.apply_boards(league, games, false);
        }
    }
    app.tick = tick;
    for (league, games) in crate::sim::Simulator::boards_at(tick) {
        app.apply_boards(league, games, false);
    }
    app
}

pub fn render_demo_buffer(
    cols: u16,
    rows: u16,
    tick: u64,
    score_style: ScoreStyle,
) -> std::io::Result<Buffer> {
    let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut app = demo_app(dir, tick);
    // Dump variants pick the score style programmatically (no env hack).
    app.config.score_style = score_style;
    let mut term = Terminal::new(TestBackend::new(cols, rows))?;
    term.draw(|f| app.draw(f))?;
    Ok(term.backend().buffer().clone())
}

/// Render one gallery variant. Sets the variant's theme for the duration of
/// the render and restores the caller's theme after, so variants can't leak
/// palettes into each other (or into tests on the same thread).
pub fn render_variant(v: &Variant, tick: u64) -> std::io::Result<Buffer> {
    with_theme(v.theme, || {
        let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut app = demo_app(dir, tick);
        app.config.score_style = v.style;
        (v.setup)(&mut app);
        let mut term = Terminal::new(TestBackend::new(v.cols, v.rows))?;
        term.draw(|f| app.draw(f))?;
        Ok(term.backend().buffer().clone())
    })
}

/// One rendered page ready for the shared write/screenshot/verify pipeline —
/// the gallery and the style lab (`--style-lab`) both feed through this shape.
pub struct Page {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    /// Built-in theme name the page was rendered under (page bg/fg).
    pub theme: &'static str,
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
                stem: v.stem,
                cols: v.cols,
                rows: v.rows,
                theme: v.theme,
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
        with_theme(p.theme, || {
            std::fs::write(&html_path, buffer_to_html(&p.buf)).and_then(|()| {
                std::fs::write(out_dir.join(format!("{}.ansi", p.stem)), buffer_to_ansi(&p.buf))
            })
        })?;
        eprintln!("wrote {}", html_path.display());
    }
    Ok(())
}

/// Phase 2: one headless-Chrome instance per PNG, all spawned in parallel —
/// Chrome startup dominates the runtime, so serial capture would blow the
/// budget at 8+ images while parallel stays well inside it. Returns whether
/// Chrome was available (and thus whether PNGs should be expected).
pub fn screenshot_pages(out_dir: &Path, pages: &[Page]) -> bool {
    let chrome = Path::new(CHROME).exists();
    if !chrome {
        eprintln!("png skipped: Chrome not found at {CHROME} (open the .html files instead)");
        return false;
    }
    let mut shots: Vec<Shot> = pages.iter().map(|p| Shot::spawn(out_dir, p)).collect();
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
/// Per-run screenshot deadline. A guess with headroom, not a measurement:
/// 8 parallel cold-started Chromes finished in ~2s on the dev machine; 15s
/// keeps the whole gallery inside the 20s budget on a slower box.
const SHOT_DEADLINE: Duration = Duration::from_secs(15);

/// One in-flight Chrome screenshot. Chrome's new headless mode sometimes
/// never exits after writing the screenshot (observed here with fresh
/// `--user-data-dir` profiles), so completion is judged by the PNG appearing
/// with a stable size — never by process exit — and stragglers are killed.
struct Shot {
    stem: &'static str,
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
        Shot { stem: v.stem, png, profile, child, last_size: 0, png_done: false }
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
    // Cascadia Mono carries the sextant glyphs (U+1FB00 block) the logo art
    // uses; system monospace fonts mostly don't, so the capture would show tofu.
    let font_face = std::env::var("GAMEDAY_DUMP_FONT")
        .ok()
        .filter(|p| Path::new(p).exists())
        .map(|p| {
            format!(
                "@font-face{{font-family:'DumpMono';src:url('file://{p}');}}\n"
            )
        })
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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
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
                "board-ceefax",
                "board-phosphor",
                "board-gruvbox",
                "board-tokyo-night",
                "board-nord",
                "board-catppuccin-mocha",
                "board-rose-pine",
                "board-everforest",
                "board-dracula",
                "board-compact",
                "tab-nfl",
                "focus",
                "help",
                "narrow",
                "zoom-stats",
                "plays-feed",
                "standings",
                "config",
                "filter",
                "theme-picker",
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
        assert!(text.contains("[NBA]"), "board must still render behind the picker:\n{text}");
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

    #[test]
    fn zoom_stats_variant_renders_rows_and_leaders_of_the_zoomed_game() {
        let text = text_of(&render_variant(&variant("zoom-stats"), 0).unwrap());
        assert!(text.contains("STATS"), "zoom tab bar with STATS missing:\n{text}");
        assert!(text.contains("Total Yards"), "stat row missing:\n{text}");
        assert!(text.contains("LEADERS"), "leaders block missing:\n{text}");
        // The capture zooms KC@TB, so every leader line must belong to KC or
        // TB — no TEN/SEA players from the mapper fixture under KC/TB columns.
        let leaders: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("PASSING YARDS") || l.contains("TACKLES"))
            .collect();
        assert!(!leaders.is_empty(), "no leader lines:\n{text}");
        for line in &leaders {
            let team = line.trim_start().split_whitespace().next().unwrap_or("");
            assert!(team == "KC" || team == "TB", "leader from another game: {line:?}");
        }
        assert!(text.contains("Mahomes"), "KC leader missing:\n{text}");
        assert!(!text.contains("TEN ") && !text.contains("SEA "), "fixture teams leaked:\n{text}");
    }

    #[test]
    fn plays_feed_variant_renders_the_global_feed() {
        let text = text_of(&render_variant(&variant("plays-feed"), 0).unwrap());
        assert!(text.contains("PLAYS"), "feed header missing:\n{text}");
        // Scoring plays from more than one demo league land in the feed.
        assert!(text.contains("TOUCHDOWN"), "NFL scoring play missing:\n{text}");
        assert!(text.contains("GOAL"), "NHL/EPL scoring play missing:\n{text}");
    }

    #[test]
    fn standings_variant_renders_the_fixture_table() {
        let text = text_of(&render_variant(&variant("standings"), 0).unwrap());
        assert!(text.contains("STANDINGS"), "standings header missing:\n{text}");
        assert!(
            text.contains("AMERICAN FOOTBALL CONFERENCE"),
            "fixture group name missing:\n{text}"
        );
        assert!(text.contains("BUF"), "fixture team row missing:\n{text}");
    }

    #[test]
    fn config_variant_renders_every_section() {
        let text = text_of(&render_variant(&variant("config"), 0).unwrap());
        for needle in ["CONFIG", "TABS", "FAVORITES", "THEME", "SCORE", "LAYOUT"] {
            assert!(text.contains(needle), "missing {needle:?} in config capture:\n{text}");
        }
    }

    #[test]
    fn filter_variant_narrows_the_nfl_tab_and_shows_the_pattern() {
        let text = text_of(&render_variant(&variant("filter"), 0).unwrap());
        assert!(text.contains("/kc"), "committed filter missing from footer:\n{text}");
        assert!(text.contains("CHIEFS"), "the matching game must stay:\n{text}");
        assert!(
            !text.contains("SEAHAWKS") && !text.contains("COWBOYS"),
            "non-matching slate games must be filtered out:\n{text}"
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
                stem: v.stem,
                cols: v.cols,
                rows: v.rows,
                theme: v.theme,
                buf: render_variant(v, 0).unwrap(),
            })
            .collect();
        write_pages(&dir, &pages).unwrap();
        verify_pages(&dir, &pages, false).unwrap();
        // The check actually bites: truncate one file and it names the path.
        std::fs::write(dir.join("help.ansi"), "").unwrap();
        let err = verify_pages(&dir, &pages, false).unwrap_err().to_string();
        assert!(err.contains("help.ansi"), "error must name the empty file: {err}");
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
    fn tab_nfl_variant_shows_the_slate_with_a_selected_row() {
        let text = text_of(&render_variant(&variant("tab-nfl"), 0).unwrap());
        assert!(text.contains(" SLATE "), "NFL tab must render the slate:\n{text}");
        assert!(text.contains('▸'), "a slate row must carry the selection marker:\n{text}");
    }

    #[test]
    fn focus_variant_renders_the_focused_game() {
        let text = text_of(&render_variant(&variant("focus"), 0).unwrap());
        assert!(text.contains("FOCUS KC@TB"), "footer must show the focused game:\n{text}");
        assert!(text.contains("BACK"), "focused footer offers [ESC] BACK:\n{text}");
    }

    #[test]
    fn help_variant_renders_the_overlay() {
        let text = text_of(&render_variant(&variant("help"), 0).unwrap());
        assert!(text.contains(" KEYS "), "help overlay panel missing:\n{text}");
    }

    #[test]
    fn narrow_variant_is_80x24_without_the_sidebar() {
        let v = variant("narrow");
        assert_eq!((v.cols, v.rows), (80, 24));
        let buf = render_variant(&v, 0).unwrap();
        assert_eq!((buf.area().width, buf.area().height), (80, 24));
        let text = text_of(&buf);
        assert!(
            !text.contains("GLOBAL ALERTS"),
            "sidebar must drop below 100 cols:\n{text}"
        );
    }

    #[test]
    fn compact_variant_uses_the_single_row_score() {
        let text = text_of(&render_variant(&variant("board-compact"), 0).unwrap());
        assert!(text.contains("27 - 24"), "compact score row missing:\n{text}");
    }

    #[test]
    fn demo_board_renders_the_redzone_grammar() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0, ScoreStyle::Compact).unwrap();
        let mut text = String::new();
        for y in 0..DUMP_ROWS {
            for x in 0..DUMP_COLS {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        for needle in [
            "GAMEDAY", "FILTER:", "[NFL]", "[NBA]", "[MLB]", "[NHL]", "27 - 24", "88 - 81",
            "5 - 3", "3 - 2", "LAST PLAYS", "MOMENTUM", "RED ZONE", "TICKER", "GLOBAL ALERTS",
            "NAV:",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in board:\n{text}");
        }
    }

    #[test]
    fn default_dump_uses_big_scores_and_shows_the_shot_clock_chip() {
        let th = theme::current();
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0, ScoreStyle::default()).unwrap();
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
        // Big style: sextant digits, so the single-row score text is gone but
        // the name+record rows appear under the digits.
        assert!(!text.contains("27 - 24"), "default is big, not the text score row:\n{text}");
        assert!(text.contains("CHIEFS 11-6"), "big identity row missing:\n{text}");
        // NBA demo tile carries a shot clock => the boxed amber chip renders
        // (star-background cells beyond the [ALL] header tab).
        assert!(text.contains(" 24 "), "shot clock chip text missing");
        assert!(star_bg > "[ALL]".len(), "amber chip cells missing, got {star_bg}");
    }

    #[test]
    fn dump_at_td_tick_renders_the_new_score() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, crate::sim::KC_TD_TICK, ScoreStyle::Compact).unwrap();
        let mut text = String::new();
        for y in 0..DUMP_ROWS {
            for x in 0..DUMP_COLS {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("33 - 24"), "KC TD score missing:\n{text}");
        assert!(text.contains("TOUCHDOWN"), "TD play missing:\n{text}");
    }

    #[test]
    fn td_tick_dump_captures_the_score_flash() {
        let th = theme::current();
        let live_bg_cells = |tick: u64| {
            let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, tick, ScoreStyle::default()).unwrap();
            let mut n = 0;
            for y in 0..DUMP_ROWS {
                for x in 0..DUMP_COLS {
                    if buf[(x, y)].bg == th.live {
                        n += 1;
                    }
                }
            }
            n
        };
        assert_eq!(live_bg_cells(0), 0, "no score changed at tick 0 — nothing flashes");
        assert!(
            live_bg_cells(crate::sim::KC_TD_TICK) > 0,
            "the KC TD at tick {} must render mid-flash",
            crate::sim::KC_TD_TICK
        );
        // Deterministic: the same tick always renders the same frame.
        assert_eq!(
            live_bg_cells(crate::sim::KC_TD_TICK),
            live_bg_cells(crate::sim::KC_TD_TICK)
        );
    }

    #[test]
    fn html_dump_contains_colored_cells() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, 0, ScoreStyle::default()).unwrap();
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
