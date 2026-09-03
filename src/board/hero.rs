//! The hero block (spec §1 Hero): the one game the board is about, drawn
//! mirror-symmetric around a center column — nameplates, team-colored
//! digits, one fragment line, one meter row, one last play.
//!
//! Everything the block needs arrives in [`HeroPlan`]; the hero never reads
//! `App`, a clock, or a tick. The digit pair is the only place team color is
//! guaranteed (spec §6's identity floor), and it goes through
//! [`theme::hero_pair`] so two navy teams can never render as one.
//!
//! Two rules the layout is built around:
//!
//! * **Logos never move a digit.** The digit rects are computed first
//!   ([`score_spots`]); the flanks are whatever margin is left over, and a
//!   flank too narrow (or a team with no committed art) simply stays empty —
//!   the nameplate already carries that team's identity, so a placeholder
//!   would be noise.
//! * **The digits are charged first** (ruling R29). The band reserves the
//!   rows the caller's bracket asked for — 8 for Full, 4 for the quadrant
//!   mid form — and the fragment/meter/play rows split what is left, in that
//!   keep order (R30). [`score_block`]'s 8-row → quad → bold `24 - 21` ladder
//!   is for a bracket too short to hold the form, never something an optional
//!   row can take away.
//!
//! sitting-1 pick 1A: the mid rung is [`tiles::quad_digits`], not
//! `tui-big-text`'s sextants. The sextant form tofus on Terminal.app and did
//! not resolve into a readable number at 80×24 even where the font covered
//! it; the quadrant form is one row taller (4 vs 3) and reads at a glance.

use crate::domain::{Game, League, Status};
use crate::text::truncate;
use crate::theme;
use crate::tiles;
use crate::tiles::quad_digits;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use time::OffsetDateTime;

/// Everything the hero draws that isn't in the `Game`. The caller (the board
/// view) decides all of it; the hero block itself is a pure function of
/// `(area, game, plan, theme)`.
#[derive(Clone, Debug)]
pub struct HeroPlan {
    /// 8-row `PixelSize::Full` digits are wanted (the ≥100-col bracket).
    /// Still only a request: the band has to have the rows for them.
    pub digits_full: bool,
    /// State chip from `rank::watchability` — "RED ZONE", "2-MIN", … The
    /// only filled-background element anywhere in the hero (spec §1).
    pub chip: Option<&'static str>,
    /// The frame's clock, for pre-game start times. Nothing here reads a
    /// wall clock, so a given tick is the same pixels every run.
    pub now: OffsetDateTime,
    pub pinned: bool,
    pub favorite: bool,
    /// Draw the flanking marks at all. The board turns this off under 100
    /// cols: the flanks are the first casualty, never the digits.
    pub show_logos: bool,
    /// The hero is a selectable row like any tier (spec §1 selection). A `▸`
    /// mark on the outside edge of each nameplate — same glyph the tier rows
    /// use in their gutter — is the only thing selection changes; the digits
    /// keep team color regardless (task-9 review carry-forward #1).
    pub selected: bool,
}

/// Minimum flank width, in cells, that earns a 16-wide hero mark: the art
/// plus one column of air on each side. The reference frame leaves 29-column
/// outer margins (docs/research/v3-identity/logo-study/NOTES.md — "16×10 …
/// the largest size that fits the empty 29-col outer margins without moving
/// a digit"), so 18 is the floor at which a mark still gets its margin
/// rather than the width at which it merely fits.
const FLANK_MIN_COLS: u16 = 18;

/// Committed hero art is 16 cells wide; a narrower flank is not a flank.
const MARK_COLS: u16 = 16;

/// The mid-form floor: one quadrant digit is [`quad_digits::QUAD_ROWS`] rows
/// (sitting-1 pick 1A), so a band shorter than that has already fallen to the
/// text form. This is what the digits are charged when the bracket didn't ask
/// for Full — never a cap on a bracket that did (ruling R29).
const DIGIT_FLOOR_ROWS: u16 = quad_digits::QUAD_ROWS;

/// Rows the score costs at each rung of the ladder. The one place the two
/// numbers are written down: [`row_plan`] reserves them and the takeover
/// ([`crate::board::cut`]) sizes its own score band off the same answer, so a
/// cut can never hand `score_block` a band one row short of the form it
/// planned for.
pub fn digit_rows(full: bool) -> u16 {
    if full {
        tiles::glyph_cell(true).1
    } else {
        DIGIT_FLOOR_ROWS
    }
}

/// Which size the score digits ended up at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScoreForm {
    /// 8×8 `PixelSize::Full` glyphs.
    Full,
    /// 3×4 quadrant-block glyphs (sitting-1 pick 1A).
    Quad,
    /// `24 - 21` on one bold row — never a blank score.
    Text,
}

/// Where the two scores land inside a band. Computed before anything else in
/// the hero so a logo can only ever use what the digits didn't want.
#[derive(Clone, Copy, Debug)]
struct ScoreSpots {
    away: Rect,
    home: Rect,
    form: ScoreForm,
}

