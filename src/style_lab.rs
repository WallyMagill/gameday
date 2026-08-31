//! THROWAWAY style lab (`gameday dump --style-lab`): renders nine variant
//! PNGs of the spec's §4 style questions so Walter can pick each winner by
//! eye. Nothing here is consumed by the app — once a winner is picked it gets
//! implemented for real in the draw code and this whole module (plus its
//! test file) is deleted.
//!
//!   calm-1/2/3 — three color-discipline levels of the broadcast board:
//!       (1) current, (2) league accents only on chips + sidebar headers on a
//!       single accent + clocks gray, (3) additionally play abbrs/team text
//!       gray — only scores, logos, and LIVE/scoring stay colored
//!   meter-a/b/c — meter-column redesigns on a two-tile strip (NFL red zone +
//!       NBA lead): (a) current right column, (b) borderless inline gauge
//!       under the identity block, (c) right column with a framed
//!       single-glyph gauge + stacked label
//!   ticker-a/b/c — ticker strips: (a) current boxed 2-row, (b) single-row
//!       solid-red `▌TICKER▐` pill + unboxed marquee, (c) 2-row framed by dim
//!       rules instead of the red box
//!
//! Variants are produced by buffer surgery on real renders (recoloring runs,
//! stitching regions, redrawing small areas) rather than by forking the draw
//! code — the production renderers stay untouched, and deleting this file
//! removes the lab completely.

use crate::domain::{Game, League, Meter};
use crate::dump::{self, Page, DUMP_COLS, DUMP_ROWS};
use crate::theme::{self, ThemeName};
use crate::tiles::{render_tile, Density, ScoreStyle, TileFx};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Block;
use ratatui::Terminal;
use std::path::Path;

/// One lab render. Same shape as a gallery capture, always broadcast theme —
/// the calm levels are levels *of the broadcast board*.
pub struct LabCapture {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    pub buf: Buffer,
}

/// The nine promised variants, in write order. Stems are the file names the
/// task contract fixes (`calm-1.png` … `ticker-c.png`).
pub fn captures(tick: u64) -> Vec<LabCapture> {
    let prev = theme::current_name();
    theme::set_current(ThemeName::Broadcast);
    let board = dump::render_demo_buffer(DUMP_COLS, DUMP_ROWS, tick, ScoreStyle::Big)
        .expect("offscreen board render cannot fail");
    let calm2 = calm_level_2(&board);
    let calm3 = calm_level_3(&calm2, tick);
    let (nfl, nba) = meter_games(tick);
    let cap = |stem, buf: Buffer| {
        let area = *buf.area();
        LabCapture { stem, cols: area.width, rows: area.height, buf }
    };
    let caps = vec![
        cap("calm-1", board.clone()),
        cap("calm-2", calm2),
        cap("calm-3", calm3),
        cap("meter-a", meter_strip(&nfl, &nba, tile_a)),
        cap("meter-b", meter_strip(&nfl, &nba, tile_b)),
        cap("meter-c", meter_strip(&nfl, &nba, tile_c)),
        cap("ticker-a", ticker_a(&board)),
        cap("ticker-b", ticker_b(&board)),
        cap("ticker-c", ticker_c(&board)),
    ];
    theme::set_current(prev);
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
            theme: ThemeName::Broadcast,
            buf: c.buf.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------- calm levels

/// Level 2: chrome discipline. Clock runs (cyan text containing digits and a
/// colon) go gray; league-accent runs that aren't a `[CHIP]` go gray; the
/// three sidebar headers collapse onto one accent family (star).
fn calm_level_2(board: &Buffer) -> Buffer {
    let th = theme::current();
    let mut buf = board.clone();
    let area = *buf.area();
    let accents: Vec<Color> = League::ALL.iter().map(|l| th.league_accent(*l)).collect();
    for y in 0..area.height {
        let mut x = 0;
        while x < area.width {
            let fg = buf[(x, y)].fg;
            let mut end = x;
            let mut text = String::new();
            while end < area.width && buf[(end, y)].fg == fg {
                text.push_str(buf[(end, y)].symbol());
                end += 1;
            }
            let is_clock =
                fg == th.cyan && text.contains(':') && text.chars().any(|c| c.is_ascii_digit());
            // Chips are the one place a league accent survives; `[` marks
            // them ([NFL] on tile borders, [NHL] etc.).
            let is_stray_accent = accents.contains(&fg) && !text.contains('[');
            if is_clock || is_stray_accent {
                for cx in x..end {
                    buf[(cx, y)].fg = th.muted;
                }
            }
            x = end;
        }
    }
    for needle in ["⚑ GLOBAL ALERTS", "TOP PLAYS", "RECORDS"] {
        recolor_text(&mut buf, needle, th.star);
    }
    buf
}

/// Level 3 (on top of level 2): team colors survive only on score digits and
/// logo art (block/sextant glyphs) — every team-colored *text* cell (play
/// abbrs, momentum arrows, sidebar names, ticker abbrs) goes gray.
fn calm_level_3(calm2: &Buffer, tick: u64) -> Buffer {
    let th = theme::current();
    let mut buf = calm2.clone();
    let mut team_colors = Vec::new();
    for (_, games) in crate::sim::Simulator::boards_at(tick) {
        for g in games {
            for c in [g.away.color, g.home.color] {
                let rgb = theme::rgb(c);
                team_colors.push(rgb);
                team_colors.push(theme::dimmed(rgb)); // momentum's cold side
            }
        }
    }
    let area = *buf.area();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &mut buf[(x, y)];
            let glyph = cell
                .symbol()
                .chars()
                .next()
                .is_some_and(|c| matches!(c as u32, 0x2580..=0x259F | 0x1FB00..=0x1FBFF));
            if !glyph && team_colors.contains(&cell.fg) {
                cell.fg = th.fg;
            }
        }
    }
    buf
}

