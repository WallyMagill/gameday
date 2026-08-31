//! THROWAWAY style lab (`gameday dump --style-lab`): renders variant PNGs of
//! the spec's §4 style questions so Walter can pick each winner by eye.
//! Nothing here is consumed by the app — once a winner is picked it gets
//! implemented for real in the draw code and this whole module (plus its
//! test file) is deleted.
//!
//! The calm-1/2/3 deck is gone (discipline is a per-theme property) and
//! meter-a/b/c is gone (variant B is the tile's real meter row). The boxed
//! ticker-a/b/c are gone too: the v2.1 spec asks for three NEW directions,
//! none a refinement of the box. What remains:
//!
//!   ticker-d — BottomLine two-lane: lane 1 a continuous compact score strip
//!       of every game in play, lane 2 the scoring alerts; thin rule, no box
//!   ticker-e — LED ribbon: one dark band; each event is a league chip +
//!       team-colored abbr block + scoring word + text
//!   ticker-f — split-flap: a 2×4 board of fixed-width cells that flip when
//!       a new event lands (`ticker-f-flip` is the mid-flip frame at the KC
//!       TD tick; `ticker-f` is settled)
//!   ticker-{d,e,f}-gruvbox — the same three on one community theme
//!
//! These are renders for a decision, not the product ticker: `App::draw_ticker`
//! stays untouched. The lab reads the sim directly (`Simulator`), so it needs
//! no App and the flip can be driven by *when* each event arrived.

use crate::domain::{Game, League, Play, Status};
use crate::dump::{self, Page};
use crate::sim::{Simulator, KC_TD_TICK};
use crate::theme;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Block;
use ratatui::Terminal;
use std::path::Path;

/// One lab render. Same shape as a gallery capture.
pub struct LabCapture {
    pub stem: &'static str,
    pub cols: u16,
    pub rows: u16,
    /// Built-in theme the strip was rendered under (also the page ground).
    pub theme: &'static str,
    pub buf: Buffer,
}

/// The seven captures, in write order. Stems are the file names the task
/// contract fixes. `ticker-f-flip` is always the [`KC_TD_TICK`] frame — the
/// one tick where a new event lands and every cell flips — regardless of
/// `tick`, so the decision always gets a mid-flip frame.
pub fn captures(tick: u64) -> Vec<LabCapture> {
    let prev = theme::current_name();
    let mut caps = Vec::new();
    for (name, suffix) in [("broadcast", ""), ("gruvbox", "-gruvbox")] {
        theme::set_current(name).expect("built-in themes are always loaded");
        let cap = |stem: &'static str, buf: Buffer| {
            let area = *buf.area();
            LabCapture { stem, cols: area.width, rows: area.height, theme: name, buf }
        };
        match suffix {
            "" => caps.extend([
                cap("ticker-d", ticker_d(tick)),
                cap("ticker-e", ticker_e(tick)),
                cap("ticker-f", ticker_f(tick)),
                cap("ticker-f-flip", ticker_f(KC_TD_TICK)),
            ]),
            _ => caps.extend([
                cap("ticker-d-gruvbox", ticker_d(tick)),
                cap("ticker-e-gruvbox", ticker_e(tick)),
                cap("ticker-f-gruvbox", ticker_f(tick)),
            ]),
        }
    }
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
            theme: c.theme,
            buf: c.buf.clone(),
        })
        .collect()
}

// ------------------------------------------------------------------- feed

/// One scoring event with the tick it first appeared on the board (0 for the
/// seeds) and the game as it stood at that moment.
struct Event {
    game: Game,
    play: Play,
    arrived: u64,
}

/// What the ticker directions draw from at tick N: every live game (League
/// order) and every scoring event, newest arrival first.
struct Feed {
    live: Vec<Game>,
    events: Vec<Event>,
}