/// Away digits right-aligned in the left third, home left-aligned in the
/// right third, biggest form that fits both sides and the band's height.
fn score_spots(area: Rect, game: &Game, full: bool) -> ScoreSpots {
    let away = game.away_score.to_string();
    let home = game.home_score.to_string();
    let third = area.width / 3;
    for form in [ScoreForm::Full, ScoreForm::Quad] {
        if form == ScoreForm::Full && !full {
            continue;
        }
        // The two forms measure differently: `PixelSize::Full` is a flat
        // 8 cells per glyph, the quad form is 3 per digit plus a 1-cell gap
        // between them (`quad_size`). Asking each form for its own size is
        // what keeps the ladder honest when a rung changes cell grid.
        let (aw, hw, gh) = if form == ScoreForm::Full {
            let (gw, gh) = tiles::glyph_cell(true);
            (away.len() as u16 * gw, home.len() as u16 * gw, gh)
        } else {
            let (aw, gh) = quad_digits::quad_size(u32::from(game.away_score));
            let (hw, _) = quad_digits::quad_size(u32::from(game.home_score));
            (aw, hw, gh)
        };
        if aw > third || hw > third || gh > area.height || third == 0 {
            continue;
        }
        let y = area.y + (area.height - gh) / 2;
        return ScoreSpots {
            away: Rect { x: area.x + third - aw, y, width: aw, height: gh },
            home: Rect { x: area.right() - third, y, width: hw, height: gh },
            form,
        };
    }
    // Text form: one centered `24 - 21`, the two numbers still addressable
    // so the flank math below has real rects to subtract.
    let text_w = (away.len() + home.len() + 3) as u16;
    let x0 = area.x + area.width.saturating_sub(text_w) / 2;
    let y = area.y + area.height.saturating_sub(1) / 2;
    ScoreSpots {
        away: Rect { x: x0, y, width: away.len() as u16, height: 1 },
        home: Rect { x: x0 + text_w - home.len() as u16, y, width: home.len() as u16, height: 1 },
        form: ScoreForm::Text,
    }
}

/// Where the away and home digits actually land inside `area`, for the same
/// `full` the caller will hand [`score_block`]. Public so the takeover can
/// hang a team label under each column without re-deriving — or disagreeing
/// with — the digit math: the digits are placed first, and the labels take
/// what is left (spec §1's logos-never-move-a-digit discipline).
pub fn score_columns(area: Rect, game: &Game, full: bool) -> (Rect, Rect) {
    let spots = score_spots(area, game, full);
    (spots.away, spots.home)
}

/// The score digits alone — mirror pair, team colors through
/// [`theme::hero_pair`]. The cut overlay and `:tv` call this same function:
/// one formatter for the score, everywhere, always (spec §1 hard rule).
pub fn score_block(frame: &mut Frame, area: Rect, game: &Game, full: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let (away_color, home_color, _) = theme::hero_pair(&th, game.away.color, game.home.color);
    let spots = score_spots(area, game, full);
    if spots.form == ScoreForm::Text {
        let bold = Modifier::BOLD;
        let line = Line::from(vec![
            Span::styled(game.away_score.to_string(), Style::default().fg(away_color).add_modifier(bold)),
            Span::styled(" - ", Style::default().fg(th.roles().dim)),
            Span::styled(game.home_score.to_string(), Style::default().fg(home_color).add_modifier(bold)),
        ]);
        let row = Rect { x: area.x, y: spots.away.y, width: area.width, height: 1 };
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), row);
        return;
    }
    // `score_spots` already proved the fit, and both renderers re-check it
    // for callers who probe instead of measuring. Two answers to one question
    // is the shape that drifts, so a disagreement is loud in debug builds.
    let (drew_away, drew_home) = if spots.form == ScoreForm::Full {
        (
            tiles::digit_glyphs(frame, spots.away, game.away_score, away_color),
            tiles::digit_glyphs(frame, spots.home, game.home_score, home_color),
        )
    } else {
        (
            quad_digits::quad_digits(frame, spots.away, u32::from(game.away_score), away_color),
            quad_digits::quad_digits(frame, spots.home, u32::from(game.home_score), home_color),
        )
    };
    debug_assert!(
        drew_away && drew_home,
        "score_spots handed {:?} rects the glyph renderer rejected: away {:?}, home {:?}",
        spots.form,
        spots.away,
        spots.home
    );
}