/// Recolor every occurrence of `needle` (used for the sidebar headers, whose
/// texts are unique on the board).
fn recolor_text(buf: &mut Buffer, needle: &str, fg: Color) {
    let area = *buf.area();
    for y in 0..area.height {
        let row: String = (0..area.width)
            .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect();
        if let Some(byte_pos) = row.find(needle) {
            let start = row[..byte_pos].chars().count() as u16;
            for x in start..start + needle.chars().count() as u16 {
                buf[(x, y)].fg = fg;
            }
        }
    }
}

// -------------------------------------------------------------- meter strips

const STRIP_W: u16 = 120;
const TILE_W: u16 = 60;
const TILE_H: u16 = 18;
/// render_tile's identity block height (IDENTITY_H in tiles/mod.rs): the
/// first lower row of a 60x18 standard tile is inner row 7.
const LOWER_Y: u16 = 7;
/// render_tile's meter column width (METER_W in tiles/mod.rs).
const METER_W: u16 = 9;

/// The two demo live games whose meters the redesigns are judged on.
fn meter_games(tick: u64) -> (Game, Game) {
    let boards = crate::sim::Simulator::boards_at(tick);
    let find = |league: League, id: &str| {
        boards
            .get(&league)
            .and_then(|gs| gs.iter().find(|g| g.id == id))
            .cloned()
            .unwrap_or_else(|| panic!("demo board is missing {id:?} at tick {tick}"))
    };
    (find(League::Nfl, "nfl-live"), find(League::Nba, "nba-live"))
}

/// Two tiles side by side, each drawn by `tile` — the same games in every
/// variant so only the meter treatment differs.
fn meter_strip(nfl: &Game, nba: &Game, tile: fn(&Game) -> Buffer) -> Buffer {
    let mut strip = Buffer::empty(Rect::new(0, 0, STRIP_W, TILE_H));
    blit(&mut strip, 0, 0, &tile(nfl), Rect::new(0, 0, TILE_W, TILE_H));
    blit(&mut strip, TILE_W, 0, &tile(nba), Rect::new(0, 0, TILE_W, TILE_H));
    strip
}

