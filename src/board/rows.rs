//! The three row tiers of the ranked board: everything under the
//! hero is one ranked list, and a game's tier is how loudly that list says
//! it. Tier 1 is a three-row block, tier 2 is one line, tier 3 is one dim
//! line for finals and later games. All three print the score as a plain
//! numeral in the same column, and all three clock in the same column too:
//! tier 1 is the same grid with height,
//! not a grid of its own.
//!
//! Three rules hold across all three tiers:
//!
//! * **Two gutters, always reserved** ([`GUTTER`]). Column 0 is the hot mark
//!   — `▌` in `hot` when the row is hot, `dim` when it isn't, and *only*
//!   those two states. Columns 2–4 are the nudge. A row that rises shows
//!   `↑2` there and does not move by a single cell (A′ calls #6/#7: rank and
//!   hotness are different facts, so a nudge may never restyle the mark or
//!   reflow the line).
//! * **Amber is the score's color.** `roles.digits`, bold, and nothing else
//!   in a row wears it. The one exception to the monochrome rest is a pinned
//!   game's abbrs, and only at `TeamColorScope::HeroMarks`.
//! * **Fixed columns, dropped from the right.** The grid below is measured
//!   off the A′ frame; a column whose x is past the area's edge is simply not
//!   drawn, which is how a row narrows instead of wrapping or clipping mid-word.
//!
//! Nothing here reads `App`, a clock, or a tick: hotness, the nudge, pin
//! state and `now` all arrive in [`RowCtx`].

use crate::domain::{Game, GameStats, League, Status};
use crate::text::{fmt_start, truncate};
use crate::theme::{self, Roles, TeamColorScope};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use time::OffsetDateTime;

/// Everything a row draws that isn't in the `Game`. The caller owns all of
/// it — `hot` comes from `rank::Watch`, `nudge` from `rank::OrderState` —
/// so a row is a pure function of `(area, game, ctx, theme)`.
#[derive(Clone, Debug)]
pub struct RowCtx {
    /// `rank::Watch::hot`. The mark's only input.
    pub hot: bool,
    /// `rank::Watch::chip` — "RED ZONE", "2-MIN", … Tier 1 prints it under
    /// the clock in `hot` (the A′ frame's red `2-MIN` beneath `Q4 0:48`);
    /// tiers 2 and 3 have no row to spare and ignore it.
    pub chip: Option<&'static str>,
    /// Places this game just rose, from `OrderState`. `None` is the normal
    /// state; the gutter stays reserved either way.
    pub nudge: Option<usize>,
    pub selected: bool,
    pub pinned: bool,
    /// Print the `[NFL]` tag. A property of the list (mixed leagues), not of
    /// the game — A′ call #5.
    pub league_tag: bool,
    /// The frame's clock, for pre-game start times.
    pub now: OffsetDateTime,
    /// The tier-3 FINAL ladder's middle rung, already
    /// formatted by [`leaders_line`] from whatever `App::stats` holds for
    /// this game. The board only ever fetches stats for the zoomed game, so
    /// this is `Some` for at most one row at a time — every other final
    /// falls through it to the newest scoring play, honestly.
    pub leaders_line: Option<String>,
    /// The wave 5 sitting's `--opt` switches — `App::design_opts`, copied in
    /// wholesale. A row reads its own field(s) off this, once a later task
    /// wires one up; until then every switch is inert.
    pub design: DesignOpts,
}

/// Design-time switches for the wave 5 sitting. Default off; only
/// `gameday frame --opt` sets them; deleted when the sitting is decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DesignOpts {
    pub tint_rows: bool,
    pub clause_cap: bool,
    pub wide_tier: bool,
}

impl DesignOpts {
    pub const NAMES: [&'static str; 3] = ["tint-rows", "clause-cap", "wide-tier"];

    /// `"tint-rows,wide-tier"` → opts; an unknown name errors naming the
    /// valid set. An empty string is every switch off (`DesignOpts::default()`).
    pub fn parse(list: &str) -> Result<DesignOpts, String> {
        let mut opts = DesignOpts::default();
        for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match name {
                "tint-rows" => opts.tint_rows = true,
                "clause-cap" => opts.clause_cap = true,
                "wide-tier" => opts.wide_tier = true,
                other => {
                    return Err(format!(
                        "unknown --opt {other:?}, valid: {}",
                        Self::NAMES.join("|")
                    ))
                }
            }
        }
        Ok(opts)
    }
}

// ---------------------------------------------------------------- the grid
//
// Column offsets from the row's left edge, measured off the A′ reference
// frame at 120 columns (docs/research/v3-identity/nfl-sunday-120x40.png:
// `GB 13 CHI 10  Q3 4:20  NFL  GB 3RD & 2 AT CHI 41`, and the LATER rows
// whose start times and league tags share the same columns) — but the grid
// itself is a *derivation* from a handful of field widths, not a table of
// offsets someone can edit one at a time. That is what the review's glue
// bugs were: `↑9TNST`, `NETFLIXLAR -3.5`, `BOISPASSING YARDS` were all one
// field ending exactly where the next one started, with no cell of air
// between them. ±1 column of pixel-measurement error on the frame; the
// frame's *ordering and alignment* is the contract, the exact offsets are
// ours.

/// Column pitch of a team abbreviation: four cells hold every abbr the mapper
/// emits (`WSH`, `MTL`, `TNST`, soccer's four-letter clubs) and the fifth is
/// air. One number for the board rows, the STATS leaders column and the
/// standings table, so a four-letter code can never glue itself to what
/// follows (the review's `BOISPASSING YARDS`).
pub const ABBR_W: u16 = 5;
/// The cells an abbr's text may occupy inside its column; the last is air.
const ABBR_TEXT_W: u16 = ABBR_W - 1;
/// Score field width: three cells for a college basketball 100+.
const SCORE_W: u16 = 3;
/// Air between two fields whose pitch does not already carry it.
const GAP: u16 = 1;
/// The hot mark and its air.
const MARK_W: u16 = 2;
/// The nudge: `↑9` plus one cell of air before the abbr — `↑9TNST` was the
/// review's L3.
const NUDGE_W: u16 = 3;
/// The two fixed gutters every tier-1/2 row reserves.
pub const GUTTER: u16 = MARK_W + NUDGE_W;
const NUDGE_X: u16 = MARK_W;
/// Largest climb two glyphs can say truthfully: `↑9` means "rose 9 or more".
const NUDGE_MAX: usize = 9;

const AWAY_ABBR_X: u16 = GUTTER;
const AWAY_SCORE_X: u16 = AWAY_ABBR_X + ABBR_W;
const HOME_ABBR_X: u16 = AWAY_SCORE_X + SCORE_W + GAP;
const HOME_SCORE_X: u16 = HOME_ABBR_X + ABBR_W;
const CLOCK_X: u16 = HOME_SCORE_X + SCORE_W + GAP;
/// The longest state a row prints is a start more than six days out,
/// `SEP 21 8:20 PM`: fourteen cells (the review's L1 clipped it at eleven).
const CLOCK_W: u16 = 14;
const LEAGUE_X: u16 = CLOCK_X + CLOCK_W + GAP;
const LEAGUE_W: u16 = 4;
/// Where a row's prose starts: the headline, the fragment, the broadcast.
const TEXT_X: u16 = LEAGUE_X + LEAGUE_W + GAP;
/// Broadcast field on a LATER row: `NETFLIX` is the longest at seven.
const BCAST_W: u16 = 7;
/// The odds after the broadcast, with air between (L2: `NETFLIXLAR -3.5`).
const ODDS_X: u16 = TEXT_X + BCAST_W + GAP;

/// V3's minimum: [`clause_cap`] never cuts inside the first 48 cells of
/// prose, so a short play sentence goes untouched. The spec's ruling
/// (`docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §7 item 3).
const CLAUSE_MIN: usize = 48;
/// L7's fragment field on a wide tier-2 row: the longest situation fragment
/// this board prints (`PHI 3RD & 6 AT DAL 38`, 21 cells) fits with room to
/// spare before the play text starts at [`PLAY_X`].
const FRAGMENT_W: u16 = 40;
/// Where a wide tier-2 row's last play starts: right after the fragment
/// field, so the two can never collide regardless of either one's content.
const PLAY_X: u16 = TEXT_X + FRAGMENT_W;
/// L7's threshold: the width past which the review found tier-2 rows
/// leaving their right 60% empty (spec §7 item 4).
pub const WIDE_TIER_MIN: u16 = 160;

