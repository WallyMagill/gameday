//! `:tv` — the jumbotron (spec §3 TV). One game fills the screen; everything
//! else that is live rides a one-row-each strip along the bottom.
//!
//! Three rules this surface is built around:
//!
//! * **One game, drawn by the hero.** TV owns no score formatter of its own —
//!   it hands [`hero::draw_hero`] a tall area and Full digits, which is the
//!   same block the board draws, so a score can never render two ways (spec
//!   §1's hard rule).
//! * **The screen switches on an EVENT, never on a timer.** Nothing here
//!   decides what is shown: `App::tv_shown` is moved by `OrderState::on_event`
//!   (`App::tv_follow`) or by `n`. Between events the strip's right edge
//!   names the game that is about to take over — `next cut: CHW 2 HOU 3` —
//!   and the picture holds.
//! * **Selection is inert.** j/k move nothing here and no row registers a
//!   click zone: there is nothing to select on a jumbotron.

use crate::app::App;
use crate::board::{hero, rows};
use crate::domain::Game;
use crate::text::truncate;
use crate::theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Plays under the hero, newest first — spec §3's "last three plays with
/// clock stamps".
const PLAY_ROWS: u16 = 3;

/// A linescore is a header plus both sides or it is nothing (a headless
/// strip is worse than no strip) — [`crate::board::linescore::ROWS`].
const LINESCORE_ROWS: u16 = crate::board::linescore::ROWS;

/// What the hero owes before TV spends a row on anything else: the nameplate,
/// the 8 rows of `PixelSize::Full` digits (`tiles::glyph_cell().1`), and
/// the fragment and meter lines under them. Everything below is charged
/// against what is left over, hero first — the same "digits are charged
/// first" discipline the board's brackets follow (ruling R29).
const HERO_MIN_ROWS: u16 = 11;

/// The same floor at the jumbotron rung (v3.3 §2): 1 nameplate + the doubled
/// digit rows + the fragment and meter lines. R29 again — on a terminal tall
/// enough for the big form, the big form is charged before the linescore and
/// the plays, not after them.
///
/// The digit half is [`hero::DOUBLE_MIN_ROWS`] itself, never a copy of it:
/// the gate and the form's height are one number in the hero, and
/// `the_jumbo_floor_reads_the_heros_own_gate` pins this arithmetic to it.
const HERO_JUMBO_ROWS: u16 = 1 + hero::DOUBLE_MIN_ROWS + 2;

/// Air the digit band keeps around the doubled digits: one row, under the
/// nameplate. Only one, because the glyph cell already carries a baseline row
/// of its own at the bottom — doubled, that is two rows of air under the
/// digits for free, and a second reserved row here would only widen the gap
/// the hero is trying to close.
const BAND_AIR: u16 = 1;

/// The body height at which TV asks for the jumbotron floor: the doubled
/// hero, the three plays and a whole linescore, so the big digits are never
/// bought by evicting the two blocks under them.
const JUMBO_BODY_ROWS: u16 = HERO_JUMBO_ROWS + PLAY_ROWS + LINESCORE_ROWS;

/// Where the strip splits into two columns. Receipt: a tier-2 row spends 38
/// cells on its grid before the situation text starts (`rows::TEXT_X`), so
/// two columns plus the gutter need 78 before a single word of fragment —
/// 100 is where both columns still read as rows rather than as stubs, and it
/// is the same bracket the board uses to flank its hero.
const STRIP_TWO_COL_COLS: u16 = 100;

/// Air between the strip's two columns.
const STRIP_GUTTER: u16 = 2;

/// Rows one strip column may spend. The reference frame
/// (docs/research/v3-identity/tv-nfl-sunday-120x40.png) lists five games per
/// column and captions the remainder; more than that and the strip is
/// competing with the game it is supposed to be a footnote to.
const STRIP_MAX_ROWS: usize = 5;

/// The play-clock stamp column: `12:34` right-aligned, plus a column of air.
const STAMP_W: usize = 6;

