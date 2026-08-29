//! `gameday dump`: render the demo board offscreen at 120x36 and write
//! HTML + ANSI captures (plus a PNG when headless Chrome is available).
//! This is the visual iteration loop — compare out/board.png to the reference.

use crate::app::App;
use crate::demo;
use crate::theme;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::{Path, PathBuf};

pub const DUMP_COLS: u16 = 120;
pub const DUMP_ROWS: u16 = 36;

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

pub fn render_demo_buffer(cols: u16, rows: u16, tick: u64) -> std::io::Result<Buffer> {
    let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut app = demo_app(dir, tick);
    // Dev hook: GAMEDAY_DUMP_HELP=1 captures the '?' overlay over the board.
    if std::env::var("GAMEDAY_DUMP_HELP").is_ok() {
        app.help_open = true;
    }
    let mut term = Terminal::new(TestBackend::new(cols, rows))?;
    term.draw(|f| app.draw(f))?;
    Ok(term.backend().buffer().clone())
}

pub fn run(out_dir: &Path, tick: u64) -> std::io::Result<()> {
    // Theme hook for visual iteration: GAMEDAY_THEME=ceefax|phosphor|broadcast.
    // The dump-gallery task will iterate all themes; this selects one for now.
    if let Ok(name) = std::env::var("GAMEDAY_THEME") {
        theme::set_current(theme::parse_or_default(&name));
    }
    std::fs::create_dir_all(out_dir)?;
    capture(out_dir, "board", tick)?;
    // Second capture for the big-score A/B; harmless extra file until decided.
    std::env::set_var("GAMEDAY_BIG_SCORES", "1");
    let res = capture(out_dir, "board-big", tick);
    std::env::remove_var("GAMEDAY_BIG_SCORES");
    res
}

fn capture(out_dir: &Path, stem: &str, tick: u64) -> std::io::Result<()> {
    let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, tick)?;
    let html_path = out_dir.join(format!("{stem}.html"));
    std::fs::write(&html_path, buffer_to_html(&buf))?;
    std::fs::write(out_dir.join(format!("{stem}.ansi")), buffer_to_ansi(&buf))?;
    eprintln!("wrote {}", html_path.display());
    match screenshot(&html_path, &out_dir.join(format!("{stem}.png"))) {
        Ok(png) => eprintln!("wrote {png}"),
        Err(e) => eprintln!("png skipped: {e} (open {stem}.html instead)"),
    }
    Ok(())
}

const CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

fn screenshot(html: &Path, png: &Path) -> Result<String, String> {
    if !Path::new(CHROME).exists() {
        return Err("Chrome not found".into());
    }
    let out = std::process::Command::new(CHROME)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--allow-file-access-from-files",
            &format!("--screenshot={}", png.display()),
            // 120 cols * ~7.8px + margins, 36 rows * 16px + margins.
            "--window-size=1020,660",
            &format!("file://{}", html.canonicalize().map_err(|e| e.to_string())?.display()),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if png.exists() {
        Ok(png.display().to_string())
    } else {
        Err(format!(
            "chrome exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ))
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
        for needle in [
            "GAMEDAY", "FILTER:", "[NFL]", "[NBA]", "[MLB]", "[NHL]", "27 - 24", "88 - 81",
            "5 - 3", "3 - 2", "LAST PLAYS", "MOMENTUM", "RED ZONE", "TICKER", "GLOBAL ALERTS",
            "NAV:",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in board:\n{text}");
        }
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
        assert!(text.contains("33 - 24"), "KC TD score missing:\n{text}");
        assert!(text.contains("TOUCHDOWN"), "TD play missing:\n{text}");
    }

    #[test]
    fn td_tick_dump_captures_the_score_flash() {
        let th = theme::current();
        let live_bg_cells = |tick: u64| {
            let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS, tick).unwrap();
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
