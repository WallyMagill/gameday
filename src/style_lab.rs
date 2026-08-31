//! THROWAWAY style lab (`gameday dump --style-lab`): renders variant PNGs of
//! the spec's §4 style questions so Walter can pick each winner by eye.
//! Nothing here is consumed by the app — once a winner is picked it gets
//! implemented for real in the draw code and this whole module (plus its
//! test file) is deleted.
//!
//! The calm-1/2/3 deck is gone: color discipline became a per-theme property
//! (`theme::Discipline`; `broadcast` is level 1, `studio` is level 3), so the
//! gallery's `board-broadcast` / `board-studio` are those renders now. The
//! meter-a/b/c strip is gone too: variant B (the inline gauge row) won and is
//! the tile's real meter now (`tiles::meter_line`).
//!
//!   ticker-a/b/c — ticker strips: (a) current boxed 2-row, (b) single-row
//!       solid-red `▌TICKER▐` pill + unboxed marquee, (c) 2-row framed by dim
//!       rules instead of the red box
//!
//! Variants are produced by buffer surgery on real renders (recoloring runs,
//! stitching regions, redrawing small areas) rather than by forking the draw
//! code — the production renderers stay untouched, and deleting this file
//! removes the lab completely.

use crate::dump::{self, Page, DUMP_COLS, DUMP_ROWS};
use crate::theme;
use crate::tiles::ScoreStyle;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Block;
use ratatui::Terminal;
use std::path::Path;

/// One lab render. Same shape as a gallery capture, always broadcast theme.
pub struct LabCapture {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    pub buf: Buffer,
}

/// The three variants, in write order. Stems are the file names the task
/// contract fixes (`ticker-a.png` … `ticker-c.png`).
pub fn captures(tick: u64) -> Vec<LabCapture> {
    let prev = theme::current_name();
    theme::set_current("broadcast").expect("broadcast is always loaded");
    let board = dump::render_demo_buffer(DUMP_COLS, DUMP_ROWS, tick, ScoreStyle::Big)
        .expect("offscreen board render cannot fail");
    let cap = |stem, buf: Buffer| {
        let area = *buf.area();
        LabCapture { stem, cols: area.width, rows: area.height, buf }
    };
    let caps = vec![
        cap("ticker-a", ticker_a(&board)),
        cap("ticker-b", ticker_b(&board)),
        cap("ticker-c", ticker_c(&board)),
    ];
    theme::set_current(&prev).expect("the previous theme is still loaded");
    caps
}

/// Write HTML + ANSI pages for every capture (the offline half of `run`).
pub fn write_pages(out_dir: &Path, caps: &[LabCapture]) -> std::io::Result<()> {
    dump::write_pages(out_dir, &pages_of(caps))
}

/// `dump --style-lab`: pages, then PNGs via the shared Chrome pipeline, then
/// the same loud completeness check the gallery uses.
pub fn run(out_dir: &Path, tick: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let caps = captures(tick);
    let pages = pages_of(&caps);
    dump::write_pages(out_dir, &pages)?;
    let chrome = dump::screenshot_pages(out_dir, &pages);
    dump::verify_pages(out_dir, &pages, chrome)
}

fn pages_of(caps: &[LabCapture]) -> Vec<Page> {
    caps.iter()
        .map(|c| Page {
            stem: c.stem,
            cols: c.cols,
            rows: c.rows,
            theme: "broadcast",
            buf: c.buf.clone(),
        })
        .collect()
}

// ------------------------------------------------------------- ticker strips

const STRIP_W: u16 = 120;
const TICKER_STRIP_H: u16 = 6;
/// Where App::draw lays the 4-row ticker at 120x36: header 1 + body 30.
const TICKER_Y: u16 = 31;
/// The " GAMEDAY  " / " TICKER   " label column width inside the box.
const LABEL_W: u16 = 10;

/// A 120x`rows` strip painted in the board background.
fn strip(rows: u16) -> Buffer {
    let th = theme::current();
    let mut term = Terminal::new(TestBackend::new(STRIP_W, rows)).expect("test backend");
    term.draw(|f| {
        f.render_widget(Block::default().style(Style::default().bg(th.bg).fg(th.fg)), f.area());
    })
    .expect("offscreen strip render cannot fail");
    term.backend().buffer().clone()
}

/// (a) The current boxed 2-row ticker, cropped straight out of the board.
fn ticker_a(board: &Buffer) -> Buffer {
    let mut buf = strip(TICKER_STRIP_H);
    blit(&mut buf, 0, 1, board, Rect::new(0, TICKER_Y, STRIP_W, 4));
    buf
}

/// (b) Single row: a solid-red `▌TICKER▐` pill, then the marquee unboxed —
/// the real content cells of both ticker rows joined into one stream.
fn ticker_b(board: &Buffer) -> Buffer {
    let th = theme::current();
    let mut buf = strip(TICKER_STRIP_H);
    let y = 2;
    let mut x = 1u16;
    put(&mut buf, &mut x, y, "▌", Style::default().fg(th.live));
    put(
        &mut buf,
        &mut x,
        y,
        "TICKER",
        Style::default().fg(th.bg).bg(th.live).add_modifier(Modifier::BOLD),
    );
    put(&mut buf, &mut x, y, "▐ ", Style::default().fg(th.live));
    // Content cells sit inside the box (rows TICKER_Y+1/+2), after the label.
    let content_x = 1 + LABEL_W..STRIP_W - 1;
    let mut push = |buf: &mut Buffer, cell: ratatui::buffer::Cell| {
        if x < STRIP_W - 1 {
            buf[(x, y)] = cell;
            x += 1;
        }
    };
    for cx in content_x.clone() {
        push(&mut buf, board[(cx, TICKER_Y + 1)].clone());
    }
    for ch in "  |  ".chars() {
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_char(ch).set_style(Style::default().fg(th.dim).bg(th.bg));
        push(&mut buf, cell);
    }
    for cx in content_x {
        push(&mut buf, board[(cx, TICKER_Y + 2)].clone());
    }
    buf
}

/// (c) The 2 labeled rows kept, but framed by dim rules instead of a red box.
fn ticker_c(board: &Buffer) -> Buffer {
    let th = theme::current();
    let mut buf = strip(TICKER_STRIP_H);
    let dim = Style::default().fg(th.dim);
    let mut cx = 0;
    put(&mut buf, &mut cx, 1, &"─".repeat(STRIP_W as usize), dim);
    blit(&mut buf, 1, 2, board, Rect::new(1, TICKER_Y + 1, STRIP_W - 2, 2));
    let mut cx = 0;
    put(&mut buf, &mut cx, 4, &"─".repeat(STRIP_W as usize), dim);
    buf
}

// ------------------------------------------------------------ buffer helpers

/// Copy `src_rect` of `src` to (`dx`, `dy`) in `dst`.
fn blit(dst: &mut Buffer, dx: u16, dy: u16, src: &Buffer, src_rect: Rect) {
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            dst[(dx + x, dy + y)] = src[(src_rect.x + x, src_rect.y + y)].clone();
        }
    }
}

/// Write `s` at (`x`, `y`); advances `x`. Styles without an explicit
/// background get the board background so strips read as one surface.
fn put(buf: &mut Buffer, x: &mut u16, y: u16, s: &str, style: Style) {
    let th = theme::current();
    let style = if style.bg.is_none() { style.bg(th.bg) } else { style };
    buf.set_string(*x, y, s, style);
    *x += s.chars().count() as u16;
}