// Tier 1 shares the one-line tiers' grid *entirely*: the same `pair_line`
// draws `GB 13 CHI 10` at the same columns a tier-2 row does, and the clock
// and the prose share those columns too. What tier 1 adds is height, not a
// second grid.
//
// The sextant garnish that used to sit between the nameplate and the clock
// was deleted: it tofued on terminals without sextant
// coverage, it duplicated the numerals `pair_line` already draws, and it was
// the only reason tier 1 pushed its clock out to x40 while every row under it
// clocked at [`CLOCK_X`]. Dropping it buys that alignment back for free.
/// The tier-1 state chip's field. It rides on row 1, where nothing sits
/// between the clock column and the play text, so it gets the whole gap
/// rather than the clock's own [`CLOCK_W`] — a fixed 11-cell chip field once
/// clipped the longest chips `rank::watchability` emits ("BASES LOADED" and
/// "GO-AHEAD 3RD" at 12, "TYING RUN 3RD" at 13) to a word that isn't one.
/// `TEXT_X - CLOCK_X` holds all of them with room to spare.
const T1_CHIP_W: u16 = TEXT_X - CLOCK_X;

/// Render `line` into the column `x..x+w` of row `y` (both relative to
/// `area`). A column that starts past the right edge is dropped — that is
/// how a row narrows on a small terminal, rather than wrapping.
fn col(
    frame: &mut Frame,
    area: Rect,
    x: u16,
    w: u16,
    y: u16,
    align: Alignment,
    line: Line<'static>,
) {
    if x >= area.width || y >= area.height || w == 0 {
        return;
    }
    let width = w.min(area.width - x);
    let rect = Rect {
        x: area.x + x,
        y: area.y + y,
        width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(line).alignment(align), rect);
}

/// The row's ink: bright while selected, `roles.ink` otherwise.
fn ink(ctx: &RowCtx) -> Color {
    let th = theme::current();
    if ctx.selected {
        th.bright
    } else {
        th.roles().ink
    }
}

/// A team abbr. Team color for a pinned game, or (V2's `tint-rows` knob)
/// every game's — either way only where the theme allows marks to carry it;
/// everything else is ink.
///
/// Padded to a 3-cell minimum (`format!("{:<3}", abbr)`) so a
/// two-letter abbr (`KC`) fills the same cell a three-letter one (`BUF`)
/// does — the pad is part of the styled span, not a coincidence of the
/// field's blank background.
fn abbr_span(game_pinned: bool, team: &crate::domain::Team, ctx: &RowCtx) -> Line<'static> {
    let th = theme::current();
    let tinted =
        (game_pinned || ctx.design.tint_rows) && th.roles().team == TeamColorScope::HeroMarks;
    let color = if tinted {
        th.art_color(team.color)
    } else {
        ink(ctx)
    };
    let padded = format!("{:<3}", team.abbr);
    Line::from(Span::styled(
        padded,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// A score. Amber, bold in the live tiers — the one color a row spends on a
/// number. Tier 3 passes `strong = false`: a final is a dim line, and a bold
/// amber score in it out-shouts the live rows above.
fn score_span(value: u16, r: &Roles, strong: bool) -> Line<'static> {
    let mut style = Style::default().fg(r.digits);
    if strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(value.to_string(), style))
}

/// `Q4 0:48` / `FT` / `FINAL` / `4:25 PM` — the clock column's text. Soccer
/// says FT where the American leagues say FINAL (the A′ frame's FINAL
/// section: `ARS 3 BHA 0 FT EPL` above `BOS 5 TEX 2 FINAL MLB`).
fn state_text(game: &Game, now: OffsetDateTime) -> String {
    truncate(&state_text_raw(game, now), CLOCK_W as usize)
}

fn state_text_raw(game: &Game, now: OffsetDateTime) -> String {
    match game.status {
        Status::Live => format!("{} {}", game.period, game.clock).trim().to_string(),
        Status::Final => match game.league {
            League::Epl | League::Mls => "FT".into(),
            _ => "FINAL".into(),
        },
        Status::Pre => game.start.map(|t| fmt_start(t, now)).unwrap_or_default(),
    }
}

/// The situation fragment the frame prints beside a promoted row:
/// `PHI 3RD & 6 AT DAL 38` for football, `2 OUT · 1-0` for the sports whose
/// headline already says everything.
pub(crate) fn situation_summary(game: &Game) -> Option<String> {
    let sit = game.situation.as_ref()?;
    let mut parts: Vec<String> = Vec::new();
    if let Some(poss) = sit.possession.as_deref().filter(|s| !s.is_empty()) {
        parts.push(poss.to_uppercase());
    }
    if !sit.down_distance.is_empty() {
        parts.push(sit.down_distance.to_uppercase());
    }
    if let Some(on) = sit.ball_on.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("AT {}", on.to_uppercase()));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A final's story, in one fixed ladder: the
/// scoreboard's own headline, else the leaders line (already formatted by
/// [`leaders_line`]), else the newest scoring play. Shared by the board's
/// tier-3 row and the zoomed final's header (`views/zoom.rs`) so the two
/// surfaces never disagree about which rung a game landed on.
pub fn final_story(game: &Game, leaders_line: Option<&str>) -> Option<String> {
    game.headline
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| leaders_line.map(str::to_string))
        .or_else(|| game.scoring_plays.last().map(|p| p.text.clone()))
}

/// The ladder's middle rung, compacted to one line: the box score's first
/// statistical leader, "TEAM LABEL: text" — the same three fields
/// `views/zoom.rs`'s STATS tab prints per row, linearized rather than given
/// a new format. `None` when the game carries no stats (every final except
/// whichever one is currently zoomed — the board fetches stats for that game
/// alone) or the stats fetched have no leaders (5/9 leagues carry them).
pub fn leaders_line(stats: &GameStats) -> Option<String> {
    let l = stats.leaders.first()?;
    Some(format!("{} {}: {}", l.team, l.label, l.text))
}

/// V3 option: cut prose at the first clause boundary (`, ` or `. `) whose
/// start is at or past `min_cells`, dropping the separator and closing with
/// `…`. A play sentence shorter than `min_cells`, or one with no clause
/// boundary at or past it, comes back untouched — [`crate::text::truncate`]
/// still runs after this at the caller, so the two compose (this one only
/// ever shortens further, on content rather than on space).
pub(crate) fn clause_cap(text: &str, min_cells: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= min_cells {
        return text.to_string();
    }
    for i in min_cells..chars.len().saturating_sub(1) {
        if (chars[i] == ',' || chars[i] == '.') && chars[i + 1] == ' ' {
            let cut: String = chars[..i].iter().collect();
            return format!("{cut}…");
        }
    }
    text.to_string()
}

/// The hot mark, column 0 — the one cell every tier draws identically, one
/// mark column for all three. `▌` in `hot`/`dim` for a bar row, `·` in `dim`
/// for tier 3's dot; drawn down `rows` rows so tier 1's taller (3-row) block
/// gets the same mark at every row, never shifted by the layout above it.
fn mark_cell(frame: &mut Frame, area: Rect, ctx: &RowCtx, rows: u16, bar: bool) {
    let r = theme::current().roles();
    let (glyph, color) = if !bar {
        ("·", r.dim)
    } else if ctx.hot {
        ("▌", r.hot)
    } else {
        ("▌", r.dim)
    };
    for y in 0..rows.min(area.height) {
        col(
            frame,
            area,
            0,
            1,
            y,
            Alignment::Left,
            Line::from(Span::styled(glyph, Style::default().fg(color))),
        );
    }
}