fn scoring_plays(boards: &std::collections::HashMap<League, Vec<Game>>) -> Vec<(Game, Play)> {
    let mut out = Vec::new();
    for league in League::ALL {
        for game in boards.get(&league).into_iter().flatten() {
            if game.status != Status::Live {
                continue;
            }
            for play in game.last_plays.iter().filter(|p| p.scoring) {
                out.push((game.clone(), play.clone()));
            }
        }
    }
    out
}

/// Step the sim from 0 to `tick`, noting the tick each scoring event first
/// shows up — that arrival order is the ticker order, and it is what drives
/// the split-flap.
fn feed_at(tick: u64) -> Feed {
    let mut sim = Simulator::new();
    let same = |e: &Event, g: &Game, p: &Play| e.game.id == g.id && e.play.clock == p.clock && e.play.text == p.text;
    let mut events: Vec<Event> = scoring_plays(sim.boards())
        .into_iter()
        .map(|(game, play)| Event { game, play, arrived: 0 })
        .collect();
    for t in 1..=tick {
        sim.step();
        let mut fresh = Vec::new();
        for (game, play) in scoring_plays(sim.boards()) {
            if !events.iter().any(|e| same(e, &game, &play)) {
                fresh.push(Event { game, play, arrived: t });
            }
        }
        events.splice(0..0, fresh);
    }
    let live = League::ALL
        .iter()
        .flat_map(|l| sim.boards().get(l).into_iter().flatten())
        .filter(|g| g.status == Status::Live)
        .cloned()
        .collect();
    Feed { live, events }
}

/// `LEAD-TRAIL LDR` as the app's ticker writes it.
fn leader_score(g: &Game) -> String {
    if g.away_score >= g.home_score {
        format!("{}-{} {}", g.away_score, g.home_score, g.away.abbr)
    } else {
        format!("{}-{} {}", g.home_score, g.away_score, g.home.abbr)
    }
}

fn team_of<'a>(g: &'a Game, abbr: &str) -> Option<&'a crate::domain::Team> {
    [&g.away, &g.home].into_iter().find(|t| t.abbr.eq_ignore_ascii_case(abbr))
}

// ------------------------------------------------------------------ cells

type Cells = Vec<(char, Style)>;

const STRIP_W: u16 = 120;
const STRIP_H: u16 = 6;
/// Blank cells between the tail and the wrapped head of a scrolling lane
/// (the app's ticker uses 10; same value so the lanes scroll like it).
const MARQUEE_GAP: usize = 10;

fn push(cells: &mut Cells, text: &str, style: Style) {
    cells.extend(text.chars().map(|c| (c, style)));
}

/// A 120x6 strip painted in the board background.
fn strip() -> Buffer {
    let th = theme::current();
    let mut term = Terminal::new(TestBackend::new(STRIP_W, STRIP_H)).expect("test backend");
    term.draw(|f| {
        f.render_widget(Block::default().style(Style::default().bg(th.bg).fg(th.fg)), f.area());
    })
    .expect("offscreen strip render cannot fail");
    term.backend().buffer().clone()
}

/// The `width` cells visible at `tick`: the whole row when it fits, else a
/// marquee window that wraps through a gap.
fn window(cells: &[(char, Style)], width: usize, tick: u64, fill: Style) -> Cells {
    if cells.len() <= width {
        let mut out = cells.to_vec();
        out.resize(width, (' ', fill));
        return out;
    }
    let total = cells.len() + MARQUEE_GAP;
    let offset = (tick as usize) % total;
    (0..width)
        .map(|i| cells.get((offset + i) % total).copied().unwrap_or((' ', fill)))
        .collect()
}

/// Write cells at (`x`, `y`). Styles without an explicit background get
/// `fill` so the row reads as one surface.
fn blit_cells(buf: &mut Buffer, x0: u16, y: u16, cells: &[(char, Style)], fill: Style) {
    for (i, (ch, style)) in cells.iter().enumerate() {
        let x = x0 + i as u16;
        if x >= buf.area().width {
            break;
        }
        let style = if style.bg.is_none() { style.patch(fill) } else { *style };
        buf[(x, y)].set_char(*ch).set_style(style);
    }
}

