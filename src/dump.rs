//! `gameday dump`: render the demo board offscreen at 120x36 and write
//! HTML + ANSI captures (plus a PNG when headless Chrome is available).
//! This is the visual iteration loop — compare out/board.png to the reference.

use crate::app::App;
use crate::demo;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::{Path, PathBuf};

pub const DUMP_COLS: u16 = 120;
pub const DUMP_ROWS: u16 = 36;

pub fn demo_app(config_dir: PathBuf) -> App {
    let mut app = App::new(demo::demo_config(), demo::demo_pins(), config_dir);
    for (league, games) in demo::demo_boards() {
        app.apply_boards(league, games, false);
    }
    app
}

pub fn render_demo_buffer(cols: u16, rows: u16) -> std::io::Result<Buffer> {
    let dir = std::env::temp_dir().join(format!("gameday-dump-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut app = demo_app(dir);
    let mut term = Terminal::new(TestBackend::new(cols, rows))?;
    term.draw(|f| app.draw(f))?;
    Ok(term.backend().buffer().clone())
}

pub fn run(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS)?;
    let html_path = out_dir.join("board.html");
    std::fs::write(&html_path, buffer_to_html(&buf))?;
    std::fs::write(out_dir.join("board.ansi"), buffer_to_ansi(&buf))?;
    eprintln!("wrote {}", html_path.display());
    match screenshot(&html_path, &out_dir.join("board.png")) {
        Ok(png) => eprintln!("wrote {png}"),
        Err(e) => eprintln!("png skipped: {e} (open board.html instead)"),
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
    let mut html = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>gameday board</title>
<style>
  {font_face}html,body{{margin:0;background:#0a0a0a;}}
  pre{{font:13px/16px 'DumpMono',Menlo,"Cascadia Mono","SF Mono",ui-monospace,monospace;
      margin:16px;padding:10px 12px;background:#0a0a0a;color:#c8c8c8;
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
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS).unwrap();
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
    fn html_dump_contains_colored_cells() {
        let buf = render_demo_buffer(DUMP_COLS, DUMP_ROWS).unwrap();
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
