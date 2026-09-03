//! The three row tiers of the ranked board (spec §1): everything under the
//! hero is one ranked list, and a game's tier is how loudly that list says
//! it. Tier 1 is a three-row block with sextant digits, tier 2 is one line,
//! tier 3 is one dim line for finals and later games.
//!
//! Three rules hold across all three tiers:
//!
//! * **Two gutters, always reserved** ([`GUTTER`]). Column 0 is the hot mark
//!   — `▌` in `hot` when the row is hot, `dim` when it isn't, and *only*
//!   those two states. Columns 2–3 are the nudge. A row that rises shows
//!   `↑2` there and does not move by a single cell (A′ calls #6/#7: rank and
//!   hotness are different facts, so a nudge may never restyle the mark or
//!   reflow the line).
//! * **Amber is the score's color.** `roles.digits`, bold, and nothing else
//!   in a row wears it. The one exception to the monochrome rest is a pinned
//!   game's abbrs, and only at `TeamColorScope::HeroMarks` (spec §6).
//! * **Fixed columns, dropped from the right.** The grid below is measured
//!   off the A′ frame; a column whose x is past the area's edge is simply not
//!   drawn, which is how a row narrows instead of wrapping or clipping mid-word.
//!
//! Nothing here reads `App`, a clock, or a tick: hotness, the nudge, pin
//! state and `now` all arrive in [`RowCtx`].

use crate::domain::{Game, League, Status};
use crate::text::{fmt_start, truncate};
use crate::theme::{self, Roles, TeamColorScope};
use crate::tiles;
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
}

/// The two fixed gutters every tier-1/2 row reserves: 2-cell hot mark,
/// 2-cell nudge.
pub const GUTTER: u16 = 4;

/// Column of the nudge inside [`GUTTER`]: the mark takes 0 and its air, the
/// nudge takes the rest.
const NUDGE_X: u16 = 2;

/// Largest climb the 2-cell nudge gutter can say truthfully (ruling R31):
/// `↑9` means "rose 9 or more places".
const NUDGE_MAX: usize = 9;

// ---------------------------------------------------------------- the grid
//
// Column offsets from the row's left edge, measured off the A′ reference
// frame at 120 columns (docs/research/v3-identity/nfl-sunday-120x40.png:
// `GB 13 CHI 10  Q3 4:20  NFL  GB 3RD & 2 AT CHI 41`, and the LATER rows
// whose start times and league tags share the same columns). ±1 column of
// pixel-measurement error; the frame's *ordering and alignment* is the
// contract, the exact offsets are ours.

/// Abbr field width: four cells holds every abbr the mapper emits (`WSH`,
/// `MTL`, and soccer's four-letter clubs).
const ABBR_W: u16 = 4;
/// Score field width: three cells for a college basketball 100+.
const SCORE_W: u16 = 3;

const AWAY_ABBR_X: u16 = GUTTER;
const AWAY_SCORE_X: u16 = 9;
const HOME_ABBR_X: u16 = 13;
const HOME_SCORE_X: u16 = 18;
/// Clock/status column — wide enough for baseball's `BOT 7TH` and soccer's
/// `2ND HALF` without touching the league tag. The text is truncated one cell
/// short of the field so a full-width state (`SEP 13 8:20`, a LATER row a week
/// out) keeps a column of air before the tag instead of reading `8:20NFL`.
const CLOCK_X: u16 = 22;
const CLOCK_W: u16 = 11;
const LEAGUE_X: u16 = 33;
const LEAGUE_W: u16 = 4;
/// Where a row's prose starts: the headline, the fragment, the broadcast.
const TEXT_X: u16 = 38;
/// Broadcast field on a LATER row, before the odds (`FOX`, `ESPN`, `PRIME`).
const BCAST_W: u16 = 7;