/// Text color that reads on a team-colored block: the page ground on a light
/// block, the brightest fg on a dark one (Rec. 601 luma, 140 picked by eye
/// against the demo palette's NYY white and TB red).
fn on_block(rgb: [u8; 3]) -> Color {
    let th = theme::current();
    let luma = (299 * rgb[0] as u32 + 587 * rgb[1] as u32 + 114 * rgb[2] as u32) / 1000;
    if luma > 140 { th.bg } else { th.bright }
}

// ---------------------------------------------------- d — BottomLine lanes

/// Lane gutter labels; the only chrome besides the rule.
const LANE_LABELS: [&str; 2] = [" SCORES ", " ALERTS "];

/// (d) ESPN BottomLine: a thin rule, then two unboxed lanes. Lane 1 is every
/// game in play as `NFL KC 27 TB 24 Q4 1:27`, lane 2 is the scoring alerts;
/// both marquee when they overflow.
pub fn ticker_d(tick: u64) -> Buffer {
    let th = theme::current();
    let feed = feed_at(tick);
    let mut buf = strip();
    let ground = Style::default().bg(th.bg);
    let sep = Style::default().fg(th.dim);

    let mut lane1: Cells = Vec::new();
    for g in &feed.live {
        if !lane1.is_empty() {
            push(&mut lane1, " │ ", sep);
        }
        push(&mut lane1, g.league.slug().to_uppercase().as_str(), Style::default().fg(th.chip(g.league)).add_modifier(Modifier::BOLD));
        push(&mut lane1, &format!(" {} ", g.away.abbr), Style::default().fg(th.fg));
        push(&mut lane1, &g.away_score.to_string(), Style::default().fg(th.bright).add_modifier(Modifier::BOLD));
        push(&mut lane1, &format!(" {} ", g.home.abbr), Style::default().fg(th.fg));
        push(&mut lane1, &g.home_score.to_string(), Style::default().fg(th.bright).add_modifier(Modifier::BOLD));
        let when = format!("{} {}", g.period, g.clock);
        push(&mut lane1, &format!(" {}", when.trim()), Style::default().fg(th.clock()));
    }

    let mut lane2: Cells = Vec::new();
    for e in &feed.events {
        if !lane2.is_empty() {
            push(&mut lane2, " │ ", sep);
        }
        let team = team_of(&e.game, &e.play.team).map_or(th.fg, |t| th.team_text(t.color));
        push(&mut lane2, &format!("{} ", e.play.clock), Style::default().fg(th.clock()));
        push(&mut lane2, &format!("{} ", e.play.team), Style::default().fg(team).add_modifier(Modifier::BOLD));
        push(&mut lane2, &format!("{} ", theme::scoring_word(e.game.league)), Style::default().fg(th.live).add_modifier(Modifier::BOLD));
        push(&mut lane2, &e.play.text, Style::default().fg(th.fg));
        push(&mut lane2, &format!(" {}", leader_score(&e.game)), Style::default().fg(th.bright));
    }

    let mut rule: Cells = Vec::new();
    push(&mut rule, &"─".repeat(STRIP_W as usize), sep);
    blit_cells(&mut buf, 0, 1, &rule, ground);
    let gutter = LANE_LABELS[0].chars().count() as u16;
    let content_w = (STRIP_W - gutter - 1) as usize;
    for (i, (label, lane)) in LANE_LABELS.iter().zip([&lane1, &lane2]).enumerate() {
        let y = 2 + i as u16;
        let mut cells: Cells = Vec::new();
        push(&mut cells, label, Style::default().fg(th.muted));
        blit_cells(&mut buf, 0, y, &cells, ground);
        blit_cells(&mut buf, gutter, y, &window(lane, content_w, tick, ground), ground);
    }
    buf
}