/// The fragment line under the digits: football's `2ND & GOAL · BALL ON 4 ·
/// KC BALL`. `None` everywhere else — basketball and hockey say everything
/// in the clock line and the chip, and MLB's diamond meter row *is* its
/// fragment line (never both; spec §1).
pub fn fragment_line(game: &Game) -> Option<Line<'static>> {
    if !matches!(game.league, League::Nfl | League::Cfb) {
        return None;
    }
    let sit = game.situation.as_ref()?;
    let th = theme::current();
    let r = th.roles();
    let ink = Style::default().fg(r.ink).add_modifier(Modifier::BOLD);
    let sep = Style::default().fg(r.dim);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut push = |span: Span<'static>| {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", sep));
        }
        spans.push(span);
    };
    if !sit.down_distance.is_empty() {
        push(Span::styled(sit.down_distance.to_uppercase(), ink));
    }
    if let Some(on) = sit.ball_on.as_deref().filter(|s| !s.is_empty()) {
        push(Span::styled(format!("BALL ON {}", on.to_uppercase()), Style::default().fg(r.ink)));
    }
    if let Some(poss) = sit.possession.as_deref().filter(|s| !s.is_empty()) {
        // The team with the ball wears its own color — the hero is where
        // team color is allowed, and this is the one word that names a team.
        let (away_color, home_color, _) = theme::hero_pair(&th, game.away.color, game.home.color);
        let color = if poss.eq_ignore_ascii_case(&game.home.abbr) { home_color } else { away_color };
        push(Span::styled(
            format!("{} BALL", poss.to_uppercase()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    if spans.is_empty() {
        None
    } else {
        Some(Line::from(spans))
    }
}

/// `Q4 1:52` / `FINAL` / `8:20 PM` — the center column's first line.
fn clock_text(game: &Game, now: OffsetDateTime) -> String {
    match game.status {
        Status::Live => format!("{} {}", game.period, game.clock).trim().to_string(),
        Status::Final => "FINAL".to_string(),
        Status::Pre => game.start.map(|t| crate::text::fmt_start(t, now)).unwrap_or_default(),
    }
}

/// One nameplate: `⚑ ★ KC 2-0` away, mirrored `74-63 BOS ★ ⚑` home. The
/// record is dim because a bright one beside a giant digit was read as a
/// second score (spec §6 decision).
fn nameplate(game: &Game, away: bool, plan: &HeroPlan) -> Line<'static> {
    let th = theme::current();
    let r = th.roles();
    let (away_color, home_color, fell) = theme::hero_pair(&th, game.away.color, game.home.color);
    let team = if away { &game.away } else { &game.home };
    let color = if away { away_color } else { home_color };
    let star = Style::default().fg(th.star);
    // The caret is the same glyph a selected tier row puts in its gutter
    // (`rows.rs`); the hero has no gutter, so it sits on the outside edge —
    // spec §1 selection, task-9 review carry-forward #1.
    let caret = Style::default().fg(th.bright).add_modifier(Modifier::BOLD);
    // Why this game is the hero, always on the outside edge so the two
    // nameplates stay mirror images of each other.
    let mut marks: Vec<&'static str> = Vec::new();
    if plan.pinned {
        marks.push("⚑");
    }
    if plan.favorite {
        marks.push("★");
    }
    let abbr = Span::styled(team.abbr.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD));
    let record = (!team.record.is_empty()).then(|| Span::styled(team.record.clone(), Style::default().fg(r.dim)));
    // A home side that lost its color to the lookalike rule says so with a
    // one-cell block in its real (lifted) color — spec §6.
    let block = (!away && fell).then(|| Span::styled("▌", Style::default().fg(th.art_color(team.color))));
    let mut spans: Vec<Span<'static>> = Vec::new();
    if away {
        if plan.selected {
            spans.push(Span::styled("▸ ", caret));
        }
        spans.extend(marks.iter().map(|m| Span::styled(format!("{m} "), star)));
        spans.push(abbr);
        if let Some(rec) = record {
            spans.push(Span::raw(" "));
            spans.push(rec);
        }
    } else {
        if let Some(rec) = record {
            spans.push(rec);
            spans.push(Span::raw(" "));
        }
        spans.extend(block);
        spans.push(abbr);
        spans.extend(marks.iter().rev().map(|m| Span::styled(format!(" {m}"), star)));
        if plan.selected {
            spans.push(Span::styled(" ▸", caret));
        }
    }
    Line::from(spans)
}

/// Draw the hero for `game` into `area` per `plan`.
///
/// Rows, top down: nameplates, the digit band (digits left/right, clock and
/// chip in the center column, marks in the outer margins), the fragment
/// line, the meter row, the last play. Every row below the band is optional
/// and is charged only while the band keeps [`DIGIT_FLOOR_ROWS`] — that is
/// the "hero shrinks last" ladder applied inside the hero itself.
/// How a hero `area` splits: the digit band's row count, then which of
/// fragment / meter / play survived under it. The one place the split is
/// computed — [`draw_hero`] draws it and [`band_rect`] reports it, so a
/// caller asking *where the digits are* can never disagree with the draw.
///
/// Ruling R29: the digits are charged FIRST. The bracket asked for a form
/// (`digits_full`), so the band reserves the rows that form needs and the
/// optional rows below split whatever is left. The Full → quad → text
/// ladder is for brackets too short to hold the form — never something a
/// meter row can take away. (Before this, the flagship 10-row bracket spent
/// three rows on options and rendered a mid form.)
///
/// sitting-1 pick 1A moved the mid rung from 3 rows to 4. The 6-row bracket
/// (60–99 cols) therefore reads 1 nameplate + 4 digits + 1 spare, and the keep
/// order spends that spare on the fragment — the meter and play rows it used
/// to afford are the honest price of a readable score. The bracket table
/// itself (R28/R32) is untouched.
///
/// Keep order, ruling R30: fragment → meter → play. The fragment carries the
/// only down-and-distance on screen; the meter's own label repeats the chip
/// ("RED ZONE" twice at the 6-row bracket), so it yields first of the two,
/// and the play stamp is the last nice-to-have.
fn row_plan(area: Rect, have: [bool; 3], digits_full: bool) -> (u16, [bool; 3]) {
    let under_nameplate = area.height.saturating_sub(1);
    let full_rows = digit_rows(true);
    let band = if digits_full && under_nameplate >= full_rows {
        full_rows
    } else if under_nameplate >= DIGIT_FLOOR_ROWS {
        DIGIT_FLOOR_ROWS
    } else {
        under_nameplate.min(1)
    };
    let mut spare = under_nameplate - band;
    let mut want = have;
    for slot in &mut want {
        if *slot && spare > 0 {
            spare -= 1;
        } else {
            *slot = false;
        }
    }
    // Rows the options declined stay with the band, so a taller-than-bracket
    // hero grows its digits' breathing room rather than stranding rows.
    (under_nameplate - want.iter().filter(|w| **w).count() as u16, want)
}

/// The digit band inside a hero `area` — the rect [`draw_hero`] hands
/// [`score_block`] and [`score_columns`]. Public so a caller that needs to
/// find the score on an already-rendered hero (the v3.3 gate captures) reads
/// the geometry instead of guessing at it.
pub fn band_rect(area: Rect, game: &Game, digits_full: bool) -> Rect {
    let have = [
        fragment_line(game).is_some(),
        tiles::meter_line(game, area.width as usize).is_some(),
        !game.last_plays.is_empty(),
    ];
    let (band_rows, _) = row_plan(area, have, digits_full);
    Rect { y: area.y + 1, height: band_rows, ..area }
}

pub fn draw_hero(frame: &mut Frame, area: Rect, game: &Game, plan: &HeroPlan) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();

    let fragment = fragment_line(game);
    let meter = tiles::meter_line(game, area.width as usize);
    let play = game.last_plays.first().map(|p| p.text.clone());

    let (band_rows, want) = row_plan(
        area,
        [fragment.is_some(), meter.is_some(), play.is_some()],
        plan.digits_full,
    );
    let [show_fragment, show_meter, show_play] = want;

    let name_row = Rect { height: 1, ..area };
    let half = area.width / 2;
    frame.render_widget(
        Paragraph::new(nameplate(game, true, plan)).alignment(Alignment::Left),
        Rect { width: half, ..name_row },
    );
    frame.render_widget(
        Paragraph::new(nameplate(game, false, plan)).alignment(Alignment::Right),
        Rect { x: area.x + half, width: area.width - half, ..name_row },
    );

    let band = Rect { y: area.y + 1, height: band_rows, ..area };
    let spots = score_spots(band, game, plan.digits_full);
    score_block(frame, band, game, plan.digits_full);
    // The glyph forms leave the center column empty by construction (digits
    // live in the outer thirds); the text form lies across the whole band, so
    // its row is the one row the clock and chip may not have.
    let taken = (spots.form == ScoreForm::Text).then_some(spots.away.y);
    draw_center_column(frame, band, game, plan, taken);
    if plan.show_logos && spots.form != ScoreForm::Text {
        draw_flanks(frame, band, game, spots);
    }

    let mut y = band.bottom();
    let row = |y: u16| Rect { x: area.x, y, width: area.width, height: 1 };
    if show_fragment {
        if let Some(line) = fragment {
            frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), row(y));
        }
        y += 1;
    }
    if show_meter {
        if let Some(line) = meter {
            // The bar meters are anchored to the frame like the field they
            // stand for; the diamond is a small cluster and centers under
            // the digits (docs/research/v3-identity/tonight-120x40.png).
            let align = match game.meter {
                Some(crate::domain::Meter::Diamond { .. }) => Alignment::Center,
                _ => Alignment::Left,
            };
            frame.render_widget(Paragraph::new(line).alignment(align), row(y));
        }
        y += 1;
    }
    if show_play {
        if let Some(text) = play {
            let body = truncate(&text, (area.width as usize).saturating_sub(2));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("▸ ", Style::default().fg(r.dim)),
                    Span::styled(body, Style::default().fg(r.ink)),
                ]))
                .alignment(Alignment::Center),
                row(y),
            );
        }
    }
}