/// The two gutters, drawn down `rows` rows. `bar` is false for tier 3, whose
/// mark is a dot: a final has no hotness to report, and a bar there reads as
/// a live row from across the room.
fn gutters(frame: &mut Frame, area: Rect, ctx: &RowCtx, rows: u16, bar: bool) {
    mark_cell(frame, area, ctx, rows, bar);
    let r = theme::current().roles();
    // Selection wins the gutter over a nudge: the caret says where the
    // keyboard is, which the viewer needs more than why the row moved.
    let mark = if ctx.selected {
        Some(Span::styled(
            "▸",
            Style::default()
                .fg(theme::current().bright)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        // The nudge is two glyphs, so the DISPLAYED climb clamps
        // at 9 — `↑9` reads "rose 9 or more". Clipping `↑12` to `↑1` would
        // print a number that never happened; an understated climb is the
        // honest failure.
        ctx.nudge.map(|n| {
            Span::styled(
                format!("↑{}", n.min(NUDGE_MAX)),
                Style::default().fg(r.digits),
            )
        })
    };
    if let Some(span) = mark {
        col(
            frame,
            area,
            NUDGE_X,
            NUDGE_W,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
}

/// Tier 1: a 3-row promoted row — abbr+league stack, the shared nameplate's
/// numerals, clock+state column, fragments, last play (A′ tier-1 rows).
pub fn draw_tier1(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();
    gutters(frame, area, ctx, crate::board::TIER1_ROWS, true);

    // The nameplate is tier 2's line, cell for cell: abbrs and
    // plain bold amber numerals on the shared columns. A promoted row is
    // louder than the rows below it — accent bar, bold weight, the indented
    // fragment and last-play rows — but its score is read the same way.
    pair_line(frame, area, game, ctx, true);
    if ctx.league_tag {
        let tag = Span::styled(
            game.league.slug().to_uppercase(),
            Style::default().fg(r.dim),
        );
        col(
            frame,
            area,
            AWAY_ABBR_X,
            ABBR_TEXT_W,
            1,
            Alignment::Right,
            Line::from(tag),
        );
    }
    let state = state_text(game, ctx.now);
    if !state.is_empty() {
        let span = Span::styled(
            state,
            Style::default().fg(ink(ctx)).add_modifier(Modifier::BOLD),
        );
        col(
            frame,
            area,
            CLOCK_X,
            CLOCK_W,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
    // The state chip sits directly under the clock (A′ frame: red `2-MIN`
    // beneath `Q4 0:48`). Plain hot text, not a filled block — the filled
    // chip is the hero's alone.
    if let Some(chip) = ctx.chip {
        let span = Span::styled(
            chip,
            Style::default().fg(r.hot).add_modifier(Modifier::BOLD),
        );
        col(
            frame,
            area,
            CLOCK_X,
            T1_CHIP_W,
            1,
            Alignment::Left,
            Line::from(span),
        );
    }
    let room = (area.width.saturating_sub(TEXT_X)) as usize;
    if let Some(fragment) = situation_summary(game) {
        let span = Span::styled(truncate(&fragment, room), Style::default().fg(r.dim));
        col(
            frame,
            area,
            TEXT_X,
            area.width,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
    if let Some(play) = game.last_plays.first() {
        let shown = if ctx.design.clause_cap {
            clause_cap(&play.text, CLAUSE_MIN)
        } else {
            play.text.clone()
        };
        let line = Line::from(vec![
            Span::styled("▸ ", Style::default().fg(r.dim)),
            Span::styled(
                truncate(&shown, room.saturating_sub(2)),
                Style::default().fg(ink(ctx)),
            ),
        ]);
        col(frame, area, TEXT_X, area.width, 1, Alignment::Left, line);
    }
}

/// Tier 2: one line — mark │ nudge │ ABBR n ABBR n │ clock │ [league] │
/// fragment.
pub fn draw_tier2(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let r = theme::current().roles();
    gutters(frame, area, ctx, 1, true);
    pair_line(frame, area, game, ctx, true);

    let state = state_text(game, ctx.now);
    if !state.is_empty() {
        let span = Span::styled(
            state,
            Style::default().fg(ink(ctx)).add_modifier(Modifier::BOLD),
        );
        col(
            frame,
            area,
            CLOCK_X,
            CLOCK_W,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
    league_tag(frame, area, game, ctx);
    // L7 option: at WIDE_TIER_MIN+ columns the fragment's own room shrinks
    // to FRAGMENT_W - 1 so it can never run into the play field that starts
    // at PLAY_X right behind it.
    let wide = ctx.design.wide_tier && area.width >= WIDE_TIER_MIN;
    if let Some(fragment) = situation_summary(game) {
        let room = if wide {
            (FRAGMENT_W - 1) as usize
        } else {
            (area.width.saturating_sub(TEXT_X)) as usize
        };
        let span = Span::styled(truncate(&fragment, room), Style::default().fg(r.dim));
        col(
            frame,
            area,
            TEXT_X,
            area.width,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
    if wide {
        if let Some(play) = game.last_plays.first() {
            let room = (area.width.saturating_sub(PLAY_X)) as usize;
            let line = Line::from(vec![
                Span::styled("▸ ", Style::default().fg(r.dim)),
                Span::styled(
                    truncate(&play.text, room.saturating_sub(2)),
                    Style::default().fg(ink(ctx)),
                ),
            ]);
            col(frame, area, PLAY_X, area.width, 0, Alignment::Left, line);
        }
    }
}

/// Tier 3: dim final (`· ARS 3 BHA 0 FT EPL headline`) or later
/// (`· TB @ ATL 4:25 PM NFL FOX TB -1.5 O/U 47.5`).
pub fn draw_tier3(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let r = theme::current().roles();
    gutters(frame, area, ctx, 1, false);
    let later = game.status == Status::Pre;
    if later {
        // No score to print yet: the `@` takes the away score's column so a
        // LATER row and a FINAL row keep the same grid.
        col(
            frame,
            area,
            AWAY_ABBR_X,
            ABBR_TEXT_W,
            0,
            Alignment::Right,
            abbr_span(ctx.pinned, &game.away, ctx),
        );
        let at = Span::styled("@", Style::default().fg(r.dim));
        col(
            frame,
            area,
            AWAY_SCORE_X,
            SCORE_W,
            0,
            Alignment::Center,
            Line::from(at),
        );
        col(
            frame,
            area,
            HOME_ABBR_X,
            ABBR_TEXT_W,
            0,
            Alignment::Left,
            abbr_span(ctx.pinned, &game.home, ctx),
        );
    } else {
        pair_line(frame, area, game, ctx, false);
    }

    let state = state_text(game, ctx.now);
    if !state.is_empty() {
        col(
            frame,
            area,
            CLOCK_X,
            CLOCK_W,
            0,
            Alignment::Left,
            Line::from(Span::styled(state, Style::default().fg(r.dim))),
        );
    }
    league_tag(frame, area, game, ctx);

    if later {
        if let Some(net) = game.broadcast.as_deref().filter(|s| !s.is_empty()) {
            let span = Span::styled(net.to_uppercase(), Style::default().fg(r.dim));
            col(
                frame,
                area,
                TEXT_X,
                BCAST_W,
                0,
                Alignment::Left,
                Line::from(span),
            );
        }
        if let Some(odds) = game.odds.as_deref().filter(|s| !s.is_empty()) {
            let x = ODDS_X;
            let room = (area.width.saturating_sub(x)) as usize;
            let span = Span::styled(truncate(odds, room), Style::default().fg(r.dim));
            col(
                frame,
                area,
                x,
                area.width,
                0,
                Alignment::Left,
                Line::from(span),
            );
        }
        return;
    }
    // A final's story, in its fixed ladder: its own
    // headline, else the leaders line, else the newest scoring play (the
    // list is oldest first), else whatever the situation still says.
    let headline = final_story(game, ctx.leaders_line.as_deref())
        .or_else(|| situation_summary(game))
        .unwrap_or_default();
    if !headline.is_empty() {
        let room = (area.width.saturating_sub(TEXT_X)) as usize;
        let span = Span::styled(truncate(&headline, room), Style::default().fg(r.dim));
        col(
            frame,
            area,
            TEXT_X,
            area.width,
            0,
            Alignment::Left,
            Line::from(span),
        );
    }
}

/// `GB 13 CHI 10` on the shared one-line grid.
fn pair_line(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx, strong: bool) {
    let r = theme::current().roles();
    col(
        frame,
        area,
        AWAY_ABBR_X,
        ABBR_TEXT_W,
        0,
        Alignment::Right,
        abbr_span(ctx.pinned, &game.away, ctx),
    );
    col(
        frame,
        area,
        AWAY_SCORE_X,
        SCORE_W,
        0,
        Alignment::Right,
        score_span(game.away_score, &r, strong),
    );
    col(
        frame,
        area,
        HOME_ABBR_X,
        ABBR_TEXT_W,
        0,
        Alignment::Left,
        abbr_span(ctx.pinned, &game.home, ctx),
    );
    col(
        frame,
        area,
        HOME_SCORE_X,
        SCORE_W,
        0,
        Alignment::Right,
        score_span(game.home_score, &r, strong),
    );
}

fn league_tag(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if !ctx.league_tag {
        return;
    }
    let r = theme::current().roles();
    let span = Span::styled(
        game.league.slug().to_uppercase(),
        Style::default().fg(r.dim),
    );
    col(
        frame,
        area,
        LEAGUE_X,
        LEAGUE_W,
        0,
        Alignment::Left,
        Line::from(span),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Game, League, Play, Situation, Status, Team};
    use crate::theme;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;
    use time::macros::datetime;

    fn team(abbr: &str, color: [u8; 3]) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            color,
            alt_color: [255, 255, 255],
            logo_key: format!("nfl/{}", abbr.to_lowercase()),
            ..Default::default()
        }
    }

    /// The A′ frame's tier-1 row: DAL 17 PHI 17, Q4 0:48, 2-MIN
    /// (docs/research/v3-identity/nfl-sunday-120x40.png).
    fn live_game(away: &str, home: &str) -> Game {
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: team(away, [0, 34, 68]),
            home: team(home, [0, 76, 84]),
            away_score: 17,
            home_score: 17,
            status: Status::Live,
            period: "Q4".into(),
            clock: "0:48".into(),
            situation: Some(Situation {
                down_distance: "3rd & 6".into(),
                possession: Some("PHI".into()),
                ball_on: Some("DAL 38".into()),
                ..Default::default()
            }),
            last_plays: vec![Play {
                text: "Hurts hit for a loss of 3, Dallas out of timeouts".into(),
                ..Default::default()
            }],
            ..Game::default()
        }
    }

    /// The tier-2 row the frame shows: GB 13 CHI 10, Q3 4:20.
    fn tier2_game() -> Game {
        let mut g = live_game("GB", "CHI");
        g.away_score = 13;
        g.home_score = 10;
        g.period = "Q3".into();
        g.clock = "4:20".into();
        g
    }

    fn ctx() -> RowCtx {
        RowCtx {
            hot: false,
            chip: None,
            nudge: None,
            selected: false,
            pinned: false,
            league_tag: true,
            now: datetime!(2026-09-13 14:47 -4),
            leaders_line: None,
            design: DesignOpts::default(),
        }
    }

    fn render(
        w: u16,
        h: u16,
        game: &Game,
        ctx: &RowCtx,
        draw: fn(&mut ratatui::Frame, Rect, &Game, &RowCtx),
    ) -> Terminal<TestBackend> {
        theme::set_current("broadcast").unwrap();
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, f.area(), game, ctx)).unwrap();
        term
    }

    fn text_of(buf: &Buffer) -> String {
        let area = *buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The columns `needle` occupies in row `y`, or None when it isn't there.
    fn col_of(buf: &Buffer, y: u16, needle: &str) -> Option<u16> {
        let row: Vec<&str> = (0..buf.area().width)
            .map(|x| buf[(x, y)].symbol())
            .collect();
        let joined: String = row.concat();
        joined
            .find(needle)
            .map(|byte| joined[..byte].chars().count() as u16)
    }

    fn cells_with_fg(buf: &Buffer, rect: Rect, color: Color) -> usize {
        let mut n = 0;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                if buf[(x, y)].fg == color {
                    n += 1;
                }
            }
        }
        n
    }

    /// broadcast with `roles.team = "hero+marks"` — no built-in ships that
    /// scope, and the pinned abbr is only colored at that scope or above.
    fn install_marks_theme() {
        let text = include_str!("../../assets/themes/broadcast.toml")
            .replace("name = \"broadcast\"", "name = \"marks\"")
            .replace("team = \"hero\"", "team = \"hero+marks\"");
        let (name, th) = theme::parse_theme(&text).unwrap();
        theme::install(theme::Entry {
            name,
            theme: th,
            user: true,
        });
        theme::set_current("marks").unwrap();
    }

    #[test]
    fn tier2_line_layout_and_amber_scores() {
        let game = tier2_game();
        let term = render(120, 1, &game, &ctx(), draw_tier2);
        let buf = term.backend().buffer();
        let r = theme::current().roles();
        let text = text_of(buf);

        assert_eq!(
            buf[(0, 0)].symbol(),
            "▌",
            "the hot mark owns column 0\n{text}"
        );
        // The frame's grid: abbr right-aligned into the gutter's shoulder,
        // score right-aligned two columns later, home pair mirrored.
        // The abbr pads to a 3-cell minimum, so a 2-char abbr
        // right-aligned in the 4-cell field now starts one column left of
        // where the unpadded text used to (ABBR_TEXT_W-3, not ABBR_TEXT_W-2).
        assert_eq!(
            col_of(buf, 0, "GB"),
            Some(AWAY_ABBR_X + ABBR_TEXT_W - 3),
            "away abbr right-aligned\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "13"),
            Some(AWAY_SCORE_X + SCORE_W - 2),
            "away score right-aligned\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "CHI"),
            Some(HOME_ABBR_X),
            "home abbr left-aligned\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "10"),
            Some(HOME_SCORE_X + SCORE_W - 2),
            "home score right-aligned\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "Q3 4:20"),
            Some(CLOCK_X),
            "clock column\n{text}"
        );

        // Amber, bold, and only on the scores: the two abbrs stay ink.
        for x in [
            AWAY_SCORE_X + 1,
            AWAY_SCORE_X + 2,
            HOME_SCORE_X + 1,
            HOME_SCORE_X + 2,
        ] {
            let c = &buf[(x, 0)];
            assert_eq!(
                c.fg,
                r.digits,
                "score cell {x} is amber ({:?})\n{text}",
                c.symbol()
            );
            assert!(
                c.modifier.contains(Modifier::BOLD),
                "score cell {x} is bold\n{text}"
            );
        }
        assert_eq!(
            buf[(HOME_ABBR_X, 0)].fg,
            r.ink,
            "an unpinned abbr is ink\n{text}"
        );

        // The league tag is a mixed-list decision, not a row decision.
        assert_eq!(
            col_of(buf, 0, "NFL"),
            Some(LEAGUE_X),
            "league tag column\n{text}"
        );
        let mut plain = ctx();
        plain.league_tag = false;
        let term = render(120, 1, &game, &plain, draw_tier2);
        let bare = text_of(term.backend().buffer());
        assert!(
            !bare.contains("NFL"),
            "no league tag when the list is single-league\n{bare}"
        );
        assert!(bare.contains("Q3 4:20"), "everything else survives\n{bare}");
    }

    #[test]
    fn hot_mark_has_two_states_only() {
        let game = tier2_game();
        let r = theme::current().roles();
        let cases = [
            (true, None, r.hot),
            (false, None, r.dim),
            // A nudge is a rank fact, not a hotness fact (A′ call #6/#7).
            (false, Some(2), r.dim),
            (true, Some(2), r.hot),
        ];
        for (hot, nudge, want) in cases {
            let c = RowCtx {
                hot,
                nudge,
                ..ctx()
            };
            let term = render(120, 1, &game, &c, draw_tier2);
            let buf = term.backend().buffer();
            assert_eq!(
                buf[(0, 0)].symbol(),
                "▌",
                "hot={hot} nudge={nudge:?}: the mark is always ▌"
            );
            assert_eq!(
                buf[(0, 0)].fg,
                want,
                "hot={hot} nudge={nudge:?}: two ink states only"
            );
        }
    }

    #[test]
    fn nudge_gutter_never_shifts_the_row() {
        let game = tier2_game();
        let r = theme::current().roles();
        let quiet = render(120, 1, &game, &ctx(), draw_tier2);
        let risen = render(
            120,
            1,
            &game,
            &RowCtx {
                nudge: Some(2),
                ..ctx()
            },
            draw_tier2,
        );
        let (a, b) = (quiet.backend().buffer(), risen.backend().buffer());
        assert_eq!(
            col_of(a, 0, "GB"),
            col_of(b, 0, "GB"),
            "the abbr keeps its column"
        );
        for x in GUTTER..120 {
            assert_eq!(
                a[(x, 0)].symbol(),
                b[(x, 0)].symbol(),
                "cell {x} moved with the nudge"
            );
            assert_eq!(
                a[(x, 0)].fg,
                b[(x, 0)].fg,
                "cell {x} changed color with the nudge"
            );
        }
        // The gutter is reserved either way: empty without a nudge, ↑2 with one.
        assert_eq!(
            col_of(b, 0, "↑2"),
            Some(NUDGE_X),
            "the nudge lives in its own gutter"
        );
        assert_eq!(b[(NUDGE_X, 0)].fg, r.digits, "the nudge is amber");
        for x in NUDGE_X..GUTTER {
            assert_eq!(
                a[(x, 0)].symbol(),
                " ",
                "no nudge leaves the gutter empty at {x}"
            );
        }
    }

    #[test]
    fn a_big_nudge_clamps_to_nine_rather_than_lying() {
        // Two cells cannot say "12", and `↑1` is a number that
        // never happened. `↑9` understates; it never fabricates.
        let game = tier2_game();
        let r = theme::current().roles();
        for (nudge, want) in [(2usize, "↑2"), (9, "↑9"), (12, "↑9"), (137, "↑9")] {
            let term = render(
                120,
                1,
                &game,
                &RowCtx {
                    nudge: Some(nudge),
                    ..ctx()
                },
                draw_tier2,
            );
            let buf = term.backend().buffer();
            // The nudge glyph is always two cells (arrow + one digit); the
            // gutter's third cell is the air before the abbr — trim it off
            // rather than pasting the glyph width as a literal.
            let got: String = (NUDGE_X..GUTTER)
                .map(|x| buf[(x, 0)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string();
            assert_eq!(
                got,
                want,
                "nudge {nudge} renders {want} in the gutter\n{}",
                text_of(buf)
            );
            // Only the glyph's own cells carry the amber style; the air
            // cell the wider gutter now reserves is unstyled.
            for x in NUDGE_X..NUDGE_X + got.chars().count() as u16 {
                assert_eq!(
                    buf[(x, 0)].fg,
                    r.digits,
                    "nudge {nudge}: the gutter is amber at {x}"
                );
            }
            // Whatever it says, it says it inside the gutter.
            assert_eq!(
                buf[(GUTTER, 0)].symbol(),
                " ",
                "nudge {nudge} must not spill past the gutter"
            );
        }
    }

    /// L3: `↑9TNST`. The nudge field is three cells so a two-glyph climb
    /// keeps a cell of air before a four-letter code.
    #[test]
    fn a_four_letter_code_with_a_nudge_keeps_its_gap() {
        let mut game = tier2_game();
        game.away.abbr = "TNST".into();
        game.home.abbr = "UGA".into();
        let mut c = ctx();
        c.nudge = Some(12); // prints ↑9: "rose 9 or more"
        let term = render(120, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(col_of(buf, 0, "↑9"), Some(NUDGE_X), "{text}");
        assert_eq!(col_of(buf, 0, "TNST"), Some(AWAY_ABBR_X), "{text}");
        assert!(!text.contains("↑9TNST"), "no air after the nudge\n{text}");
        assert_eq!(
            buf[(AWAY_ABBR_X + ABBR_TEXT_W, 0)].symbol(),
            " ",
            "air before the score\n{text}"
        );
        // Right-aligned in the 3-cell score field, same as every other
        // 2-digit score in this file.
        assert_eq!(
            col_of(buf, 0, "13"),
            Some(AWAY_SCORE_X + SCORE_W - 2),
            "{text}"
        );
    }

    #[test]
    fn selection_is_a_caret_in_the_gutter_and_brighter_text() {
        let game = tier2_game();
        let th = theme::current();
        let quiet = render(120, 1, &game, &ctx(), draw_tier2);
        let picked = render(
            120,
            1,
            &game,
            &RowCtx {
                selected: true,
                ..ctx()
            },
            draw_tier2,
        );
        let (a, b) = (quiet.backend().buffer(), picked.backend().buffer());
        let text = text_of(b);

        assert_eq!(
            b[(NUDGE_X, 0)].symbol(),
            "▸",
            "the caret takes the nudge gutter\n{text}"
        );
        assert_eq!(b[(NUDGE_X, 0)].fg, th.bright, "the caret is bright\n{text}");
        assert_eq!(
            a[(NUDGE_X, 0)].symbol(),
            " ",
            "an unselected row has no caret"
        );

        // The abbrs and the clock brighten; the score keeps its amber, and
        // nothing moves.
        for needle in ["GB", "CHI"] {
            let x = col_of(b, 0, needle).unwrap();
            assert_eq!(
                col_of(a, 0, needle),
                Some(x),
                "selection never moves {needle}\n{text}"
            );
            assert_eq!(
                b[(x, 0)].fg,
                th.bright,
                "{needle} is bright while selected\n{text}"
            );
            assert_eq!(
                a[(x, 0)].fg,
                th.roles().ink,
                "{needle} is plain ink otherwise"
            );
        }
        assert_eq!(
            b[(CLOCK_X, 0)].fg,
            th.bright,
            "the clock brightens too\n{text}"
        );
        assert_eq!(
            b[(AWAY_SCORE_X + 2, 0)].fg,
            th.roles().digits,
            "the score stays amber\n{text}"
        );

        // Selection wins the gutter over a nudge — one glyph, not two facts.
        let both = render(
            120,
            1,
            &game,
            &RowCtx {
                selected: true,
                nudge: Some(3),
                ..ctx()
            },
            draw_tier2,
        );
        let c = both.backend().buffer();
        assert_eq!(
            c[(NUDGE_X, 0)].symbol(),
            "▸",
            "the caret outranks the nudge\n{}",
            text_of(c)
        );
        assert!(
            !text_of(c).contains("↑3"),
            "no nudge beside the caret\n{}",
            text_of(c)
        );
    }

    #[test]
    fn pinned_abbr_wears_team_color_others_ink() {
        install_marks_theme();
        let game = tier2_game();
        let th = theme::current();
        let r = th.roles();
        assert_eq!(r.team, theme::TeamColorScope::HeroMarks);
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        let pinned = RowCtx {
            pinned: true,
            ..ctx()
        };
        term.draw(|f| draw_tier2(f, f.area(), &game, &pinned))
            .unwrap();
        let buf = term.backend().buffer();
        let text = text_of(buf);
        let (ax, hx) = (
            col_of(buf, 0, "GB").unwrap(),
            col_of(buf, 0, "CHI").unwrap(),
        );
        assert_eq!(
            buf[(ax, 0)].fg,
            th.art_color(game.away.color),
            "a pinned away abbr wears its color\n{text}"
        );
        assert_eq!(
            buf[(hx, 0)].fg,
            th.art_color(game.home.color),
            "a pinned home abbr wears its color\n{text}"
        );
        // Nothing else in the row does — the scores stay amber, the clock ink.
        assert_eq!(
            buf[(AWAY_SCORE_X + 2, 0)].fg,
            r.digits,
            "scores are never team-colored\n{text}"
        );
        assert_eq!(buf[(CLOCK_X, 0)].fg, r.ink, "the clock is ink\n{text}");
        assert_eq!(
            cells_with_fg(
                buf,
                Rect {
                    x: CLOCK_X,
                    y: 0,
                    width: 120 - CLOCK_X,
                    height: 1
                },
                th.art_color(game.away.color)
            ),
            0,
            "team color stops at the abbr\n{text}"
        );

        // At the default `hero` scope even a pinned abbr is ink.
        theme::set_current("broadcast").unwrap();
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        term.draw(|f| draw_tier2(f, f.area(), &game, &pinned))
            .unwrap();
        let buf = term.backend().buffer();
        assert_eq!(
            buf[(ax, 0)].fg,
            theme::current().roles().ink,
            "hero-scope keeps rows monochrome"
        );
    }

    /// V2 option: every tier row's abbrs in team color inside the theme's
    /// `team` scope, not only a pinned game's. Draws its own `Terminal`
    /// rather than the shared `render` helper — `render` pins the theme to
    /// `broadcast` for every other (theme-agnostic) test in this module, the
    /// same reason `pinned_abbr_wears_team_color_others_ink` above does too.
    #[test]
    fn tint_rows_colors_an_unpinned_abbr_only_under_hero_marks() {
        install_marks_theme();
        let game = tier2_game();
        let mut c = ctx();
        c.design.tint_rows = true;
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        term.draw(|f| draw_tier2(f, f.area(), &game, &c)).unwrap();
        let buf = term.backend().buffer();
        let x = col_of(buf, 0, "GB").unwrap();
        assert_eq!(
            buf[(x, 0)].fg,
            theme::current().art_color(game.away.color),
            "tinted under hero+marks"
        );

        theme::set_current("broadcast").unwrap(); // scope hero: never tinted
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        term.draw(|f| draw_tier2(f, f.area(), &game, &c)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf[(x, 0)].fg, ink(&c), "hero scope stays ink");

        let mut off = ctx();
        off.design.tint_rows = false;
        install_marks_theme();
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        term.draw(|f| draw_tier2(f, f.area(), &game, &off)).unwrap();
        assert_eq!(
            term.backend().buffer()[(x, 0)].fg,
            ink(&off),
            "knob off: unpinned stays ink"
        );
    }

    /// V3 option: the first clause after 48 cells, `…` closes.
    #[test]
    fn clause_cap_cuts_at_the_first_clause_after_the_minimum() {
        let long = "No Huddle-Shotgun #29 T.Reed Jr. rush right for 4 yards gain to the SEMO20 (#91 B.Hawkins; #13 K.Bilal-Jones), and the clock runs";
        let cut = clause_cap(long, CLAUSE_MIN);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(
            cut.chars().count() > CLAUSE_MIN && cut.chars().count() < long.chars().count(),
            "{cut}"
        );
        assert!(
            cut.starts_with(
                "No Huddle-Shotgun #29 T.Reed Jr. rush right for 4 yards gain to the SEMO20 (#91 B.Hawkins; #13 K.Bilal-Jones)"
            ),
            "cut at the `, ` after 48: {cut}"
        );
        assert_eq!(
            clause_cap("Short play, no cut", CLAUSE_MIN),
            "Short play, no cut",
            "under the minimum: untouched"
        );
        let no_clause = "a".repeat(120);
        assert_eq!(
            clause_cap(&no_clause, CLAUSE_MIN),
            no_clause,
            "no clause boundary: untouched (truncate still applies)"
        );
        // Through the tier-1 row.
        let mut game = live_game("DAL", "PHI");
        game.last_plays[0].text = long.into();
        let mut c = ctx();
        c.design.clause_cap = true;
        let text = text_of(render(200, 3, &game, &c, draw_tier1).backend().buffer());
        assert!(text.contains("K.Bilal-Jones)…"), "{text}");
        assert!(!text.contains("and the clock runs"), "{text}");
    }

    /// L7 option: at ≥160 columns a tier-2 row carries its last play after
    /// the fragment.
    #[test]
    fn wide_tier_puts_the_last_play_on_the_tier2_row_past_160_columns() {
        let game = live_game("DAL", "PHI"); // has a fragment and a last play
        let mut c = ctx();
        c.design.wide_tier = true;
        let term = render(180, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(
            col_of(buf, 0, "▸ Hurts hit"),
            Some(PLAY_X),
            "play at PLAY_X\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "PHI 3RD & 6"),
            Some(TEXT_X),
            "fragment stays\n{text}"
        );
        let narrow = text_of(render(120, 1, &game, &c, draw_tier2).backend().buffer());
        assert!(
            !narrow.contains("▸ Hurts"),
            "under WIDE_TIER_MIN: the two-row form\n{narrow}"
        );
        let mut off = ctx();
        off.design.wide_tier = false;
        let wide_off = text_of(render(180, 1, &game, &off, draw_tier2).backend().buffer());
        assert!(
            !wide_off.contains("▸ Hurts"),
            "knob off: current form\n{wide_off}"
        );
    }

    #[test]
    fn tier3_later_shows_local_time_never_iso() {
        let mut game = live_game("TB", "ATL");
        game.status = Status::Pre;
        game.situation = None;
        game.last_plays.clear();
        game.start = Some(datetime!(2026-09-13 16:25 -4));
        game.broadcast = Some("FOX".into());
        game.odds = Some("TB -1.5  O/U 47.5".into());
        let term = render(120, 1, &game, &ctx(), draw_tier3);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(
            col_of(buf, 0, "4:25 PM"),
            Some(CLOCK_X),
            "fmt_start(ctx.now) in the clock column\n{text}"
        );
        assert!(!text.contains("2026-"), "never an ISO stamp\n{text}");
        // Padded to a 3-cell minimum, ABBR_TEXT_W-3 not ABBR_TEXT_W-2.
        assert_eq!(
            col_of(buf, 0, "TB"),
            Some(AWAY_ABBR_X + ABBR_TEXT_W - 3),
            "away abbr right-aligned\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "@"),
            Some(AWAY_SCORE_X + 1),
            "the @ takes the score column\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "ATL"),
            Some(HOME_ABBR_X),
            "home abbr left-aligned\n{text}"
        );
        assert_eq!(col_of(buf, 0, "FOX"), Some(TEXT_X), "broadcast\n{text}");
        assert_eq!(col_of(buf, 0, "TB -1.5"), Some(ODDS_X), "odds\n{text}");
        assert_eq!(
            buf[(0, 0)].symbol(),
            "·",
            "tier 3 is a dot, never a bar\n{text}"
        );
        assert_eq!(buf[(0, 0)].fg, theme::current().roles().dim);

        // A final row: newest scoring play as the headline, amber scores.
        let mut fin = live_game("ARS", "BHA");
        fin.league = League::Epl;
        fin.status = Status::Final;
        fin.away_score = 3;
        fin.home_score = 0;
        fin.scoring_plays = vec![
            Play {
                text: "Saka opens the scoring".into(),
                scoring: true,
                ..Default::default()
            },
            Play {
                text: "Ødegaard 2 assists".into(),
                scoring: true,
                ..Default::default()
            },
        ];
        let term = render(120, 1, &fin, &ctx(), draw_tier3);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(
            col_of(buf, 0, "FT"),
            Some(CLOCK_X),
            "soccer finals say FT\n{text}"
        );
        assert_eq!(col_of(buf, 0, "EPL"), Some(LEAGUE_X), "league tag\n{text}");
        assert_eq!(
            col_of(buf, 0, "Ødegaard 2 assists"),
            Some(TEXT_X),
            "newest scoring play\n{text}"
        );
        assert!(!text.contains("Saka"), "only the newest\n{text}");
        assert_eq!(
            buf[(AWAY_SCORE_X + 2, 0)].fg,
            theme::current().roles().digits,
            "scores stay amber\n{text}"
        );
    }

    /// L1 + L2: a start more than six days out prints whole at every width,
    /// and `NETFLIX` is followed by air before the odds.
    #[test]
    fn a_later_row_a_week_out_prints_its_whole_date_at_80_and_200_columns() {
        let mut game = live_game("TB", "ATL");
        game.status = Status::Pre;
        game.situation = None;
        game.last_plays.clear();
        // Eight days past ctx().now (2026-09-13): the month-day form.
        game.start = Some(datetime!(2026-09-21 20:20 -4));
        game.broadcast = Some("NETFLIX".into());
        game.odds = Some("LAR -3.5  O/U 44.5".into());
        for w in [80u16, 200] {
            let term = render(w, 1, &game, &ctx(), draw_tier3);
            let buf = term.backend().buffer();
            let text = text_of(buf);
            assert_eq!(
                col_of(buf, 0, "SEP 21 8:20 PM"),
                Some(CLOCK_X),
                "whole start at {w} columns\n{text}"
            );
            assert_eq!(
                buf[(CLOCK_X + CLOCK_W, 0)].symbol(),
                " ",
                "air before the tag at {w}\n{text}"
            );
            assert_eq!(col_of(buf, 0, "NFL"), Some(LEAGUE_X), "{text}");
            assert_eq!(col_of(buf, 0, "NETFLIX"), Some(TEXT_X), "{text}");
            assert_eq!(
                buf[(TEXT_X + BCAST_W, 0)].symbol(),
                " ",
                "air after NETFLIX at {w}\n{text}"
            );
            assert_eq!(col_of(buf, 0, "LAR -3.5"), Some(ODDS_X), "{text}");
        }
    }

    /// This guards the DERIVATION rule, not the numbers themselves: every
    /// `_X` offset must be computed from the widths before it, so a future
    /// edit that re-literalizes one (types a number where a sum belongs)
    /// fails here even if that number happens to still be right. The two
    /// width pins below (`CLOCK_W`, `BCAST_W`) are the numbers' own
    /// receipts; this test is not.
    #[test]
    fn the_grid_derives_from_its_widths() {
        assert_eq!(GUTTER, MARK_W + NUDGE_W);
        assert_eq!(AWAY_SCORE_X, AWAY_ABBR_X + ABBR_W);
        assert_eq!(HOME_ABBR_X, AWAY_SCORE_X + SCORE_W + GAP);
        assert_eq!(HOME_SCORE_X, HOME_ABBR_X + ABBR_W);
        assert_eq!(CLOCK_X, HOME_SCORE_X + SCORE_W + GAP);
        assert_eq!(LEAGUE_X, CLOCK_X + CLOCK_W + GAP);
        assert_eq!(TEXT_X, LEAGUE_X + LEAGUE_W + GAP);
        assert_eq!(ODDS_X, TEXT_X + BCAST_W + GAP);
        assert_eq!(
            CLOCK_W as usize,
            "SEP 21 8:20 PM".chars().count(),
            "CLOCK_W holds the longest state"
        );
        assert_eq!(BCAST_W as usize, "NETFLIX".len());
    }

    /// The tier-3 FINAL ladder in its fixed order — `headline` → leaders line →
    /// newest scoring play.
    /// (MLS carries no `headlines` at all on any committed fixture final —
    /// 0/9 coverage — so its finals exercise the lower two rungs honestly;
    /// this test stands in for that with a synthetic game instead of
    /// depending on the MLS fixture directly.)
    #[test]
    fn the_final_row_prefers_the_headline() {
        let mut fin = live_game("ARS", "BHA");
        fin.league = League::Epl;
        fin.status = Status::Final;
        fin.scoring_plays = vec![Play {
            text: "Saka opens the scoring".into(),
            scoring: true,
            ..Default::default()
        }];

        // Headline present: it wins over both lower rungs.
        fin.headline = Some("Arsenal beat Brighton to go top of the table".into());
        let with_headline = RowCtx {
            leaders_line: Some("ARS Saka: 2 G, 1 A".into()),
            ..ctx()
        };
        let term = render(120, 1, &fin, &with_headline, draw_tier3);
        let text = text_of(term.backend().buffer());
        assert!(
            text.contains("Arsenal beat Brighton to go top of the table"),
            "{text}"
        );
        assert!(
            !text.contains("Saka opens"),
            "the headline wins over the scoring play\n{text}"
        );

        // No headline: falls to the leaders line.
        fin.headline = None;
        let leaders_only = RowCtx {
            leaders_line: Some("ARS Saka: 2 G, 1 A".into()),
            ..ctx()
        };
        let term = render(120, 1, &fin, &leaders_only, draw_tier3);
        let text = text_of(term.backend().buffer());
        assert!(text.contains("ARS Saka: 2 G, 1 A"), "{text}");
        assert!(
            !text.contains("Saka opens"),
            "leaders wins over the scoring play\n{text}"
        );

        // Neither headline nor leaders: existing behavior — newest scoring
        // play — is pinned.
        let term = render(120, 1, &fin, &ctx(), draw_tier3);
        let text = text_of(term.backend().buffer());
        assert!(text.contains("Saka opens the scoring"), "{text}");
    }

    /// An outsized headline (a 200-char string, far past
    /// anything ESPN sends) at a narrow tier-3 width truncates with an
    /// ellipsis and never overflows the row — no panic, no bleeding past
    /// the frame's own width.
    #[test]
    fn a_200_char_headline_truncates_with_ellipsis_at_60_cols() {
        let mut fin = live_game("ARS", "BHA");
        fin.league = League::Epl;
        fin.status = Status::Final;
        fin.headline = Some("A".repeat(200));
        let term = render(60, 1, &fin, &ctx(), draw_tier3);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(
            text.chars().count(),
            60,
            "row must stay exactly the frame's width, no overflow\n{text}"
        );
        assert!(
            text.contains('…'),
            "a 200-char headline at 60 cols must be truncated with an ellipsis\n{text}"
        );
        assert!(
            !text.contains(&"A".repeat(23)),
            "the headline itself must be cut, not just clipped by the terminal\n{text}"
        );
    }

    /// A final that already carries a headline, then gets a
    /// later refresh that appends a newer scoring play (e.g. a correction or
    /// a stats-pass update landing after the game went final) — the ladder
    /// must still show the headline, not fall to the newly-appended play.
    #[test]
    fn a_headline_survives_a_later_scoring_play_refresh() {
        // Two directions on the same game so the test can actually fail:
        // with a headline present, it wins even over a scoring play that
        // arrived after it (the refresh case); with the headline absent,
        // the same scoring play is what surfaces. If `final_story` ever
        // stopped checking the headline first, direction one would show it
        // — direction two proves the scoring-play rung still works at all.
        let mut fin = live_game("ARS", "BHA");
        fin.league = League::Epl;
        fin.status = Status::Final;
        fin.headline = Some("Arsenal beat Brighton to go top of the table".into());
        fin.scoring_plays = vec![
            Play {
                text: "Saka opens the scoring".into(),
                scoring: true,
                ..Default::default()
            },
            // A refresh lands a newer scoring play after the headline was
            // already set.
            Play {
                text: "Ødegaard doubles the lead".into(),
                scoring: true,
                ..Default::default()
            },
        ];
        assert_eq!(
            final_story(&fin, None).as_deref(),
            Some("Arsenal beat Brighton to go top of the table"),
            "the headline must survive a later scoring-play refresh"
        );

        // Same game, headline cleared: the newest scoring play must now
        // surface — proving the fallback rung isn't dead code.
        fin.headline = None;
        assert_eq!(
            final_story(&fin, None).as_deref(),
            Some("Ødegaard doubles the lead"),
            "without a headline, the newest scoring play from the refresh must surface"
        );
    }

    #[test]
    fn tier1_is_three_rows_on_the_shared_grid() {
        let game = live_game("DAL", "PHI");
        let r = theme::current().roles();
        let term = render(120, 3, &game, &RowCtx { hot: true, ..ctx() }, draw_tier1);
        let buf = term.backend().buffer();
        let text = text_of(buf);

        // The garnish is gone, and what makes
        // that checkable is not the one column it vacated (with the clock at
        // x22 there is exactly one cell between the nameplate and the clock,
        // which is too degenerate to fail interestingly) but the *kind* of
        // ink it was. The garnish was big-glyph digits; a tier-1 block must
        // now draw no block-element glyph anywhere, on any row.
        for y in 0..3u16 {
            for x in 0..120u16 {
                let ch = buf[(x, y)].symbol().chars().next().unwrap_or(' ');
                let o = ch as u32;
                // `▌` (U+258C) is the hot mark in column 0 — the one block
                // element a row is allowed, and it is a gutter, not a score.
                let allowed = ch == '▌' && x == 0;
                assert!(
                    allowed
                        || !((0x2580..=0x259F).contains(&o) || (0x1FB00..=0x1FBFF).contains(&o)),
                    "tier 1 draws a glyph score at ({x},{y}): U+{o:04X} — the numerals ARE the \
                     score now (R39)\n{text}"
                );
            }
        }

        // The stack sits on the shared nameplate grid — abbr
        // over league tag at tier 2's own columns — and the clock and the two
        // text rows sit in tier 2's own columns as well.
        assert_eq!(
            col_of(buf, 0, "DAL"),
            Some(AWAY_ABBR_X + ABBR_TEXT_W - 3),
            "away abbr, row 0\n{text}"
        );
        assert_eq!(
            col_of(buf, 1, "NFL"),
            Some(AWAY_ABBR_X + ABBR_TEXT_W - 3),
            "league under it, row 1\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "PHI"),
            Some(HOME_ABBR_X),
            "home abbr, row 0\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "Q4 0:48"),
            Some(CLOCK_X),
            "clock column, row 0\n{text}"
        );
        assert_eq!(
            col_of(buf, 0, "PHI 3RD & 6 AT DAL 38"),
            Some(TEXT_X),
            "situation, row 0\n{text}"
        );
        assert_eq!(
            col_of(buf, 1, "▸ Hurts hit"),
            Some(TEXT_X),
            "last play, row 1\n{text}"
        );

        // The mark is a bar down the whole block, one state.
        for y in 0..3 {
            assert_eq!(
                buf[(0, y)].symbol(),
                "▌",
                "the mark runs the block's height\n{text}"
            );
            assert_eq!(buf[(0, y)].fg, r.hot, "hot row\n{text}");
        }

        // The state chip rides under the clock, in `hot` — the A′ frame's red
        // `2-MIN` beneath `Q4 0:48` (docs/research/v3-identity/nfl-sunday-120x40.png).
        let chipped = RowCtx {
            chip: Some("2-MIN"),
            ..ctx()
        };
        let term = render(120, 3, &game, &chipped, draw_tier1);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(
            col_of(buf, 1, "2-MIN"),
            Some(CLOCK_X),
            "chip under the clock\n{text}"
        );
        assert_eq!(buf[(CLOCK_X, 1)].fg, r.hot, "the chip is hot\n{text}");
        assert_eq!(
            col_of(buf, 0, "Q4 0:48"),
            Some(CLOCK_X),
            "the clock keeps row 0\n{text}"
        );
        assert_eq!(
            col_of(buf, 1, "▸ Hurts hit"),
            Some(TEXT_X),
            "the play keeps its column\n{text}"
        );
        let bare = text_of(render(120, 3, &game, &ctx(), draw_tier1).backend().buffer());
        assert!(!bare.contains("2-MIN"), "no chip, no row\n{bare}");

        // The longest chip the ranker emits fits whole — before the chip had
        // `T1_CHIP_W`, the clock column's own width clipped "BASES LOADED"
        // to "BASES LOADE".
        // "10 MEN" is the shortest of the family and the
        // reason the men chip could not name the side: "AVL 10 MEN" is 10
        // cells but the chip is a `&'static str`, not a format.
        for chip in ["BASES LOADED", "TYING RUN 3RD", "GO-AHEAD 3RD", "10 MEN"] {
            let c = RowCtx {
                chip: Some(chip),
                ..ctx()
            };
            let term = render(120, 3, &game, &c, draw_tier1);
            let text = text_of(term.backend().buffer());
            assert!(text.contains(chip), "{chip} must not be clipped\n{text}");
        }

        // A one-row tier 1 still prints its numerals — the block never blanks
        // its score just because the rows under it were cut.
        let term = render(120, 1, &game, &ctx(), draw_tier1);
        let text = text_of(term.backend().buffer());
        assert!(
            text.contains("17"),
            "a one-row tier 1 still prints its score\n{text}"
        );
    }

    #[test]
    fn tier1_score_is_readable_text_on_the_shared_grid() {
        // A tier-1 row's score is a plain numeral in the SAME
        // column tier 2 puts it in — both reviews flagged tier 1 as the one
        // row whose score can't be read at a glance, sitting off the grid the
        // rows below share.
        let game = tier2_game(); // GB 13 CHI 10 — two distinguishable scores
        let r = theme::current().roles();
        let t1 = render(120, 3, &game, &ctx(), draw_tier1);
        let t2 = render(120, 1, &game, &ctx(), draw_tier2);
        let (b1, b2) = (t1.backend().buffer(), t2.backend().buffer());
        let (text1, text2) = (text_of(b1), text_of(b2));

        for needle in ["GB", "13", "CHI", "10"] {
            assert_eq!(
                col_of(b1, 0, needle),
                col_of(b2, 0, needle),
                "{needle} shares tier 2's column\n{text1}\n---\n{text2}"
            );
        }
        assert_eq!(
            col_of(b1, 0, "13"),
            Some(AWAY_SCORE_X + SCORE_W - 2),
            "away numeral\n{text1}"
        );
        assert_eq!(
            col_of(b1, 0, "10"),
            Some(HOME_SCORE_X + SCORE_W - 2),
            "home numeral\n{text1}"
        );
        // Amber and bold, the score's one color, on both numerals.
        for x in [
            AWAY_SCORE_X + 1,
            AWAY_SCORE_X + 2,
            HOME_SCORE_X + 1,
            HOME_SCORE_X + 2,
        ] {
            let c = &b1[(x, 0)];
            assert_eq!(
                c.fg,
                r.digits,
                "numeral cell {x} is amber ({:?})\n{text1}",
                c.symbol()
            );
            assert!(
                c.modifier.contains(Modifier::BOLD),
                "numeral cell {x} is bold\n{text1}"
            );
        }
    }

    #[test]
    fn tier1_and_tier2_clock_in_the_same_column() {
        // The sextant garnish was the ONLY reason tier 1 pushed its
        // clock out to x40 while every row under it clocked at x22.
        // With the garnish deleted the two tiers share the
        // column, and this test is what stops a future "tier 1 needs room
        // for X" from quietly re-introducing the zig-zag.
        let game = tier2_game(); // GB 13 CHI 10, Q3 4:20
        let t1 = render(120, 3, &game, &ctx(), draw_tier1);
        let t2 = render(120, 1, &game, &ctx(), draw_tier2);
        let (b1, b2) = (t1.backend().buffer(), t2.backend().buffer());
        let (text1, text2) = (text_of(b1), text_of(b2));
        let clock = state_text(&game, ctx().now);
        assert!(
            !clock.is_empty(),
            "the fixture must have a clock to compare"
        );
        let (c1, c2) = (col_of(b1, 0, &clock), col_of(b2, 0, &clock));
        assert_eq!(
            c1,
            Some(CLOCK_X),
            "tier 1 clocks at the shared column\n{text1}"
        );
        assert_eq!(
            c1, c2,
            "tier 1 and tier 2 clock in the same column\n{text1}\n---\n{text2}"
        );
        // Cell for cell, not just "same x": the clock's first cell is the
        // same symbol in the same ink on both tiers.
        assert_eq!(
            b1[(CLOCK_X, 0)].symbol(),
            b2[(CLOCK_X, 0)].symbol(),
            "same clock cell\n{text1}"
        );
        assert_eq!(
            b1[(CLOCK_X, 0)].fg,
            b2[(CLOCK_X, 0)].fg,
            "same clock ink\n{text1}"
        );
        // The prose column follows the clock: tier 1's fragment starts where
        // tier 2's does.
        assert_eq!(
            col_of(b1, 0, "GB 3RD & 2 AT CHI 41"),
            col_of(b2, 0, "GB 3RD & 2 AT CHI 41"),
            "the fragment shares tier 2's column\n{text1}\n---\n{text2}"
        );
    }

    #[test]
    fn the_mark_column_is_column_zero_in_every_tier() {
        // One mark column — tier 1's taller (3-row) layout must
        // not shift the mark off x==0, same as tiers 2 and 3.
        let game = live_game("DAL", "PHI");
        let hot = RowCtx { hot: true, ..ctx() };
        let t1 = render(120, 3, &game, &hot, draw_tier1);
        let t2 = render(120, 1, &game, &hot, draw_tier2);
        let t3 = render(120, 1, &game, &hot, draw_tier3);
        let b1 = t1.backend().buffer();
        let b2 = t2.backend().buffer();
        // Tier 3's mark is a dot (no hotness reported by a final/later row),
        // so only its position — not its glyph or color — is compared here.
        let b3 = t3.backend().buffer();
        assert_eq!(b1[(0, 0)].symbol(), "▌", "tier1 mark at (0,0)");
        assert_eq!(
            b1[(0, 0)].fg,
            theme::current().roles().hot,
            "tier1 mark is hot"
        );
        assert_eq!(b2[(0, 0)].symbol(), "▌", "tier2 mark at (0,0)");
        assert_eq!(
            b2[(0, 0)].fg,
            theme::current().roles().hot,
            "tier2 mark is hot"
        );
        assert_eq!(b3[(0, 0)].symbol(), "·", "tier3 mark at (0,0)");
    }

    #[test]
    fn two_char_abbrs_occupy_the_three_char_cell() {
        // Abbrs pad to a fixed cell so the score column never
        // shifts with the abbr's length — KC (2 chars) vs BUF (3 chars).
        let r = theme::current().roles();
        let short = tier2_game_with("KC", "TB");
        let long = tier2_game_with("BUF", "MIA");
        let a = render(120, 1, &short, &ctx(), draw_tier2);
        let b = render(120, 1, &long, &ctx(), draw_tier2);
        let (ba, bb) = (a.backend().buffer(), b.backend().buffer());
        let score_x = |buf: &Buffer| -> u16 {
            (0..buf.area().width)
                .find(|&x| buf[(x, 0)].fg == r.digits)
                .unwrap()
        };
        assert_eq!(
            score_x(ba),
            score_x(bb),
            "the score column doesn't move with abbr length"
        );
        // KC's cell pads right with a space, one past the "KC" glyphs.
        let kc_end = col_of(ba, 0, "KC").unwrap() + 2;
        assert_eq!(
            ba[(kc_end, 0)].symbol(),
            " ",
            "KC's cell pads right with a space"
        );
    }

    fn tier2_game_with(away: &str, home: &str) -> Game {
        let mut g = tier2_game();
        g.away = team(away, [0, 34, 68]);
        g.home = team(home, [0, 76, 84]);
        g
    }

    #[test]
    fn no_row_size_panics_and_none_blanks_the_score() {
        let game = live_game("DAL", "PHI");
        for (w, h) in [(1u16, 1u16), (4, 1), (12, 2), (40, 3), (60, 1), (119, 3)] {
            for draw in [
                draw_tier1 as fn(&mut ratatui::Frame, Rect, &Game, &RowCtx),
                draw_tier2,
                draw_tier3,
            ] {
                let term = render(w, h, &game, &ctx(), draw);
                let buf = term.backend().buffer();
                let text = text_of(buf);
                if w >= 24 {
                    // Either form counts — text digits, or amber glyph cells.
                    let glyphs = cells_with_fg(buf, *buf.area(), theme::current().roles().digits);
                    assert!(
                        text.contains("17") || glyphs >= 4,
                        "{w}x{h} must still show a score in some form\n{text}"
                    );
                }
            }
        }
    }
}