// Tier 1 runs its own grid: the sextant digits are 4 cells per glyph, so the
// two score fields are 12 wide (three digits) and everything after them sits
// further right than the one-line tiers.
const T1_ABBR_X: u16 = GUTTER;
const T1_AWAY_DIGITS_X: u16 = 9;
/// Three sextant digits at `tiles::glyph_cell(false).0` = 4 cells each.
const DIGIT_FIELD_W: u16 = 12;
const T1_HOME_ABBR_X: u16 = 22;
const T1_HOME_DIGITS_X: u16 = 27;
const T1_CLOCK_X: u16 = 40;
const T1_TEXT_X: u16 = 53;
/// The tier-1 state chip's field. It rides on row 1, where nothing sits
/// between the clock column and the play text, so it gets the whole gap
/// rather than the clock's own [`CLOCK_W`] — at 11 cells the longest chips
/// `rank::watchability` emits ("BASES LOADED", "TYING ON 3RD", "GO-AHEAD 3RD", all 12) were
/// clipped to a word that isn't one.
const T1_CHIP_W: u16 = T1_TEXT_X - T1_CLOCK_X;
/// A sextant glyph is three rows tall; a shorter block falls to text.
const SEXTANT_ROWS: u16 = 3;

/// Render `line` into the column `x..x+w` of row `y` (both relative to
/// `area`). A column that starts past the right edge is dropped — that is
/// how a row narrows on a small terminal, rather than wrapping.
fn col(frame: &mut Frame, area: Rect, x: u16, w: u16, y: u16, align: Alignment, line: Line<'static>) {
    if x >= area.width || y >= area.height || w == 0 {
        return;
    }
    let width = w.min(area.width - x);
    let rect = Rect { x: area.x + x, y: area.y + y, width, height: 1 };
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

/// A team abbr. Team color only for a pinned game, and only where the theme
/// allows marks to carry it (spec §6); everything else is ink.
///
/// Padded to a 3-cell minimum (spec v3.3 §4: `format!("{:<3}", abbr)`) so a
/// two-letter abbr (`KC`) fills the same cell a three-letter one (`BUF`)
/// does — the pad is part of the styled span, not a coincidence of the
/// field's blank background.
fn abbr_span(game_pinned: bool, team: &crate::domain::Team, ctx: &RowCtx) -> Line<'static> {
    let th = theme::current();
    let color = if game_pinned && th.roles().team == TeamColorScope::HeroMarks {
        th.art_color(team.color)
    } else {
        ink(ctx)
    };
    let padded = format!("{:<3}", team.abbr);
    Line::from(Span::styled(padded, Style::default().fg(color).add_modifier(Modifier::BOLD)))
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
    truncate(&state_text_raw(game, now), CLOCK_W as usize - 1)
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
fn situation_summary(game: &Game) -> Option<String> {
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

/// The hot mark, column 0 — the one cell every tier draws identically (spec
/// v3.3 §4: one mark column). `▌` in `hot`/`dim` for a bar row, `·` in `dim`
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
        col(frame, area, 0, 1, y, Alignment::Left, Line::from(Span::styled(glyph, Style::default().fg(color))));
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
        Some(Span::styled("▸", Style::default().fg(theme::current().bright).add_modifier(Modifier::BOLD)))
    } else {
        // Ruling R31: the gutter is two cells, so the DISPLAYED climb clamps
        // at 9 — `↑9` reads "rose 9 or more". Clipping `↑12` to `↑1` would
        // print a number that never happened; an understated climb is the
        // honest failure.
        ctx.nudge.map(|n| Span::styled(format!("↑{}", n.min(NUDGE_MAX)), Style::default().fg(r.digits)))
    };
    if let Some(span) = mark {
        col(frame, area, NUDGE_X, GUTTER - NUDGE_X, 0, Alignment::Left, Line::from(span));
    }
}

