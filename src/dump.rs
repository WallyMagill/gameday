//! `gameday dump`: render every surface of the demo board offscreen and write
//! a fixed-name gallery into out/ — HTML + ANSI always, plus a PNG per capture
//! when headless Chrome is available. This is the visual iteration loop:
//! compare out/board-broadcast.png to the reference image. The gallery:
//!
//!   board-broadcast / board-ceefax / board-phosphor — home board, big scores,
//!       one per theme (selected programmatically, not via env)
//!   board-compact — broadcast theme, compact score_style
//!   tab-nfl       — NFL league tab with the slate visible and a slate row selected
//!   focus         — a focused game view
//!   help          — the '?' overlay over the dimmed board
//!   narrow        — 80x24, the sidebar-less layout
//!
//! Every capture is the sim state at a fixed tick (`--tick N`, default 0), so
//! repeated runs are pixel-deterministic. No timestamps in file names.

use crate::app::{App, Tab};
use crate::demo;
use crate::domain::League;
use crate::theme::{self, ThemeName};
use crate::tiles::ScoreStyle;
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
    pub theme: ThemeName,
    pub style: ScoreStyle,
    setup: fn(&mut App),
}

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
        // The demo NFL live game; focus renders the expanded single-game view.
        app.focused_id = Some("nfl-live".into());
    }
    fn help(app: &mut App) {
        app.help_open = true;
    }
    let full = |stem, theme, style, setup| Variant {
        stem,
        cols: DUMP_COLS,
        rows: DUMP_ROWS,
        theme,
        style,
        setup,
    };
    vec![
        full("board-broadcast", ThemeName::Broadcast, ScoreStyle::Big, home as fn(&mut App)),
        full("board-ceefax", ThemeName::Ceefax, ScoreStyle::Big, home),
        full("board-phosphor", ThemeName::Phosphor, ScoreStyle::Big, home),
        full("board-compact", ThemeName::Broadcast, ScoreStyle::Compact, home),
        full("tab-nfl", ThemeName::Broadcast, ScoreStyle::Big, tab_nfl),
        full("focus", ThemeName::Broadcast, ScoreStyle::Big, focus),
        full("help", ThemeName::Broadcast, ScoreStyle::Big, help),
        Variant {
            stem: "narrow",
            cols: 80,
            rows: 24,
            theme: ThemeName::Broadcast,
            style: ScoreStyle::Big,
            setup: home,
        },
    ]
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
    let prev = theme::current_name();
    theme::set_current(v.theme);
    let result = (|| {
        let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut app = demo_app(dir, tick);
        app.config.score_style = v.style;
        (v.setup)(&mut app);
        let mut term = Terminal::new(TestBackend::new(v.cols, v.rows))?;
        term.draw(|f| app.draw(f))?;
        Ok(term.backend().buffer().clone())
    })();
    theme::set_current(prev);
    result
}

pub fn run(out_dir: &Path, tick: u64) -> std::io::Result<()> {
    let start = Instant::now();
    std::fs::create_dir_all(out_dir)?;
    let variants = gallery();
    // Phase 1 (one process, cheap): render every buffer and write HTML + ANSI.
    for v in &variants {
        // buffer_to_html reads theme::current() for the page bg/fg, so the
        // serialization happens under the variant's theme too.
        let prev = theme::current_name();
        theme::set_current(v.theme);
        let buf = render_variant(v, tick)?;
        let html_path = out_dir.join(format!("{}.html", v.stem));
        std::fs::write(&html_path, buffer_to_html(&buf))?;
        std::fs::write(out_dir.join(format!("{}.ansi", v.stem)), buffer_to_ansi(&buf))?;
        theme::set_current(prev);
        eprintln!("wrote {}", html_path.display());
    }
    let render_done = start.elapsed();
    // Phase 2: one headless-Chrome instance per PNG, all spawned in parallel —
    // Chrome startup dominates the runtime, so serial capture would blow the
    // budget at 8 images while parallel stays well inside it.
    let chrome = Path::new(CHROME).exists();
    if chrome {
        let mut shots: Vec<Shot> = variants
            .iter()
            .map(|v| Shot::spawn(out_dir, v))
            .collect();
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
    } else {
        eprintln!("png skipped: Chrome not found at {CHROME} (open the .html files instead)");
    }
    // The gallery is a verification artifact for other tasks: fail loudly if
    // any promised file is missing or empty instead of exiting green.
    verify_gallery(out_dir, &variants, chrome)?;
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

/// Every promised gallery file must exist and be non-empty (PNGs only when
/// Chrome is available to produce them). Errors name the offending path.
fn verify_gallery(out_dir: &Path, variants: &[Variant], expect_png: bool) -> std::io::Result<()> {
    for v in variants {
        let mut exts = vec!["html", "ansi"];
        if expect_png {
            exts.push("png");
        }
        for ext in exts {
            let path = out_dir.join(format!("{}.{ext}", v.stem));
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
    fn spawn(out_dir: &Path, v: &Variant) -> Shot {
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
                "board-ceefax",
                "board-phosphor",
                "board-compact",
                "tab-nfl",
                "focus",
                "help",
                "narrow",
            ],
            "gallery stems are a stable contract for other tasks"
        );
    }

    #[test]
    fn every_gallery_file_is_written_nonempty() {
        // Full run() minus Chrome: write html+ansi for every variant into a
        // scratch dir and hold run()'s own completeness check against it.
        let dir = std::env::temp_dir().join(format!("gameday-gallery-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let variants = gallery();
        for v in &variants {
            let prev = theme::current_name();
            theme::set_current(v.theme);
            let buf = render_variant(v, 0).unwrap();
            std::fs::write(dir.join(format!("{}.html", v.stem)), buffer_to_html(&buf)).unwrap();
            std::fs::write(dir.join(format!("{}.ansi", v.stem)), buffer_to_ansi(&buf)).unwrap();
            theme::set_current(prev);
        }
        verify_gallery(&dir, &variants, false).unwrap();
        // The check actually bites: truncate one file and it names the path.
        std::fs::write(dir.join("help.ansi"), "").unwrap();
        let err = verify_gallery(&dir, &variants, false).unwrap_err().to_string();
        assert!(err.contains("help.ansi"), "error must name the empty file: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn themed_boards_use_their_palette_and_restore_the_thread_theme() {
        assert_eq!(theme::current_name(), ThemeName::Broadcast);
        for (stem, want) in [
            ("board-broadcast", theme::Theme::broadcast().bg),
            ("board-ceefax", theme::Theme::ceefax().bg),
            ("board-phosphor", theme::Theme::phosphor().bg),
        ] {
            let buf = render_variant(&variant(stem), 0).unwrap();
            assert_eq!(buf[(0, 0)].bg, want, "{stem} background");
        }
        // Rendering ceefax/phosphor must not leak into the thread's theme.
        assert_eq!(theme::current_name(), ThemeName::Broadcast);
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