// -------------------------------------------------------- e — LED ribbon

/// (e) A stadium LED ribbon: one row on a dim band. Each event is a league
/// chip (its accent as a block), the team abbr on a team-colored block, the
/// scoring word in `live`, then the text in `star` — amber matrix text.
pub fn ticker_e(tick: u64) -> Buffer {
    let th = theme::current();
    let feed = feed_at(tick);
    let mut buf = strip();
    let band = Style::default().bg(th.dim);
    let mut cells: Cells = Vec::new();
    let pill = |cells: &mut Cells, text: &str, block: Color, ink: Color| {
        // Half-blocks whose colored halves abut the text block, so each chip
        // reads as one solid LED tile with soft ends: ▐NFL▌.
        push(cells, "▐", Style::default().fg(block).bg(th.dim));
        push(cells, text, Style::default().fg(ink).bg(block).add_modifier(Modifier::BOLD));
        push(cells, "▌", Style::default().fg(block).bg(th.dim));
    };
    for e in &feed.events {
        if !cells.is_empty() {
            push(&mut cells, "   ", band);
        }
        pill(&mut cells, &e.game.league.slug().to_uppercase(), th.chip(e.game.league), th.bg);
        push(&mut cells, " ", band);
        let (block, ink) = team_of(&e.game, &e.play.team)
            .map_or((th.border, th.bright), |t| (theme::rgb(t.color), on_block(t.color)));
        pill(&mut cells, &e.play.team, block, ink);
        push(&mut cells, &format!(" {} ", theme::scoring_word(e.game.league)), Style::default().fg(th.live).bg(th.dim).add_modifier(Modifier::BOLD));
        push(&mut cells, &e.play.text, Style::default().fg(th.star).bg(th.dim));
        push(&mut cells, &format!("  {}", leader_score(&e.game)), Style::default().fg(th.bright).bg(th.dim));
    }
    let y = 2;
    blit_cells(&mut buf, 0, y, &window(&cells, STRIP_W as usize, tick, band), band);
    buf
}

// -------------------------------------------------------- f — split-flap

/// Cells per row and rows on the board: 4 × 30 columns fill the strip.
const FLAP_COLS: usize = 4;
const FLAP_ROWS: usize = 2;
const FLAP_W: usize = 30;
/// Glyphs a flap module rolls through (every glyph the cell text emits, so
/// a roll never has to jump).
const FLAP_ALPHABET: &str = " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:-'!";
/// Ticks until every module has settled. The demo's scoring events land as
/// close as 3 ticks apart (KC TD at 15, BOS at 18), so a flip longer than 2
/// ticks would never be seen settled between them; 2 also matches a real
/// flap module (~1-2 s). Tests use it to find the settled frame.
pub const FLIP_TICKS: u64 = 2;
/// Glyphs a rolling module advances per tick: alphabet (41) / FLIP_TICKS,
/// rounded up, so the farthest glyph lands exactly at FLIP_TICKS.
const FLAP_STEP: usize = 21;

/// Two-letter-ish scoring code for a flap cell.
fn scoring_code(league: League) -> &'static str {
    match league {
        League::Nfl | League::Cfb => "TD",
        League::Nba | League::Wnba | League::Cbb => "BKT",
        League::Mlb => "HR",
        League::Nhl | League::Epl | League::Mls => "GOAL",
    }
}