/// Tier 1: a 3-row promoted row (sextant digits) — abbr+league stack,
/// digits, clock+state column, fragments, last play (A′ tier-1 rows).
pub fn draw_tier1(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();
    gutters(frame, area, ctx, SEXTANT_ROWS, true);

    col(frame, area, T1_ABBR_X, ABBR_W, 0, Alignment::Right, abbr_span(ctx.pinned, &game.away, ctx));
    col(frame, area, T1_HOME_ABBR_X, ABBR_W, 0, Alignment::Left, abbr_span(ctx.pinned, &game.home, ctx));
    if ctx.league_tag {
        let tag = Span::styled(game.league.slug().to_uppercase(), Style::default().fg(r.dim));
        col(frame, area, T1_ABBR_X, ABBR_W, 1, Alignment::Right, Line::from(tag));
    }

    digits(frame, area, game.away_score, T1_AWAY_DIGITS_X, Alignment::Right, r.digits);
    digits(frame, area, game.home_score, T1_HOME_DIGITS_X, Alignment::Left, r.digits);

    let state = state_text(game, ctx.now);
    if !state.is_empty() {
        let span = Span::styled(state, Style::default().fg(ink(ctx)).add_modifier(Modifier::BOLD));
        col(frame, area, T1_CLOCK_X, CLOCK_W, 0, Alignment::Left, Line::from(span));
    }
    // The state chip sits directly under the clock (A′ frame: red `2-MIN`
    // beneath `Q4 0:48`). Plain hot text, not a filled block — the filled
    // chip is the hero's alone (spec §1).
    if let Some(chip) = ctx.chip {
        let span = Span::styled(chip, Style::default().fg(r.hot).add_modifier(Modifier::BOLD));
        col(frame, area, T1_CLOCK_X, T1_CHIP_W, 1, Alignment::Left, Line::from(span));
    }
    let room = (area.width.saturating_sub(T1_TEXT_X)) as usize;
    if let Some(fragment) = situation_summary(game) {
        let span = Span::styled(truncate(&fragment, room), Style::default().fg(r.dim));
        col(frame, area, T1_TEXT_X, area.width, 0, Alignment::Left, Line::from(span));
    }
    if let Some(play) = game.last_plays.first() {
        let line = Line::from(vec![
            Span::styled("▸ ", Style::default().fg(r.dim)),
            Span::styled(truncate(&play.text, room.saturating_sub(2)), Style::default().fg(ink(ctx))),
        ]);
        col(frame, area, T1_TEXT_X, area.width, 1, Alignment::Left, line);
    }
}

/// One score as sextant glyphs in a [`DIGIT_FIELD_W`] field at `x`, aligned
/// inside it. A block too short (or too narrow) for the glyphs prints the
/// number as bold amber text instead — the hero's ladder, one rung shorter:
/// never a blank score.
fn digits(frame: &mut Frame, area: Rect, value: u16, x: u16, align: Alignment, color: Color) {
    if x >= area.width {
        return;
    }
    let field = DIGIT_FIELD_W.min(area.width - x);
    let (gw, gh) = tiles::glyph_cell(false);
    let want = value.to_string().len() as u16 * gw;
    if area.height >= gh && want <= field {
        let gx = match align {
            Alignment::Right => x + field - want,
            _ => x,
        };
        let rect = Rect { x: area.x + gx, y: area.y, width: want, height: gh };
        if tiles::digit_glyphs(frame, rect, value, color, false) {
            return;
        }
    }
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    col(frame, area, x, field, 0, align, Line::from(Span::styled(value.to_string(), style)));
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
        let span = Span::styled(state, Style::default().fg(ink(ctx)).add_modifier(Modifier::BOLD));
        col(frame, area, CLOCK_X, CLOCK_W, 0, Alignment::Left, Line::from(span));
    }
    league_tag(frame, area, game, ctx);
    if let Some(fragment) = situation_summary(game) {
        let room = (area.width.saturating_sub(TEXT_X)) as usize;
        let span = Span::styled(truncate(&fragment, room), Style::default().fg(r.dim));
        col(frame, area, TEXT_X, area.width, 0, Alignment::Left, Line::from(span));
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
        col(frame, area, AWAY_ABBR_X, ABBR_W, 0, Alignment::Right, abbr_span(ctx.pinned, &game.away, ctx));
        let at = Span::styled("@", Style::default().fg(r.dim));
        col(frame, area, AWAY_SCORE_X, SCORE_W, 0, Alignment::Center, Line::from(at));
        col(frame, area, HOME_ABBR_X, ABBR_W, 0, Alignment::Left, abbr_span(ctx.pinned, &game.home, ctx));
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
            col(frame, area, TEXT_X, BCAST_W, 0, Alignment::Left, Line::from(span));
        }
        if let Some(odds) = game.odds.as_deref().filter(|s| !s.is_empty()) {
            let x = TEXT_X + BCAST_W;
            let room = (area.width.saturating_sub(x)) as usize;
            let span = Span::styled(truncate(odds, room), Style::default().fg(r.dim));
            col(frame, area, x, area.width, 0, Alignment::Left, Line::from(span));
        }
        return;
    }
    // A final's headline: the newest scoring play (the list is oldest first),
    // else whatever the situation still says, else nothing.
    let headline = game
        .scoring_plays
        .last()
        .map(|p| p.text.clone())
        .or_else(|| situation_summary(game))
        .unwrap_or_default();
    if !headline.is_empty() {
        let room = (area.width.saturating_sub(TEXT_X)) as usize;
        let span = Span::styled(truncate(&headline, room), Style::default().fg(r.dim));
        col(frame, area, TEXT_X, area.width, 0, Alignment::Left, Line::from(span));
    }
}

