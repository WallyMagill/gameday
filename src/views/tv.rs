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
/// the 8 rows of `PixelSize::Full` digits (`tiles::glyph_cell(true).1`), and
/// the fragment and meter lines under them. Everything below is charged
/// against what is left over, hero first — the same "digits are charged
/// first" discipline the board's brackets follow (ruling R29).
const HERO_MIN_ROWS: u16 = 11;

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

    // ----------------------------------------------------------- the strip
    let others: Vec<&&Game> = slate.iter().filter(|g| g.id != game.id).collect();
    let strip_rows = if others.is_empty() {
        0
    } else {
        (others.len() as u16 + 1).min(area.height / STRIP_MAX_SHARE)
    };
    let body = Rect {
        height: area.height - strip_rows,
        ..area
    };

    // ------------------------------------------------------------ the hero
    // Row budget, in keep order: the hero's floor first, then the plays, then
    // the linescore. Whatever is left over stays with the hero, where it
    // becomes air around the digits — `draw_hero` gives unspent rows back to
    // the digit band, which is exactly the jumbotron look.
    let plays: Vec<&crate::domain::Play> = game.last_plays.iter().take(PLAY_ROWS as usize).collect();
    let mut spare = body.height.saturating_sub(HERO_MIN_ROWS);
    let plays_rows = spare.min(plays.len() as u16);
    spare -= plays_rows;
    let linescore = crate::board::linescore::linescore_lines(game, &th).filter(|_| spare >= LINESCORE_ROWS);
    let ls_rows = if linescore.is_some() { LINESCORE_ROWS } else { 0 };
    let hero_rows = body.height - plays_rows - ls_rows;

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
        Rect { height: hero_rows, ..body },
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

    let mut y = body.y + hero_rows;
    if let Some(lines) = linescore {
        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            Rect { y, height: ls_rows, ..body },
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
                Span::styled(format!("{stamp:>STAMP_W$}"), Style::default().fg(th.clock())),
                Span::styled("  ▸ ", Style::default().fg(r.dim)),
                Span::styled(
                    truncate(&play.text, text_room),
                    Style::default().fg(if play.scoring { r.hot } else { r.ink }),
                ),
            ])),
            Rect { x: body.x + 2, y, width: body.width.saturating_sub(2), height: 1 },
        );
        y += 1;
    }

    if strip_rows > 0 {
        draw_strip(
            app,
            frame,
            Rect { y: body.bottom(), height: strip_rows, ..area },
            &others,
            now,
        );
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
    let mut caption = format!("{} GAMES", others.len());
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
            Span::styled(label, Style::default().fg(r.cool).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled("─".repeat(dashes), Style::default().fg(r.dim)),
            Span::raw(" "),
            Span::styled(caption, Style::default().fg(r.dim)),
        ])),
        Rect { height: 1, ..area },
    );
    for (i, game) in others.iter().take(area.height as usize - 1).enumerate() {
        let watch = crate::rank::watchability(game, now);
        rows::draw_tier2(
            frame,
            Rect { y: area.y + 1 + i as u16, height: 1, ..area },
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
            },
        );
    }
}