/// The strip never takes more than this share of the body: the whole point of
/// TV is the one game, so a 20-game slate shrinks its own strip, never the
/// jumbotron.
const STRIP_MAX_SHARE: u16 = 2;

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();
    let now = app.now();
    let d = app.derived();

    let shown = app.tv_shown_in(d);
    let slate = App::tv_slate(d);
    let Some(game) = shown
        .as_ref()
        .and_then(|id| slate.iter().find(|g| g.id == *id).copied())
    else {
        frame.render_widget(
            Paragraph::new("nothing is live · esc board")
                .style(Style::default().fg(r.dim).bg(r.ground))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };

    // ----------------------------------------------------- the band's rows
    // Spec §3: the scoring band is reserved here exactly as it is on the
    // board (`layout::TierPlan::band_rows`) — TV is a live surface by
    // definition (it drew a game, so something is live), and until this
    // reservation existed a band fired over TV shoved the jumbotron down two
    // rows and squeezed the strip. `App::draw` paints the band into these
    // rows; when nothing is firing they are air above the nameplate, which is
    // where a jumbotron wants air anyway.
    let band_rows = crate::board::layout::plan(
        area.width,
        area.height,
        d.in_play.len().max(1),
        d.finals.len(),
        d.later.len(),
        0,
    )
    .band_rows;
    let area = Rect {
        y: area.y + band_rows,
        height: area.height - band_rows,
        ..area
    };

    // ----------------------------------------------------------- the strip
    let others: Vec<&&Game> = slate.iter().filter(|g| g.id != game.id).collect();
    let columns = strip_columns(area.width);
    let strip_rows = if others.is_empty() {
        0
    } else {
        let per_column = others.len().div_ceil(columns as usize).min(STRIP_MAX_ROWS) as u16;
        (per_column + 1).min(area.height / STRIP_MAX_SHARE)
    };
    let body = Rect {
        height: area.height - strip_rows,
        ..area
    };

    // ------------------------------------------------------------ the hero
    // Row budget, in keep order: the hero's floor first, then the plays, then
    // the linescore.
    let plays: Vec<&crate::domain::Play> =
        game.last_plays.iter().take(PLAY_ROWS as usize).collect();
    let floor = if body.height >= JUMBO_BODY_ROWS {
        HERO_JUMBO_ROWS
    } else {
        HERO_MIN_ROWS
    };
    let mut spare = body.height.saturating_sub(floor);
    let plays_rows = spare.min(plays.len() as u16);
    spare -= plays_rows;
    let linescore =
        crate::board::linescore::linescore_lines(game, &th).filter(|_| spare >= LINESCORE_ROWS);
    let ls_rows = if linescore.is_some() {
        LINESCORE_ROWS
    } else {
        0
    };
    // What the hero can have, and what it actually wants. Before v3.3 these
    // were the same number: every leftover row went into the digit band, so a
    // 40-row terminal centered 8 rows of digits inside a 25-row band and left
    // fifteen dead rows around them (the design review's "TV is the weakest
    // frame"). The band now takes the doubled form plus `BAND_AIR` and stops;
    // the rows it declines go to the gap above the linescore, where the
    // reference frame's air is (docs/research/v3-identity/tv-nfl-sunday-120x40.png).
    let can = body.height - plays_rows - ls_rows;
    let options = u16::from(hero::fragment_line(game).is_some())
        + u16::from(crate::tiles::meter_line(game, body.width as usize).is_some());
    let wants = 1 + hero::digit_rows_in(hero::DOUBLE_MIN_ROWS, true) + BAND_AIR + options;
    let hero_rows = can.min(wants.max(HERO_MIN_ROWS));

    let watch = crate::rank::watchability(game, now);
    // TV draws the play block itself (three plays, stamped), so the hero is
    // handed the game without its plays rather than drawing a fourth,
    // unstamped copy of the newest one. Nothing else in the hero reads
    // `last_plays`, and the clone is local to this frame.
    //
    // This seam is load-bearing twice: an empty `last_plays` also drops the
    // play row from the hero's R30 keep-order budget, so the row it would
    // have taken goes back to the digit band (`hero.rs`'s `band_rows`) —
    // which is where TV's air around the digits comes from. Anyone changing
    // `HeroPlan` needs both halves.
    let mut headline = game.clone();
    headline.last_plays.clear();
    hero::draw_hero(
        frame,
        Rect {
            height: hero_rows,
            ..body
        },
        &headline,
        &hero::HeroPlan {
            // The jumbotron always asks for the big digits; a terminal too
            // short for them falls through the hero's own ladder.
            digits_full: true,
            chip: watch.chip,
            now,
            pinned: app.pins.iter().any(|p| p.game_id == game.id),
            favorite: app.is_my_game(game),
            // Spec §0: TV, the cut and the row tiers stay logo-free. The
            // logo study measured two 40-col marks collapsing the digits
            // from 15 rows to 6 and evicting the play feed, and the
            // reference frame has no flanks.
            show_logos: false,
            // Nothing is selectable in TV, so no caret.
            selected: false,
        },
    );

    // The linescore and the plays hang off the strip, not off the hero: the
    // three-play feed sits directly above the `ALSO LIVE` rule the way the
    // reference frame has it, and any row the hero declined shows up as one
    // gap between the two blocks rather than as air inside the digit band.
    let mut y = body.bottom() - plays_rows - ls_rows;
    if let Some(lines) = linescore {
        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            Rect {
                y,
                height: ls_rows,
                ..body
            },
        );
        y += ls_rows;
    }
    for play in plays.iter().take(plays_rows as usize) {
        // One stamp formatter for every feed (`tiles::play_stamp`): TV and
        // the zoom must never disagree about what a baseball play's `[B7]`
        // looks like.
        let stamp = crate::tiles::play_stamp(play);
        let text_room = (body.width as usize).saturating_sub(STAMP_W + 6);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{stamp:>STAMP_W$}"),
                    Style::default().fg(th.clock()),
                ),
                Span::styled("  ▸ ", Style::default().fg(r.dim)),
                Span::styled(
                    truncate(&play.text, text_room),
                    Style::default().fg(if play.scoring { r.hot } else { r.ink }),
                ),
            ])),
            Rect {
                x: body.x + 2,
                y,
                width: body.width.saturating_sub(2),
                height: 1,
            },
        );
        y += 1;
    }

    if strip_rows > 0 {
        draw_strip(
            app,
            frame,
            Rect {
                y: body.bottom(),
                height: strip_rows,
                ..area
            },
            &others,
            now,
        );
    }
}