/// One standard tile rendered exactly as the live board draws it.
fn tile_buf(game: &Game, w: u16, h: u16) -> Buffer {
    let th = theme::current();
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
    term.draw(|f| {
        f.render_widget(Block::default().style(Style::default().bg(th.bg).fg(th.fg)), f.area());
        render_tile(f, f.area(), game, Density::Standard, false, TileFx::default(), ScoreStyle::Big);
    })
    .expect("offscreen tile render cannot fail");
    term.backend().buffer().clone()
}

/// Variant a: the current design, untouched.
fn tile_a(game: &Game) -> Buffer {
    tile_buf(game, TILE_W, TILE_H)
}

/// Variant b: no right column — a borderless one-row gauge sits directly
/// under the identity block and the plays feed gets the full tile width.
/// Built by stitching: the lower-left of a 9-cols-wider render of the same
/// tile is exactly full-inner-width here, so momentum/rule/plays keep their
/// real look at the new width.
fn tile_b(game: &Game) -> Buffer {
    let th = theme::current();
    let mut a = tile_buf(game, TILE_W, TILE_H);
    let wide = tile_buf(game, TILE_W + METER_W, TILE_H);
    let inner_w = TILE_W - 2; // 58
    // Gauge row replaces the first lower row (was momentum, which moves down).
    fill(&mut a, 1, LOWER_Y, inner_w, 1, th.bg);
    // Wide render's lower-left (58 cols) fills rows LOWER_Y+1..bottom border.
    blit(
        &mut a,
        1,
        LOWER_Y + 1,
        &wide,
        Rect::new(1, LOWER_Y, inner_w, TILE_H - LOWER_Y - 2),
    );
    inline_gauge(&mut a, 1, LOWER_Y, inner_w as usize, game);
    a
}

/// Variant c: the right column stays, but the gauge is framed in its own box
/// with the label stacked over a single-glyph track.
fn tile_c(game: &Game) -> Buffer {
    let th = theme::current();
    let mut a = tile_buf(game, TILE_W, TILE_H);
    let x0 = TILE_W - 1 - METER_W; // meter column start (col 50)
    let box_h = TILE_H - LOWER_Y - 1; // rows LOWER_Y..bottom border (10)
    fill(&mut a, x0, LOWER_Y, METER_W, box_h, th.bg);
    framed_gauge(&mut a, x0, LOWER_Y, METER_W, box_h, game);
    a
}

/// Borderless inline gauge: `LABEL track● tail` on one row, full width.
fn inline_gauge(buf: &mut Buffer, x: u16, y: u16, w: usize, game: &Game) {
    let th = theme::current();
    match &game.meter {
        Some(Meter::RedZone { yards_to_goal }) => {
            let label = " RED ZONE  ";
            let tail = format!("  G  {yards_to_goal} TO GOAL");
            let bar_w = w.saturating_sub(label.chars().count() + tail.chars().count());
            let ytg = (*yards_to_goal).min(20) as usize;
            let filled = (bar_w.saturating_sub(1)) * (20 - ytg) / 20;
            let mut cx = x;
            put(buf, &mut cx, y, label, Style::default().fg(th.live).add_modifier(Modifier::BOLD));
            put(buf, &mut cx, y, &"━".repeat(filled), Style::default().fg(th.live));
            put(buf, &mut cx, y, "●", Style::default().fg(th.live).add_modifier(Modifier::BOLD));
            put(
                buf,
                &mut cx,
                y,
                &"─".repeat(bar_w.saturating_sub(filled + 1)),
                Style::default().fg(th.dim),
            );
            put(buf, &mut cx, y, &tail, Style::default().fg(th.muted));
        }
        Some(Meter::Lead { plus_minus }) => {
            let accent = th.league_accent(game.league);
            let label = " LEAD  -15 ";
            let pm = i32::from(*plus_minus).clamp(-15, 15);
            let tail = format!(" +15  {pm:+}");
            let bar_w = w.saturating_sub(label.chars().count() + tail.chars().count());
            let pos = ((pm + 15) as usize * bar_w.saturating_sub(1)) / 30;
            let marker = if pm >= 0 { th.green } else { th.live };
            let mut cx = x;
            put(buf, &mut cx, y, label, Style::default().fg(accent).add_modifier(Modifier::BOLD));
            for i in 0..bar_w {
                let (sym, style) = if i == pos {
                    ("█", Style::default().fg(marker).add_modifier(Modifier::BOLD))
                } else if i == bar_w / 2 {
                    ("┼", Style::default().fg(th.muted))
                } else {
                    ("─", Style::default().fg(th.dim))
                };
                put(buf, &mut cx, y, sym, style);
            }
            put(buf, &mut cx, y, &tail, Style::default().fg(accent).add_modifier(Modifier::BOLD));
        }
        _ => {}
    }
}