/// The 29 content cells of one flap, styled as if settled. `None` = blank flap.
fn flap_cells(e: Option<&Event>) -> Cells {
    let th = theme::current();
    let tile = Style::default().bg(th.dim);
    let mut cells: Cells = Vec::new();
    let Some(e) = e else {
        cells.resize(FLAP_W - 1, (' ', tile));
        return cells;
    };
    let g = &e.game;
    let (lead, trail, ldr) = if g.away_score >= g.home_score {
        (g.away_score, g.home_score, g.away.abbr.as_str())
    } else {
        (g.home_score, g.away_score, g.home.abbr.as_str())
    };
    let ldr = if lead == trail { "TIE" } else { ldr };
    push(&mut cells, &format!(" {:>5}  ", e.play.clock), tile.fg(th.clock()));
    push(&mut cells, &format!("{:<3}  ", e.play.team), tile.fg(th.bright).add_modifier(Modifier::BOLD));
    push(&mut cells, &format!("{:<4}  ", scoring_code(g.league)), tile.fg(th.live).add_modifier(Modifier::BOLD));
    push(&mut cells, &format!("{:>5} ", format!("{lead}-{trail}")), tile.fg(th.bright));
    push(&mut cells, &format!("{:<3} ", ldr), tile.fg(th.fg));
    debug_assert_eq!(cells.len(), FLAP_W - 1, "flap content is a fixed width");
    cells.truncate(FLAP_W - 1);
    cells
}

/// One board = FLAP_ROWS × FLAP_COLS flaps for the newest events.
fn board_cells(events: &[Event]) -> Vec<Cells> {
    (0..FLAP_ROWS * FLAP_COLS).map(|i| flap_cells(events.get(i))).collect()
}

/// A module `progress` ticks into a flip from `old` to `new`: glyphs that
/// changed roll forward through the alphabet at FLAP_STEP per tick (drawn
/// muted while moving) and stop on their target.
fn roll(old: &[(char, Style)], new: &[(char, Style)], progress: u64) -> Cells {
    let th = theme::current();
    let alphabet: Vec<char> = FLAP_ALPHABET.chars().collect();
    let idx = |c: char| alphabet.iter().position(|&a| a == c).unwrap_or(0);
    let moved = progress as usize * FLAP_STEP;
    new.iter()
        .enumerate()
        .map(|(i, &(target, style))| {
            let from = old.get(i).map_or(' ', |c| c.0);
            if from == target {
                return (target, style);
            }
            let (i0, i1) = (idx(from), idx(target));
            let dist = (i1 + alphabet.len() - i0) % alphabet.len();
            if moved >= dist {
                (target, style)
            } else {
                (alphabet[(i0 + moved) % alphabet.len()], Style::default().fg(th.muted).bg(th.dim))
            }
        })
        .collect()
}

/// (f) A departure-board of flap cells: `│ 3:21  KC   TD    27-24 KC  `.
/// When an event lands every module flips; the frame at the landing tick
/// (progress 1) is mid-roll, and by `FLIP_TICKS` later it has settled.
pub fn ticker_f(tick: u64) -> Buffer {
    let th = theme::current();
    let feed = feed_at(tick);
    let mut buf = strip();
    let new = board_cells(&feed.events);
    let last_change = feed.events.iter().map(|e| e.arrived).max().unwrap_or(0);
    let progress = tick - last_change + 1;
    let shown: Vec<Cells> = if last_change > 0 && progress <= FLIP_TICKS {
        let old_events: Vec<&Event> = feed.events.iter().filter(|e| e.arrived < last_change).collect();
        let old: Vec<Cells> = (0..FLAP_ROWS * FLAP_COLS)
            .map(|i| flap_cells(old_events.get(i).copied()))
            .collect();
        new.iter().zip(&old).map(|(n, o)| roll(o, n, progress)).collect()
    } else {
        new
    };
    let divider = Style::default().fg(th.border).bg(th.bg);
    for (i, cells) in shown.iter().enumerate() {
        let y = 2 + (i / FLAP_COLS) as u16;
        let x = ((i % FLAP_COLS) * FLAP_W) as u16;
        buf[(x, y)].set_char('│').set_style(divider);
        blit_cells(&mut buf, x + 1, y, cells, Style::default().bg(th.dim));
    }
    buf
}