/// Columns the strip lays its rows out in — two once the width can carry two
/// readable tier-2 rows side by side ([`STRIP_TWO_COL_COLS`]). One place, so
/// [`draw`]'s row budget and [`draw_strip`]'s layout can never disagree about
/// how tall the strip is.
fn strip_columns(width: u16) -> u16 {
    if width >= STRIP_TWO_COL_COLS {
        2
    } else {
        1
    }
}

/// `ALSO LIVE ──── 5 GAMES · next cut: CHW 2 HOU 3`, then one tier-2 row per
/// game. Same rule-and-caption grammar the board's sections use, and the same
/// row renderer — the strip is the board's list, just shorter.
fn draw_strip(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    others: &[&&Game],
    now: time::OffsetDateTime,
) {
    let th = theme::current();
    let r = th.roles();
    let d = app.derived();
    let label = "ALSO LIVE";
    // Count what the strip SHOWS, not what the slate holds: past two full
    // columns the rest never render, and a caption that says "14 GAMES" over
    // ten rows is the frame lying about itself (review finding 3).
    let columns = strip_columns(area.width);
    let per_column = (area.height as usize).saturating_sub(1).min(STRIP_MAX_ROWS);
    let shown = others.len().min(per_column * columns as usize);
    let mut caption = if shown < others.len() {
        format!("{shown} GAMES · {} MORE", others.len() - shown)
    } else {
        format!("{shown} GAMES")
    };
    // The next cut rides the caption's right edge, because that is where a
    // section says what it is about — and the switch itself waits for an
    // event (spec §3), so this is the only warning there is.
    if let Some(next) = app.tv_next_cut_in(d) {
        caption = format!(
            "{caption} · next cut: {} {} {} {}",
            next.away.abbr, next.away_score, next.home.abbr, next.home_score
        );
    }
    let w = area.width as usize;
    let dashes = w.saturating_sub(label.chars().count() + caption.chars().count() + 3);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                label,
                Style::default().fg(r.cool).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("─".repeat(dashes), Style::default().fg(r.dim)),
            Span::raw(" "),
            Span::styled(caption, Style::default().fg(r.dim)),
        ])),
        Rect { height: 1, ..area },
    );
    // Two columns where the width affords them (`STRIP_TWO_COL_COLS`), filled
    // down the left column first: the strip is ranked, and a reader who stops
    // after three rows has still read the three most watchable games.
    let col_w = (area.width - STRIP_GUTTER * (columns - 1)) / columns;
    for (i, game) in others.iter().take(shown).enumerate() {
        let (col, row) = ((i / per_column) as u16, (i % per_column) as u16);
        let watch = crate::rank::watchability(game, now);
        rows::draw_tier2(
            frame,
            Rect {
                x: area.x + col * (col_w + STRIP_GUTTER),
                y: area.y + 1 + row,
                width: col_w,
                height: 1,
            },
            game,
            &rows::RowCtx {
                // The mark still says which strip games are hot: it is the
                // reason you would reach for `n`.
                hot: watch.hot,
                chip: watch.chip,
                // No nudges and no selection on the strip — TV has no j/k, so
                // a caret there would point at a key that does nothing.
                nudge: None,
                selected: false,
                pinned: app.pins.iter().any(|p| p.game_id == game.id),
                league_tag: d.mixed,
                now,
                // The strip is tier-2, which never reads the ladder.
                leaders_line: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review finding 1: TV's stack budget is arithmetic on the hero's own
    /// jumbotron gate, not a copy of the number. Move `DOUBLE_MIN_ROWS` and
    /// this fails here rather than silently budgeting a band the hero will
    /// not fill.
    #[test]
    fn the_jumbo_floor_reads_the_heros_own_gate() {
        assert_eq!(
            HERO_JUMBO_ROWS,
            1 + hero::DOUBLE_MIN_ROWS + 2,
            "the jumbotron floor is a nameplate, the doubled digits and the two option rows"
        );
        // ...and the hero's own answer to "how tall is the score in this
        // band" is what TV asks for, at both sides of the gate.
        assert_eq!(
            hero::digit_rows_in(hero::DOUBLE_MIN_ROWS, true),
            hero::digit_rows(true) * 2,
            "a band at the gate gets the doubled form"
        );
        assert_eq!(
            hero::digit_rows_in(hero::DOUBLE_MIN_ROWS - 1, true),
            hero::digit_rows(true),
            "one row under the gate is the single form"
        );
    }

    /// The strip is bounded by [`STRIP_MAX_ROWS`] per column, so a big slate
    /// has rows it never draws — the caption counts what is on screen.
    #[test]
    fn the_strip_has_a_ceiling_the_caption_has_to_respect() {
        assert_eq!(
            strip_columns(STRIP_TWO_COL_COLS),
            2,
            "the gate width runs two columns"
        );
        assert_eq!(
            strip_columns(STRIP_TWO_COL_COLS - 1),
            1,
            "under the gate, one"
        );
        assert_eq!(
            strip_columns(120) as usize * STRIP_MAX_ROWS,
            10,
            "ten rows is the ceiling at 120 cols"
        );
    }
}