/// `GB 13 CHI 10` on the shared one-line grid.
fn pair_line(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx, strong: bool) {
    let r = theme::current().roles();
    col(frame, area, AWAY_ABBR_X, ABBR_W, 0, Alignment::Right, abbr_span(ctx.pinned, &game.away, ctx));
    col(frame, area, AWAY_SCORE_X, SCORE_W, 0, Alignment::Right, score_span(game.away_score, &r, strong));
    col(frame, area, HOME_ABBR_X, ABBR_W, 0, Alignment::Left, abbr_span(ctx.pinned, &game.home, ctx));
    col(frame, area, HOME_SCORE_X, SCORE_W, 0, Alignment::Right, score_span(game.home_score, &r, strong));
}

fn league_tag(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx) {
    if !ctx.league_tag {
        return;
    }
    let r = theme::current().roles();
    let span = Span::styled(game.league.slug().to_uppercase(), Style::default().fg(r.dim));
    col(frame, area, LEAGUE_X, LEAGUE_W, 0, Alignment::Left, Line::from(span));
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
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The columns `needle` occupies in row `y`, or None when it isn't there.
    fn col_of(buf: &Buffer, y: u16, needle: &str) -> Option<u16> {
        let row: Vec<&str> = (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect();
        let joined: String = row.concat();
        joined.find(needle).map(|byte| joined[..byte].chars().count() as u16)
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
        theme::install(theme::Entry { name, theme: th, user: true });
        theme::set_current("marks").unwrap();
    }

    #[test]
    fn tier2_line_layout_and_amber_scores() {
        let game = tier2_game();
        let term = render(120, 1, &game, &ctx(), draw_tier2);
        let buf = term.backend().buffer();
        let r = theme::current().roles();
        let text = text_of(buf);

        assert_eq!(buf[(0, 0)].symbol(), "▌", "the hot mark owns column 0\n{text}");
        // The frame's grid: abbr right-aligned into the gutter's shoulder,
        // score right-aligned two columns later, home pair mirrored.
        // spec v3.3 §4: the abbr pads to a 3-cell minimum, so a 2-char abbr
        // right-aligned in the 4-cell field now starts one column left of
        // where the unpadded text used to (ABBR_W-3, not ABBR_W-2).
        assert_eq!(col_of(buf, 0, "GB"), Some(AWAY_ABBR_X + ABBR_W - 3), "away abbr right-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "13"), Some(AWAY_SCORE_X + SCORE_W - 2), "away score right-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "CHI"), Some(HOME_ABBR_X), "home abbr left-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "10"), Some(HOME_SCORE_X + SCORE_W - 2), "home score right-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "Q3 4:20"), Some(CLOCK_X), "clock column\n{text}");

        // Amber, bold, and only on the scores: the two abbrs stay ink.
        for x in [AWAY_SCORE_X + 1, AWAY_SCORE_X + 2, HOME_SCORE_X + 1, HOME_SCORE_X + 2] {
            let c = &buf[(x, 0)];
            assert_eq!(c.fg, r.digits, "score cell {x} is amber ({:?})\n{text}", c.symbol());
            assert!(c.modifier.contains(Modifier::BOLD), "score cell {x} is bold\n{text}");
        }
        assert_eq!(buf[(HOME_ABBR_X, 0)].fg, r.ink, "an unpinned abbr is ink\n{text}");

        // The league tag is a mixed-list decision, not a row decision.
        assert_eq!(col_of(buf, 0, "NFL"), Some(LEAGUE_X), "league tag column\n{text}");
        let mut plain = ctx();
        plain.league_tag = false;
        let term = render(120, 1, &game, &plain, draw_tier2);
        let bare = text_of(term.backend().buffer());
        assert!(!bare.contains("NFL"), "no league tag when the list is single-league\n{bare}");
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
            let c = RowCtx { hot, nudge, ..ctx() };
            let term = render(120, 1, &game, &c, draw_tier2);
            let buf = term.backend().buffer();
            assert_eq!(buf[(0, 0)].symbol(), "▌", "hot={hot} nudge={nudge:?}: the mark is always ▌");
            assert_eq!(buf[(0, 0)].fg, want, "hot={hot} nudge={nudge:?}: two ink states only");
        }
    }

    #[test]
    fn nudge_gutter_never_shifts_the_row() {
        let game = tier2_game();
        let r = theme::current().roles();
        let quiet = render(120, 1, &game, &ctx(), draw_tier2);
        let risen = render(120, 1, &game, &RowCtx { nudge: Some(2), ..ctx() }, draw_tier2);
        let (a, b) = (quiet.backend().buffer(), risen.backend().buffer());
        assert_eq!(col_of(a, 0, "GB"), col_of(b, 0, "GB"), "the abbr keeps its column");
        for x in GUTTER..120 {
            assert_eq!(a[(x, 0)].symbol(), b[(x, 0)].symbol(), "cell {x} moved with the nudge");
            assert_eq!(a[(x, 0)].fg, b[(x, 0)].fg, "cell {x} changed color with the nudge");
        }
        // The gutter is reserved either way: empty without a nudge, ↑2 with one.
        assert_eq!(col_of(b, 0, "↑2"), Some(NUDGE_X), "the nudge lives in its own gutter");
        assert_eq!(b[(NUDGE_X, 0)].fg, r.digits, "the nudge is amber");
        for x in NUDGE_X..GUTTER {
            assert_eq!(a[(x, 0)].symbol(), " ", "no nudge leaves the gutter empty at {x}");
        }
    }

    #[test]
    fn a_big_nudge_clamps_to_nine_rather_than_lying() {
        // Ruling R31: two cells cannot say "12", and `↑1` is a number that
        // never happened. `↑9` understates; it never fabricates.
        let game = tier2_game();
        let r = theme::current().roles();
        for (nudge, want) in [(2usize, "↑2"), (9, "↑9"), (12, "↑9"), (137, "↑9")] {
            let term = render(120, 1, &game, &RowCtx { nudge: Some(nudge), ..ctx() }, draw_tier2);
            let buf = term.backend().buffer();
            let got: String = (NUDGE_X..GUTTER).map(|x| buf[(x, 0)].symbol()).collect();
            assert_eq!(got, want, "nudge {nudge} renders {want} in the gutter\n{}", text_of(buf));
            for x in NUDGE_X..GUTTER {
                assert_eq!(buf[(x, 0)].fg, r.digits, "nudge {nudge}: the gutter is amber at {x}");
            }
            // Whatever it says, it says it inside the gutter.
            assert_eq!(buf[(GUTTER, 0)].symbol(), " ", "nudge {nudge} must not spill past the gutter");
        }
    }

    #[test]
    fn selection_is_a_caret_in_the_gutter_and_brighter_text() {
        let game = tier2_game();
        let th = theme::current();
        let quiet = render(120, 1, &game, &ctx(), draw_tier2);
        let picked = render(120, 1, &game, &RowCtx { selected: true, ..ctx() }, draw_tier2);
        let (a, b) = (quiet.backend().buffer(), picked.backend().buffer());
        let text = text_of(b);

        assert_eq!(b[(NUDGE_X, 0)].symbol(), "▸", "the caret takes the nudge gutter\n{text}");
        assert_eq!(b[(NUDGE_X, 0)].fg, th.bright, "the caret is bright\n{text}");
        assert_eq!(a[(NUDGE_X, 0)].symbol(), " ", "an unselected row has no caret");

        // The abbrs and the clock brighten; the score keeps its amber, and
        // nothing moves.
        for needle in ["GB", "CHI"] {
            let x = col_of(b, 0, needle).unwrap();
            assert_eq!(col_of(a, 0, needle), Some(x), "selection never moves {needle}\n{text}");
            assert_eq!(b[(x, 0)].fg, th.bright, "{needle} is bright while selected\n{text}");
            assert_eq!(a[(x, 0)].fg, th.roles().ink, "{needle} is plain ink otherwise");
        }
        assert_eq!(b[(CLOCK_X, 0)].fg, th.bright, "the clock brightens too\n{text}");
        assert_eq!(b[(AWAY_SCORE_X + 2, 0)].fg, th.roles().digits, "the score stays amber\n{text}");

        // Selection wins the gutter over a nudge — one glyph, not two facts.
        let both = render(120, 1, &game, &RowCtx { selected: true, nudge: Some(3), ..ctx() }, draw_tier2);
        let c = both.backend().buffer();
        assert_eq!(c[(NUDGE_X, 0)].symbol(), "▸", "the caret outranks the nudge\n{}", text_of(c));
        assert!(!text_of(c).contains("↑3"), "no nudge beside the caret\n{}", text_of(c));
    }

    #[test]
    fn pinned_abbr_wears_team_color_others_ink() {
        install_marks_theme();
        let game = tier2_game();
        let th = theme::current();
        let r = th.roles();
        assert_eq!(r.team, theme::TeamColorScope::HeroMarks);
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        let pinned = RowCtx { pinned: true, ..ctx() };
        term.draw(|f| draw_tier2(f, f.area(), &game, &pinned)).unwrap();
        let buf = term.backend().buffer();
        let text = text_of(buf);
        let (ax, hx) = (col_of(buf, 0, "GB").unwrap(), col_of(buf, 0, "CHI").unwrap());
        assert_eq!(buf[(ax, 0)].fg, th.art_color(game.away.color), "a pinned away abbr wears its color\n{text}");
        assert_eq!(buf[(hx, 0)].fg, th.art_color(game.home.color), "a pinned home abbr wears its color\n{text}");
        // Nothing else in the row does — the scores stay amber, the clock ink.
        assert_eq!(buf[(AWAY_SCORE_X + 2, 0)].fg, r.digits, "scores are never team-colored\n{text}");
        assert_eq!(buf[(CLOCK_X, 0)].fg, r.ink, "the clock is ink\n{text}");
        assert_eq!(
            cells_with_fg(buf, Rect { x: CLOCK_X, y: 0, width: 120 - CLOCK_X, height: 1 }, th.art_color(game.away.color)),
            0,
            "team color stops at the abbr\n{text}"
        );

        // At the default `hero` scope even a pinned abbr is ink.
        theme::set_current("broadcast").unwrap();
        let mut term = Terminal::new(TestBackend::new(120, 1)).unwrap();
        term.draw(|f| draw_tier2(f, f.area(), &game, &pinned)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf[(ax, 0)].fg, theme::current().roles().ink, "hero-scope keeps rows monochrome");
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
        assert_eq!(col_of(buf, 0, "4:25 PM"), Some(CLOCK_X), "fmt_start(ctx.now) in the clock column\n{text}");
        assert!(!text.contains("2026-"), "never an ISO stamp\n{text}");
        // spec v3.3 §4: padded to a 3-cell minimum, ABBR_W-3 not ABBR_W-2.
        assert_eq!(col_of(buf, 0, "TB"), Some(AWAY_ABBR_X + ABBR_W - 3), "away abbr right-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "@"), Some(AWAY_SCORE_X + 1), "the @ takes the score column\n{text}");
        assert_eq!(col_of(buf, 0, "ATL"), Some(HOME_ABBR_X), "home abbr left-aligned\n{text}");
        assert_eq!(col_of(buf, 0, "FOX"), Some(TEXT_X), "broadcast\n{text}");
        assert_eq!(col_of(buf, 0, "TB -1.5"), Some(TEXT_X + BCAST_W), "odds\n{text}");
        assert_eq!(buf[(0, 0)].symbol(), "·", "tier 3 is a dot, never a bar\n{text}");
        assert_eq!(buf[(0, 0)].fg, theme::current().roles().dim);

        // A final row: newest scoring play as the headline, amber scores.
        let mut fin = live_game("ARS", "BHA");
        fin.league = League::Epl;
        fin.status = Status::Final;
        fin.away_score = 3;
        fin.home_score = 0;
        fin.scoring_plays = vec![
            Play { text: "Saka opens the scoring".into(), scoring: true, ..Default::default() },
            Play { text: "Ødegaard 2 assists".into(), scoring: true, ..Default::default() },
        ];
        let term = render(120, 1, &fin, &ctx(), draw_tier3);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(col_of(buf, 0, "FT"), Some(CLOCK_X), "soccer finals say FT\n{text}");
        assert_eq!(col_of(buf, 0, "EPL"), Some(LEAGUE_X), "league tag\n{text}");
        assert_eq!(col_of(buf, 0, "Ødegaard 2 assists"), Some(TEXT_X), "newest scoring play\n{text}");
        assert!(!text.contains("Saka"), "only the newest\n{text}");
        assert_eq!(buf[(AWAY_SCORE_X + 2, 0)].fg, theme::current().roles().digits, "scores stay amber\n{text}");
    }

    #[test]
    fn tier1_is_three_rows_with_sextant_digits() {
        let game = live_game("DAL", "PHI");
        let r = theme::current().roles();
        let term = render(120, 3, &game, &RowCtx { hot: true, ..ctx() }, draw_tier1);
        let buf = term.backend().buffer();
        let text = text_of(buf);

        // Sextant digits: 4x3 cells per glyph, so amber cells appear on every
        // row of the block — above the baseline, not only on it.
        let field = Rect { x: T1_AWAY_DIGITS_X, y: 0, width: DIGIT_FIELD_W, height: 3 };
        for y in 0..3 {
            let row = Rect { y, height: 1, ..field };
            assert!(cells_with_fg(buf, row, r.digits) >= 2, "sextant digit cells on row {y}\n{text}");
        }
        assert!(!text.contains("17 - 17"), "the glyph form fits at 120 cols\n{text}");

        // The stack: abbr over league tag, then the clock and the two text
        // rows the A′ frame puts to the right of the digits.
        assert_eq!(col_of(buf, 0, "DAL"), Some(T1_ABBR_X + ABBR_W - 3), "away abbr, row 0\n{text}");
        assert_eq!(col_of(buf, 1, "NFL"), Some(T1_ABBR_X + ABBR_W - 3), "league under it, row 1\n{text}");
        assert_eq!(col_of(buf, 0, "PHI"), Some(T1_HOME_ABBR_X), "home abbr, row 0\n{text}");
        assert_eq!(col_of(buf, 0, "Q4 0:48"), Some(T1_CLOCK_X), "clock column, row 0\n{text}");
        assert_eq!(col_of(buf, 0, "PHI 3RD & 6 AT DAL 38"), Some(T1_TEXT_X), "situation, row 0\n{text}");
        assert_eq!(col_of(buf, 1, "▸ Hurts hit"), Some(T1_TEXT_X), "last play, row 1\n{text}");

        // The mark is a bar down the whole block, one state.
        for y in 0..3 {
            assert_eq!(buf[(0, y)].symbol(), "▌", "the mark runs the block's height\n{text}");
            assert_eq!(buf[(0, y)].fg, r.hot, "hot row\n{text}");
        }

        // The state chip rides under the clock, in `hot` — the A′ frame's red
        // `2-MIN` beneath `Q4 0:48` (docs/research/v3-identity/nfl-sunday-120x40.png).
        let chipped = RowCtx { chip: Some("2-MIN"), ..ctx() };
        let term = render(120, 3, &game, &chipped, draw_tier1);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(col_of(buf, 1, "2-MIN"), Some(T1_CLOCK_X), "chip under the clock\n{text}");
        assert_eq!(buf[(T1_CLOCK_X, 1)].fg, r.hot, "the chip is hot\n{text}");
        assert_eq!(col_of(buf, 0, "Q4 0:48"), Some(T1_CLOCK_X), "the clock keeps row 0\n{text}");
        assert_eq!(col_of(buf, 1, "▸ Hurts hit"), Some(T1_TEXT_X), "the play keeps its column\n{text}");
        let bare = text_of(render(120, 3, &game, &ctx(), draw_tier1).backend().buffer());
        assert!(!bare.contains("2-MIN"), "no chip, no row\n{bare}");

        // The longest chip the ranker emits fits whole — the clock column's
        // 11 cells clipped "BASES LOADED" to "BASES LOADE".
        for chip in ["BASES LOADED", "TYING ON 3RD", "GO-AHEAD 3RD"] {
            let c = RowCtx { chip: Some(chip), ..ctx() };
            let term = render(120, 3, &game, &c, draw_tier1);
            let text = text_of(term.backend().buffer());
            assert!(text.contains(chip), "{chip} must not be clipped\n{text}");
        }

        // Too short for a sextant: the score is text, never blank.
        let term = render(120, 1, &game, &ctx(), draw_tier1);
        let text = text_of(term.backend().buffer());
        assert!(text.contains("17"), "a one-row tier 1 still prints its score\n{text}");
    }

    #[test]
    fn the_mark_column_is_column_zero_in_every_tier() {
        // spec v3.3 §4: one mark column — tier 1's taller (3-row) layout must
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
        assert_eq!(b1[(0, 0)].fg, theme::current().roles().hot, "tier1 mark is hot");
        assert_eq!(b2[(0, 0)].symbol(), "▌", "tier2 mark at (0,0)");
        assert_eq!(b2[(0, 0)].fg, theme::current().roles().hot, "tier2 mark is hot");
        assert_eq!(b3[(0, 0)].symbol(), "·", "tier3 mark at (0,0)");
    }

    #[test]
    fn two_char_abbrs_occupy_the_three_char_cell() {
        // spec v3.3 §4: abbrs pad to a fixed cell so the score column never
        // shifts with the abbr's length — KC (2 chars) vs BUF (3 chars).
        let r = theme::current().roles();
        let short = tier2_game_with("KC", "TB");
        let long = tier2_game_with("BUF", "MIA");
        let a = render(120, 1, &short, &ctx(), draw_tier2);
        let b = render(120, 1, &long, &ctx(), draw_tier2);
        let (ba, bb) = (a.backend().buffer(), b.backend().buffer());
        let score_x = |buf: &Buffer| -> u16 {
            (0..buf.area().width).find(|&x| buf[(x, 0)].fg == r.digits).unwrap()
        };
        assert_eq!(score_x(ba), score_x(bb), "the score column doesn't move with abbr length");
        // KC's cell pads right with a space, one past the "KC" glyphs.
        let kc_end = col_of(ba, 0, "KC").unwrap() + 2;
        assert_eq!(ba[(kc_end, 0)].symbol(), " ", "KC's cell pads right with a space");
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