/// The center column: clock line, then the state chip — the hero's only
/// filled background. `taken` is a band row the score already owns; the
/// column steps around it, because a clock written over the score is worse
/// than no clock at all.
pub(crate) fn draw_center_column(
    frame: &mut Frame,
    band: Rect,
    game: &Game,
    plan: &HeroPlan,
    taken: Option<u16>,
) {
    let th = theme::current();
    let r = th.roles();
    let third = band.width / 3;
    if third == 0 || band.height == 0 {
        return;
    }
    let center = Rect { x: band.x + third, width: band.width - 2 * third, ..band };
    let free: Vec<u16> = (center.y..center.bottom()).filter(|y| Some(*y) != taken).collect();
    let Some(&first) = free.first() else {
        return;
    };
    // Clock above the middle, chip below it: the mirror keeps the pair off
    // the digits' own center row. With one row to spend the clock wins —
    // the chip is a duplicate of state the meter row is already saying.
    let mid = free.len() / 2;
    let (clock_y, chip_y) = match free.len() {
        1 => (first, first),
        2 => (free[0], free[1]),
        _ => (free[mid - 1], free[mid + 1]),
    };
    let text = clock_text(game, plan.now);
    if !text.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(r.ink).add_modifier(Modifier::BOLD)))
                .alignment(Alignment::Center),
            Rect { y: clock_y, height: 1, ..center },
        );
    }
    if let Some(chip) = plan.chip {
        if chip_y != clock_y {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {chip} "),
                    Style::default().fg(r.ground).bg(r.hot).add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center),
                Rect { y: chip_y, height: 1, ..center },
            );
        }
    }
}