/// Framed single-glyph gauge: a dim box, the label stacked one word per row,
/// a one-character track with the position glyph, the value at the bottom.
fn framed_gauge(buf: &mut Buffer, x0: u16, y0: u16, w: u16, h: u16, game: &Game) {
    let th = theme::current();
    let dim = Style::default().fg(th.dim);
    let inner_w = (w - 2) as usize;
    // Box.
    let mut cx = x0;
    put(buf, &mut cx, y0, &format!("┌{}┐", "─".repeat(inner_w)), dim);
    for y in y0 + 1..y0 + h - 1 {
        let mut cx = x0;
        put(buf, &mut cx, y, "│", dim);
        let mut cx = x0 + w - 1;
        put(buf, &mut cx, y, "│", dim);
    }
    let mut cx = x0;
    put(buf, &mut cx, y0 + h - 1, &format!("└{}┘", "─".repeat(inner_w)), dim);
    let centered = |buf: &mut Buffer, y: u16, s: &str, style: Style| {
        let mut cx = x0 + 1 + ((inner_w.saturating_sub(s.chars().count())) / 2) as u16;
        put(buf, &mut cx, y, s, style);
    };
    let track_h = (h.saturating_sub(5)) as usize; // label 2 + borders 2 + value 1
    let (label, value, pos) = match &game.meter {
        Some(Meter::RedZone { yards_to_goal }) => {
            let ytg = (*yards_to_goal).min(20) as usize;
            (
                ("RED", "ZONE", Style::default().fg(th.live).add_modifier(Modifier::BOLD)),
                (format!("{ytg} YD"), th.live),
                (20 - ytg) * track_h.saturating_sub(1) / 20,
            )
        }
        Some(Meter::Lead { plus_minus }) => {
            let accent = th.league_accent(game.league);
            let pm = i32::from(*plus_minus).clamp(-15, 15);
            (
                ("LEAD", "METER", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
                (format!("{pm:+}"), if pm >= 0 { th.green } else { th.live }),
                // +15 rounds to nearest so a small lead visibly leaves center.
                ((15 - pm) as usize * track_h.saturating_sub(1) + 15) / 30,
            )
        }
        _ => return,
    };
    centered(buf, y0 + 1, label.0, label.2);
    centered(buf, y0 + 2, label.1, label.2);
    let track_top = y0 + 3;
    for (i, y) in (track_top..track_top + track_h as u16).enumerate() {
        if i == pos {
            centered(buf, y, "█", Style::default().fg(value.1).add_modifier(Modifier::BOLD));
        } else {
            centered(buf, y, "┆", dim);
        }
    }
    centered(
        buf,
        y0 + h - 2,
        &value.0,
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
    );
}

// ------------------------------------------------------------- ticker strips

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

/// Blank a region to the board background.
fn fill(buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16, bg: Color) {
    for cy in y..y + h {
        for cx in x..x + w {
            let cell = &mut buf[(cx, cy)];
            cell.reset();
            cell.set_style(Style::default().bg(bg));
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