/// The two hero marks, each vertically centered in the margin its own side's
/// digits left over. A team with no committed art leaves an empty margin —
/// its identity is already in the nameplate, and a placeholder there reads
/// as a broken image.
fn draw_flanks(frame: &mut Frame, band: Rect, game: &Game, spots: ScoreSpots) {
    let left = Rect { x: band.x, y: band.y, width: spots.away.x - band.x, height: band.height };
    let right = Rect {
        x: spots.home.right(),
        y: band.y,
        width: band.right() - spots.home.right(),
        height: band.height,
    };
    // Symmetry is the point: one lone mark reads as a rendering bug, so
    // both margins have to earn the art.
    if left.width < FLANK_MIN_COLS || right.width < FLANK_MIN_COLS {
        return;
    }
    // spec v3.3 §5: both or neither. A team with no committed art used to
    // leave its own margin empty while the other side still drew — a lone
    // logo reads as a rendering bug, not as "one team has art and one
    // doesn't". So the gate lives here, before either side is drawn, not
    // inside a per-side loop where it can only skip one of them.
    let (Some(away_mark), Some(home_mark)) =
        (crate::board::logo::hero_mark(&game.away.logo_key), crate::board::logo::hero_mark(&game.home.logo_key))
    else {
        return;
    };
    // Whole mark or none: a clipped mark is a smear, not an identity. Width
    // can't half-fit — bundled marks are a constant 16 cols and the 18-col
    // check above already covers it — but bundled heights range 4–10 rows,
    // so a short-art team beside a tall-art team can fit one flank and not
    // the other at an in-between band height. That fit check has to join
    // the same symmetric gate as presence: both sides fit, or neither
    // draws — never one flank alone because its own mark happened to be
    // shorter.
    let away_fits = away_mark.width <= MARK_COLS.min(left.width) && away_mark.height <= left.height;
    let home_fits = home_mark.width <= MARK_COLS.min(right.width) && home_mark.height <= right.height;
    if !away_fits || !home_fits {
        return;
    }
    crate::board::logo::draw_hero_mark(frame, left, away_mark);
    crate::board::logo::draw_hero_mark(frame, right, home_mark);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Meter, Play, Situation, Team};
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn team(abbr: &str, color: [u8; 3], key: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            record: "2-0".into(),
            color,
            alt_color: [255, 255, 255],
            logo_key: key.into(),
            ..Default::default()
        }
    }

    /// KC red vs BUF blue, red zone, 24-21 — the reference frame's game
    /// (docs/research/v3-identity/nfl-sunday-120x40.png).
    fn nfl_game() -> Game {
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: team("KC", [227, 24, 55], "nfl/kc"),
            home: team("BUF", [0, 51, 141], "nfl/buf"),
            away_score: 24,
            home_score: 21,
            status: Status::Live,
            period: "Q4".into(),
            clock: "1:52".into(),
            situation: Some(Situation {
                down_distance: "2nd & Goal".into(),
                possession: Some("KC".into()),
                ball_on: Some("4".into()),
                ..Default::default()
            }),
            last_plays: vec![Play {
                clock: "1:52".into(),
                team: "KC".into(),
                text: "Mahomes scrambles for 9, first and goal".into(),
                ..Default::default()
            }],
            meter: Some(Meter::RedZone { yards_to_goal: 4 }),
            ..Game::default()
        }
    }

    fn mlb_game() -> Game {
        Game {
            id: "2".into(),
            league: League::Mlb,
            away: team("SEA", [12, 44, 86], "mlb/sea"),
            home: team("BOS", [189, 48, 57], "mlb/bos"),
            away_score: 8,
            home_score: 7,
            status: Status::Live,
            period: "BOT 9TH".into(),
            clock: String::new(),
            situation: Some(Situation {
                down_distance: "1 OUT · 1-0".into(),
                balls: Some(1),
                strikes: Some(0),
                outs: Some(1),
                on_base: Some([false, false, true]),
                ..Default::default()
            }),
            last_plays: vec![Play {
                text: "Rodriguez singles to right, Crawford to third".into(),
                ..Default::default()
            }],
            meter: Some(Meter::Diamond { occupied: [false, false, true] }),
            ..Game::default()
        }
    }

    fn plan() -> HeroPlan {
        HeroPlan {
            digits_full: true,
            chip: Some("RED ZONE"),
            now: OffsetDateTime::UNIX_EPOCH,
            pinned: false,
            favorite: false,
            show_logos: true,
            selected: false,
        }
    }

    fn render(w: u16, h: u16, game: &Game, plan: &HeroPlan) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_hero(f, f.area(), game, plan)).unwrap();
        term
    }

    fn text_of(buf: &Buffer) -> String {
        let area = *buf.area();
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every cell in `rect` whose foreground is `color`.
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

    /// The digit band of a `h`-row hero: everything under the nameplate and
    /// above the three text rows at the bottom.
    fn band_of(w: u16, h: u16) -> Rect {
        Rect { x: 0, y: 1, width: w, height: h - 4 }
    }

    #[test]
    fn hero_digits_take_team_colors_and_lookalikes_separate() {
        let th = theme::current();
        let (w, h) = (120u16, 12u16);
        let game = nfl_game();
        let (away_color, home_color, fell) = theme::hero_pair(&th, game.away.color, game.home.color);
        assert!(!fell, "KC red and BUF blue are not lookalikes");
        let term = render(w, h, &game, &plan());
        let buf = term.backend().buffer();
        let band = band_of(w, h);
        let third = w / 3;
        let left = Rect { x: 0, width: third, ..band };
        let right = Rect { x: w - third, width: third, ..band };
        assert!(
            cells_with_fg(buf, left, away_color) >= 8,
            "away digits must paint the left third in KC's lifted red ({away_color:?})\n{}",
            text_of(buf)
        );
        assert!(
            cells_with_fg(buf, right, home_color) >= 8,
            "home digits must paint the right third in BUF's lifted blue ({home_color:?})\n{}",
            text_of(buf)
        );
        assert_eq!(cells_with_fg(buf, right, away_color), 0, "away color must not leak into the home third");

        // Two navies: the home side falls back to the amber `digits` role,
        // and its own color appears nowhere in the home third.
        let mut look = nfl_game();
        look.away = team("SEA", [12, 44, 86], "nfl/sea");
        look.home = team("BOS", [19, 41, 75], "nfl/bos");
        let (away2, home2, fell2) = theme::hero_pair(&th, look.away.color, look.home.color);
        assert!(fell2, "two navies must separate");
        assert_eq!(home2, th.roles().digits);
        let term = render(w, h, &look, &plan());
        let buf = term.backend().buffer();
        assert!(cells_with_fg(buf, right, home2) >= 8, "home digits fall back to amber\n{}", text_of(buf));
        assert_eq!(cells_with_fg(buf, right, away2), 0, "the two navies must not both draw navy digits");
    }

    #[test]
    fn the_chip_is_the_only_filled_badge_and_sits_center() {
        let (w, h) = (120u16, 12u16);
        let r = theme::current().roles();
        let mut p = plan();
        p.show_logos = false; // marks carry their own backgrounds
        let term = render(w, h, &nfl_game(), &p);
        let buf = term.backend().buffer();
        let mut filled: Vec<(u16, u16)> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let bg = buf[(x, y)].bg;
                if bg != Color::Reset {
                    assert_eq!(bg, r.hot, "the only filled background in the hero is the chip, at ({x},{y})");
                    filled.push((x, y));
                }
            }
        }
        assert!(!filled.is_empty(), "RED ZONE chip must render\n{}", text_of(buf));
        let rows: Vec<u16> = {
            let mut ys: Vec<u16> = filled.iter().map(|(_, y)| *y).collect();
            ys.dedup();
            ys
        };
        assert_eq!(rows.len(), 1, "the chip is one row");
        let (min_x, max_x) = (filled[0].0, filled[filled.len() - 1].0);
        let third = w / 3;
        assert!(min_x >= third && max_x < w - third, "the chip sits in the center column ({min_x}..={max_x})");
        let chip: String = (min_x..=max_x).map(|x| buf[(x, rows[0])].symbol()).collect();
        assert_eq!(chip, " RED ZONE ");
    }

    #[test]
    fn mlb_hero_has_no_duplicate_fragment_line() {
        let game = mlb_game();
        assert!(fragment_line(&game).is_none(), "MLB's meter row IS its fragment line");
        let term = render(120, 12, &game, &{
            let mut p = plan();
            p.chip = Some("TYING RUN 3RD"); // spec v3.3 §7
            p
        });
        let text = text_of(term.backend().buffer());
        assert_eq!(text.matches("BASES").count(), 1, "one diamond row, not two\n{text}");
        assert_eq!(text.matches("OUTS").count(), 1, "one outs cluster\n{text}");
        assert_eq!(text.matches("1 OUT · 1-0").count(), 0, "the count headline must not repeat as a fragment\n{text}");
    }

    #[test]
    fn logos_flank_when_art_exists_and_leave_clean_margin_when_not() {
        let (w, h) = (120u16, 12u16);
        let band = band_of(w, h);
        // The flank is the margin the digits didn't take: left third minus
        // the right-aligned away digits.
        let spots = score_spots(band, &nfl_game(), true);
        let left = Rect { x: 0, y: band.y, width: spots.away.x, height: band.height };
        assert!(left.width >= FLANK_MIN_COLS, "the reference frame leaves a real margin: {}", left.width);

        let term = render(w, h, &nfl_game(), &plan());
        let painted = {
            let buf = term.backend().buffer();
            let mut n = 0;
            for y in left.y..left.bottom() {
                for x in left.x..left.right() {
                    let c = &buf[(x, y)];
                    if c.symbol() != " " || c.bg != Color::Reset {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(painted >= 20, "nfl/kc art must fill the left flank, painted {painted} cells");

        // A team with no committed art: the margin stays ground, and no
        // placeholder is drawn in its place.
        let mut unknown = nfl_game();
        unknown.away.logo_key = "nfl/zzz".into();
        unknown.home.logo_key = "nfl/zzz".into();
        let term = render(w, h, &unknown, &plan());
        let buf = term.backend().buffer();
        for y in left.y..left.bottom() {
            for x in left.x..left.right() {
                let c = &buf[(x, y)];
                assert_eq!(c.symbol(), " ", "missing art leaves the margin empty at ({x},{y})");
                assert_eq!(c.bg, Color::Reset, "no placeholder background at ({x},{y})");
            }
        }
    }

    #[test]
    fn dropping_the_logos_never_moves_or_shrinks_a_digit() {
        // 120x12: the one geometry where the marks actually draw (10-row art
        // in a 8-row band is rejected, so a smaller frame would compare two
        // identical logo-less renders and prove nothing).
        let (w, h) = (120u16, 12u16);
        let game = nfl_game();
        let band = band_of(w, h);
        let spots = score_spots(band, &game, true);

        let mut with = plan();
        with.show_logos = true;
        let mut without = with.clone();
        without.show_logos = false;

        let lit = render(w, h, &game, &with);
        let dark = render(w, h, &game, &without);

        // The art is on screen in the lit render — without this the
        // comparison below can pass for a `draw_flanks` that reflows digits.
        let flank = Rect { x: 0, y: band.y, width: spots.away.x, height: band.height };
        let painted = |t: &Terminal<TestBackend>| {
            let buf = t.backend().buffer();
            let mut n = 0;
            for y in flank.y..flank.bottom() {
                for x in flank.x..flank.right() {
                    if buf[(x, y)].symbol() != " " || buf[(x, y)].bg != Color::Reset {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(painted(&lit) >= 20, "the KC mark must be drawn for this test to mean anything");
        assert_eq!(painted(&dark), 0, "show_logos = false leaves the flank untouched ground");

        // Cell for cell, both digit rects are the same pixels either way.
        let (a, b) = (lit.backend().buffer(), dark.backend().buffer());
        for rect in [spots.away, spots.home] {
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    assert_eq!(a[(x, y)].symbol(), b[(x, y)].symbol(), "digit cell ({x},{y}) moved with the logos on");
                    assert_eq!(a[(x, y)].fg, b[(x, y)].fg, "digit color at ({x},{y}) changed with the logos on");
                }
            }
        }
    }

    #[test]
    fn every_layout_bracket_renders_the_digit_form_it_asked_for() {
        // The seam Task 5 owns the other half of: `layout::plan` picks
        // (hero_rows, hero_digits_full) and the hero must honour it. Ruling
        // R29 — digits are charged before any optional row, so the flagship
        // 10-row bracket really does render 8-row LEDs.
        let th = theme::current();
        let game = nfl_game();
        let (away_color, ..) = theme::hero_pair(&th, game.away.color, game.home.color);
        let digit_rows = |t: &Terminal<TestBackend>, w: u16, h: u16| -> Vec<u16> {
            let buf = t.backend().buffer();
            (0..h)
                .filter(|y| (0..w / 3).filter(|x| buf[(*x, *y)].fg == away_color).count() >= 4)
                .collect()
        };
        // (terminal size, rows of away-colored digit cells the bracket owes)
        //
        // sitting-1 pick 1A: the mid rung is the 4-row quad form, so the
        // 6-row bracket owes 4 digit rows where it used to owe 3 sextant
        // ones. The bracket table itself did not move (R28/R32).
        for (w, h, want) in [(120u16, 40u16, 8usize), (100, 32, 8), (80, 24, 4), (60, 20, 4)] {
            let tier = crate::board::layout::plan(w, h, 8, 2, 4, 0);
            let mut p = plan();
            p.digits_full = tier.hero_digits_full;
            let term = render(w, tier.hero_rows, &game, &p);
            let rows = digit_rows(&term, w, tier.hero_rows);
            assert_eq!(
                rows.len(),
                want,
                "{w}x{h} → hero_rows {} digits_full {}: wanted {want} digit rows, got {rows:?}\n{}",
                tier.hero_rows,
                tier.hero_digits_full,
                text_of(term.backend().buffer())
            );
            // Contiguous, and under the nameplate — not scattered by a stray
            // team-colored span somewhere else in the block.
            assert_eq!(rows[0], 1, "the digit band starts right under the nameplate");
            assert_eq!(*rows.last().unwrap() as usize, rows.len(), "the digit band is contiguous: {rows:?}");
            // R30 keep order: the fragment is the row a football hero keeps.
            let text = text_of(term.backend().buffer());
            assert!(text.contains("2ND & GOAL"), "the fragment line survives at {w}x{h}\n{text}");
        }
        // The compact bracket has no room for a glyph at all and says so in text.
        let tier = crate::board::layout::plan(55, 38, 3, 1, 2, 0);
        assert_eq!((tier.hero_rows, tier.hero_digits_full), (2, false));
        let term = render(55, tier.hero_rows, &game, &plan());
        assert!(text_of(term.backend().buffer()).contains("24 - 21"));
    }

    /// `band_rect` is the public answer to "where are the digits?", and the
    /// v3.3 gate captures overpaint exactly that rect. If it and `draw_hero`
    /// ever disagree, a capture silently lies about what it is comparing.
    #[test]
    fn band_rect_is_the_band_draw_hero_actually_uses() {
        let th = theme::current();
        for game in [nfl_game(), mlb_game()] {
            let (away_color, ..) = theme::hero_pair(&th, game.away.color, game.home.color);
            // Every bracket that draws a glyph form; the text arm has no
            // band to speak of (it lies across the whole width by design).
            //
            // sitting-1 pick 1A: a 4-row hero is 1 nameplate + 3 rows, under
            // the quad form's floor, so it now takes the text arm — the case
            // moved to the assertion below the loop. `layout::plan` never
            // asks for one (its brackets are 0/2/6/12 rows), so nothing on
            // the product path lost a glyph here.
            for (w, h) in [(120u16, 12u16), (100, 10), (80, 6), (80, 5), (60, 6)] {
                let mut p = plan();
                p.digits_full = h >= 10;
                let band = band_rect(Rect { x: 0, y: 0, width: w, height: h }, &game, p.digits_full);
                let term = render(w, h, &game, &p);
                let buf = term.backend().buffer();
                let inked: Vec<u16> = (0..h)
                    // 3+ away-colored cells in the INNER half of the left
                    // third is a digit row. Three, not four: the thinnest row
                    // the quad table draws for a one-digit score is `8`'s
                    // `█▀█` — 3 cells (MLB's away 8 here). At three the
                    // nameplate's own `SEA` would qualify, so the window
                    // starts at `w/6`: the digits are right-aligned against
                    // the third's edge and never reach back that far, and the
                    // abbr never reaches forward that far.
                    .filter(|y| (w / 6..w / 3).filter(|x| buf[(*x, *y)].fg == away_color).count() >= 3)
                    .collect();
                assert!(!inked.is_empty(), "no away digits at {w}x{h}\n{}", text_of(buf));
                assert!(
                    inked.iter().all(|y| (band.y..band.bottom()).contains(y)),
                    "{w}x{h}: digits on rows {inked:?}, band_rect says {}..{}\n{}",
                    band.y,
                    band.bottom(),
                    text_of(buf)
                );
                assert!(band.y == 1 && band.bottom() <= h, "{w}x{h}: band {band:?} escapes the hero");
            }
        }
        // Under the quad floor (sitting-1 pick 1A): a 4-row hero has 3 rows
        // under its nameplate, one short of the form, so the ladder takes its
        // text arm — a score, never a blank — and `band_rect` still reports a
        // band inside the hero for a caller to find it in.
        let game = nfl_game();
        let band = band_rect(Rect { x: 0, y: 0, width: 60, height: 4 }, &game, false);
        assert!(band.y == 1 && band.bottom() <= 4, "short band {band:?} escapes the hero");
        let term = render(60, 4, &game, &HeroPlan { digits_full: false, ..plan() });
        let text = text_of(term.backend().buffer());
        assert!(text.contains("24 - 21"), "a hero under the quad floor still says its score\n{text}");
    }

    #[test]
    fn the_nameplates_are_mirror_images_of_each_other() {
        let (w, h) = (120u16, 12u16);
        let th = theme::current();
        let game = nfl_game();
        let (away_color, home_color, _) = theme::hero_pair(&th, game.away.color, game.home.color);
        let mut p = plan();
        p.pinned = true;
        p.favorite = true;
        let term = render(w, h, &game, &p);
        let buf = term.backend().buffer();
        // Split by column, not by byte: the pin and star are multi-byte.
        let cells: Vec<&str> = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
        let row: String = cells.concat();
        let left: String = cells[..(w / 2) as usize].concat();
        let right: &String = &cells[(w / 2) as usize..].concat();
        // Glyphs, abbr, record outward-in on the away side; the exact reverse
        // on the home side — that mirror is the hero's whole shape.
        assert_eq!(left.trim_end(), "⚑ ★ KC 2-0", "away nameplate is left-aligned, glyphs outermost");
        assert_eq!(right.trim_start(), "2-0 BUF ★ ⚑", "home nameplate mirrors it, right-aligned");
        // The abbr is the identity floor: it wears the team's hero color.
        // (Column, not byte offset — the glyphs ahead of it are multi-byte.)
        let col = |hay: &str, needle: &str| hay[..hay.find(needle).unwrap()].chars().count() as u16;
        let ax = col(&row, "KC");
        assert_eq!(buf[(ax, 0)].fg, away_color, "away abbr carries KC's hero color");
        let hx = (w / 2) + col(right, "BUF");
        assert_eq!(buf[(hx, 0)].fg, home_color, "home abbr carries BUF's hero color");
        // The record beside a giant digit reads as a second score unless it
        // is dim (spec §6) — check the away record's first cell.
        let rx = col(&row, "2-0");
        assert_eq!(buf[(rx, 0)].fg, th.roles().dim, "the record stays dim");
    }

    #[test]
    fn selected_hero_carries_a_bright_caret_on_both_outside_edges() {
        // Task-9 review carry-forward #1: the hero is a selectable row like
        // any tier, and had no way to show it. A `▸` lands on the outer edge
        // of each nameplate — mirrored, like every other mark there — in
        // `th.bright`, the same ink `rows.rs` uses for a selected row.
        let (w, h) = (120u16, 12u16);
        let th = theme::current();
        let mut p = plan();
        p.selected = true;
        let term = render(w, h, &nfl_game(), &p);
        let buf = term.backend().buffer();
        let cells: Vec<&str> = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
        let left: String = cells[..(w / 2) as usize].concat();
        let right: String = cells[(w / 2) as usize..].concat();
        assert!(left.trim_start().starts_with('▸'), "away caret on the outside edge: {left:?}");
        assert!(right.trim_end().ends_with('▸'), "home caret on the outside edge: {right:?}");
        let ax = left.find('▸').unwrap();
        assert_eq!(buf[(ax as u16, 0)].fg, th.bright, "the caret is bright");
        let hx = (w / 2) as usize + right.rfind('▸').unwrap();
        assert_eq!(buf[(hx as u16, 0)].fg, th.bright, "the home caret is bright too");

        // Unselected: no caret anywhere on the nameplate row.
        let mut unselected = plan();
        unselected.selected = false;
        let term = render(w, h, &nfl_game(), &unselected);
        let buf = term.backend().buffer();
        let row: String = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(!row.contains('▸'), "no caret when the hero is not selected: {row:?}");
    }

    #[test]
    fn a_two_row_hero_keeps_the_nameplates_and_the_score() {
        // The compact bracket: two rows is the whole hero. Nameplates on top,
        // the text score under them, the chip dropped rather than written
        // over the score.
        let (w, h) = (55u16, 2u16);
        let term = render(w, h, &nfl_game(), &plan());
        let buf = term.backend().buffer();
        let rows: Vec<String> =
            (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect()).collect();
        assert!(rows[0].starts_with("KC 2-0"), "away nameplate survives\n{}", rows.join("\n"));
        assert!(rows[0].trim_end().ends_with("2-0 BUF"), "home nameplate survives\n{}", rows.join("\n"));
        assert_eq!(rows[1].trim(), "24 - 21", "the score is never blank\n{}", rows.join("\n"));
        for y in 0..h {
            for x in 0..w {
                assert_eq!(buf[(x, y)].bg, Color::Reset, "no chip crowds a two-row hero at ({x},{y})");
            }
        }
    }


    #[test]
    fn a_band_with_no_room_for_glyphs_still_prints_the_score() {
        // The v3.1 clamping discipline: every slot is derived from the area,
        // so no size may panic — and none may blank the score either.
        for (w, h) in [(1u16, 1u16), (4, 3), (40, 2), (20, 12), (60, 4), (119, 39)] {
            let term = render(w, h, &nfl_game(), &plan());
            let text = text_of(term.backend().buffer());
            if w >= 12 && h >= 2 {
                assert!(
                    text.contains("24 - 21") || text.contains("████") || text.contains('🬂'),
                    "{w}x{h} must still show a score in some form\n{text}"
                );
            }
        }
    }
}
