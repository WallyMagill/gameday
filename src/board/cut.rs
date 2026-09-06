//! The cut (spec §3): when a score lands, the app says so.
//!
//! Two sizes, one decision. A game you care about — pinned, favorited, or the
//! only thing on screen in `:tv` — takes the whole frame for three seconds:
//! the scoring word in block letters, the score under it, one line of detail.
//! Every other score gets a quiet two-row band above the list for a second
//! and a half, and the board never moves.
//!
//! Two rules this module is built around:
//!
//! * **One score formatter.** The takeover draws its digits with
//!   [`hero::score_block`] — the same function the hero calls. There is no
//!   second digit renderer anywhere in the app, so a cut and the board behind
//!   it can never disagree about what 24-21 looks like (spec §1 hard rule).
//! * **[`CutState`] is pure.** It knows a game id, a play, a size and a
//!   deadline. Whether a cut is *allowed* — startup history, an open prompt,
//!   the help overlay — is the caller's judgment, passed in as `full` and
//!   enforced by `App` before `fire` is ever called. That keeps the firing
//!   rules testable without an `App`.

use crate::app::LIVE_TICKS_PER_SEC;
use crate::board::hero;
use crate::domain::{Game, Play};
use crate::text::truncate;
use crate::theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

/// Takeover lifetime: 3 s (spec §3). The render loop runs at
/// [`LIVE_TICKS_PER_SEC`] = 10 ticks/s while anything is live — and a cut
/// only ever fires off a live score delta, and pins the loop to that cadence
/// while it is up (`App::any_live`) — so 3 s is 30 ticks.
pub const CUT_TICKS: u64 = 3 * LIVE_TICKS_PER_SEC;
/// Band lifetime: 1.5 s (spec §3), same cadence, so 15 ticks.
pub const BAND_TICKS: u64 = 3 * LIVE_TICKS_PER_SEC / 2;

/// The cut's mark: the same `▲` the spec's band and takeover chip both wear
/// (spec §3). One glyph, one meaning — "a score just happened".
const MARK: &str = "▲";

/// Rows the quiet band occupies above the list: the headline and one detail
/// line (spec §3).
pub const BAND_ROWS: u16 = 2;

/// One firing. `full` = the takeover; false = the 2-row band.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub game_id: String,
    pub play: Play,
    pub full: bool,
    pub until_tick: u64,
}

impl Cut {
    /// Whole seconds left before this cut clears, rounded up — what the
    /// takeover's strip and the band's affordance count down (spec §3).
    ///
    /// Receipt: [`LIVE_TICKS_PER_SEC`] is 10, so a band's 15 remaining ticks
    /// is 1.5 s and reads `2s`. Ceiling, not truncation: a cut that is still
    /// on screen must never say `0s`, and rounding down would spend the last
    /// half second lying. Zero is reserved for expired.
    pub fn remaining_secs(&self, tick: u64) -> u64 {
        self.until_tick
            .saturating_sub(tick)
            .div_ceil(LIVE_TICKS_PER_SEC)
    }
}

/// The single active cut. At most one — two scores inside three seconds are
/// one moment, not two overlays.
#[derive(Default)]
pub struct CutState {
    active: Option<Cut>,
}

impl CutState {
    /// Called with every newly captured scoring play. `full` when the team is
    /// pinned/favorited or TV is on; the caller (`App`) refuses to call at all
    /// during the first 30 s of a session and while a prompt or help is open
    /// (spec §3).
    pub fn fire(&mut self, game_id: &str, play: &Play, full: bool, tick: u64) {
        // A takeover is never downgraded: while one is up, a second score
        // anywhere is already part of the same moment. A band, on the other
        // hand, yields to a takeover the instant one is earned.
        if let Some(active) = self.active(tick) {
            if !(full && !active.full) {
                return;
            }
        }
        self.active = Some(Cut {
            game_id: game_id.to_string(),
            play: play.clone(),
            full,
            until_tick: tick + if full { CUT_TICKS } else { BAND_TICKS },
        });
    }

    /// The cut on screen at `tick`, if any. Expiry is read here rather than
    /// swept on a timer, so a dump at a fixed tick renders the same frame
    /// every run.
    pub fn active(&self, tick: u64) -> Option<&Cut> {
        self.active.as_ref().filter(|c| tick < c.until_tick)
    }
}

/// The word this play earns: the league's scoring word, sharpened by the
/// play's structural kind where the kind says something more specific
/// (football's field goals and safeties — see
/// [`theme::scoring_word_for_play`]).
fn word_for(game: &Game, play: &Play) -> &'static str {
    theme::scoring_word_for_play(game.league, play)
}

/// Which size the scoring word ended up at, and the rows it costs.
///
/// Ruling R42: there is no sextant rung. It fired at widths 40–71 — every
/// terminal narrower than `TOUCHDOWN` at block size — and `PixelSize::Sextant`
/// draws from U+1FB00–1FB3B, which Terminal.app's default font does not cover:
/// the loudest moment the app has rendered as a row of tofu boxes. The ladder
/// is block letters or a plain bold line, and a bold line that says TOUCHDOWN
/// beats a big shape that says nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordForm {
    Full,
    Text,
}

impl WordForm {
    fn rows(self) -> u16 {
        match self {
            WordForm::Full => crate::tiles::glyph_cell().1,
            WordForm::Text => 1,
        }
    }

    /// Cells `word` needs at this size.
    fn cols(self, word: &str) -> u16 {
        let glyphs = word.chars().count() as u16;
        match self {
            WordForm::Full => glyphs * crate::tiles::glyph_cell().0,
            WordForm::Text => glyphs,
        }
    }
}

/// Where the takeover's pieces land. Computed before anything is drawn so
/// the score's rect is knowable from outside (the hard-rule test asks for it).
struct Plan {
    /// Rows spent on the chip / detail / strip singles: 3, or 0 when there is
    /// no middle left to bracket.
    brackets: u16,
    word_form: WordForm,
    word_rect: Rect,
    score: Rect,
    score_full: bool,
    /// The row the team labels hang on, when the middle has a spare row after
    /// the word and the score have taken theirs. `None` is a frame too short
    /// for them — a label never costs the digits a row (spec §1).
    labels: Option<Rect>,
}

fn plan(area: Rect, word: &str) -> Plan {
    // Three fixed single rows bracket the middle: chip, detail, strip. Below
    // six rows there is no middle left, and the cut collapses to the word and
    // the score alone.
    let brackets: u16 = if area.height >= 6 { 3 } else { 0 };
    let middle = area.height - brackets;
    // Biggest (word, score) pair that fits the middle. The word gives way
    // before the score does: the digits are what is being announced.
    //
    // Width receipt: at `PixelSize::Full` one glyph is 8 cells wide
    // (`tiles::glyph_cell`), so the longest word we ship — "TOUCHDOWN", 9
    // glyphs (spec v3.3 §7 dropped the "!") — needs 72 columns. Anything
    // narrower goes straight to a plain bold line (ruling R42 deleted the
    // sextant rung that used to sit between them). Never a clipped letter,
    // and never a rung that tofus.
    let (word_form, score_full) = [
        (WordForm::Full, true),
        (WordForm::Full, false),
        (WordForm::Text, true),
        (WordForm::Text, false),
    ]
    .into_iter()
    .find(|(form, score_full)| {
        // The score's rows come from the hero's ladder, not from
        // `glyph_cell`: the mid rung is the 4-row quad form (sitting-1 pick
        // 1A), and a cut that reserved 3 would hand `score_block` a band it
        // has to refuse, collapsing the announcement to `27 - 24`.
        let score_rows = hero::digit_rows(*score_full);
        form.cols(word) <= area.width && form.rows() + score_rows <= middle
    })
    .unwrap_or((WordForm::Text, false));

    let score_rows = hero::digit_rows(score_full).min(middle);
    let stack = word_form.rows() + score_rows;
    let top = area.y + brackets.min(1);
    let word_y = top + middle.saturating_sub(stack) / 2;
    let word_rect = Rect {
        y: word_y,
        height: word_form.rows(),
        ..area
    };
    let score = Rect {
        y: word_rect.bottom(),
        height: score_rows,
        ..area
    };
    // The labels are the last thing planned, and only out of a row the word
    // and the score did not want. `stack < middle` is exactly the condition
    // that leaves `score.bottom()` inside the middle.
    let labels = (stack < middle).then(|| Rect {
        y: score.bottom(),
        height: 1,
        ..area
    });
    Plan {
        brackets,
        word_form,
        word_rect,
        score,
        score_full,
        labels,
    }
}

/// The rect the takeover hands [`hero::score_block`], and the `full` flag it
/// passes with it. Public so a test can prove the cut's digits and the
/// hero's are literally the same cells (spec §1's hard rule).
pub fn score_slot(area: Rect, game: &Game, play: &Play) -> (Rect, bool) {
    let p = plan(area, word_for(game, play));
    (p.score, p.score_full)
}

/// Draw the takeover: the caller keeps the header row; everything from here
/// down is the cut. Chip line, the scoring word in block letters, the score
/// through [`hero::score_block`], one detail line, a dim bottom strip.
pub fn draw_takeover(frame: &mut Frame, area: Rect, game: &Game, cut: &Cut, tick: u64) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let play = &cut.play;
    let th = theme::current();
    let r = th.roles();
    // The board behind is not drawn dimmed or blurred — it is gone. A cut
    // that leaves rows showing through reads as a rendering fault (spec §3).
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(th.bg).fg(th.fg)),
        area,
    );

    let word = word_for(game, play);
    let Plan {
        brackets,
        word_form,
        word_rect,
        score,
        score_full,
        labels,
    } = plan(area, word);

    if brackets > 0 {
        frame.render_widget(
            Paragraph::new(chip_line(play)).alignment(Alignment::Center),
            Rect { height: 1, ..area },
        );
    }
    let hot = Style::default().fg(r.hot).add_modifier(Modifier::BOLD);
    match word_form {
        WordForm::Text => frame.render_widget(
            Paragraph::new(Span::styled(word, hot)).alignment(Alignment::Center),
            word_rect,
        ),
        WordForm::Full => {
            let cols = word_form.cols(word);
            let slot = Rect {
                x: word_rect.x + (word_rect.width - cols) / 2,
                width: cols,
                ..word_rect
            };
            crate::tiles::word_glyphs(frame, slot, word, r.hot);
        }
    }
    // The hard rule (spec §1): the takeover does not know how to draw a
    // score. It asks the hero's own formatter, so the cut and the board
    // behind it are the same digits, cell for cell.
    hero::score_block(frame, score, game, score_full);
    if let Some(row) = labels {
        draw_labels(frame, row, score, game, score_full);
    }
    if brackets > 0 {
        let bottom = area.bottom();
        frame.render_widget(
            Paragraph::new(detail_line(game, play, area.width)).alignment(Alignment::Center),
            Rect {
                y: bottom - 2,
                height: 1,
                ..area
            },
        );
        let strip = Rect {
            y: bottom - 1,
            height: 1,
            ..area
        };
        let dim = Style::default().fg(r.dim);
        frame.render_widget(
            Paragraph::new(Span::styled(strip_text(game), dim)).alignment(Alignment::Center),
            strip,
        );
        // The timer is the strip's right end. Rendered as its own
        // right-aligned paragraph over the same row: the matchup is centered
        // and short, so the two never meet on any width that fits both.
        frame.render_widget(
            Paragraph::new(Span::styled(timer_text(cut, tick), dim)).alignment(Alignment::Right),
            strip,
        );
    }
}

/// `KC` under the away digits, `BUF` under the home digits, each in the color
/// its own score is wearing. The rects come from [`hero::score_columns`], so
/// a label can only ever sit under the digits it belongs to — and the
/// takeover never asks where the digits *should* be, only where they are.
///
/// Both abbrs or neither: one lonely label reads as a rendering fault.
fn draw_labels(frame: &mut Frame, row: Rect, score: Rect, game: &Game, score_full: bool) {
    let th = theme::current();
    // The pair straight from `hero_pair` — the same call `score_block` makes.
    // v3.2 routed these through `App::team_color`, which knows nothing about
    // the hero's lift or its lookalike rule, and painted the away side
    // neutral whenever the theme's discipline withheld play-row team color.
    let (away_color, home_color, _) = theme::hero_pair(&th, game.away.color, game.home.color);
    let (away_col, home_col) = hero::score_columns(score, game, score_full);
    let bold = Modifier::BOLD;
    // Centered under its column, clamped inside the row.
    let slot = |col: Rect, text: &str| -> Option<Rect> {
        let w = text.chars().count() as u16;
        if w == 0 || w > row.width {
            return None;
        }
        let x = (col.x + col.width / 2)
            .saturating_sub(w / 2)
            .min(row.right() - w)
            .max(row.x);
        Some(Rect { x, width: w, ..row })
    };
    let (away, home) = (game.away.abbr.to_uppercase(), game.home.abbr.to_uppercase());
    let (Some(a), Some(h)) = (slot(away_col, &away), slot(home_col, &home)) else {
        return;
    };
    if a.right() >= h.x {
        return; // no room to name both without them touching
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            away,
            Style::default().fg(away_color).add_modifier(bold),
        )),
        a,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            home,
            Style::default().fg(home_color).add_modifier(bold),
        )),
        h,
    );
}

/// `clears in 3s` — the cut's own countdown, in the lowercase legend voice
/// the footer uses (spec §5).
fn timer_text(cut: &Cut, tick: u64) -> String {
    format!("clears in {}s", cut.remaining_secs(tick))
}

/// Draw the band into the 2 rows above the list (spec §3, ruling R33): both
/// rows on the `hot` ground, `▲ HOME RUN · TEX Seager (32) · ATH 0 TEX 5` on
/// the first, the rest of the play on the second. Ink is the theme's ground
/// throughout — team color on a hot fill is unreadable, and the band's job is
/// to be a bar of the alert color that the eye catches above the list.
pub fn draw_band(frame: &mut Frame, area: Rect, game: &Game, cut: &Cut, tick: u64) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let play = &cut.play;
    let th = theme::current();
    let r = th.roles();
    let on_hot = Style::default().fg(r.ground).bg(r.hot);
    let bold = on_hot.add_modifier(Modifier::BOLD);
    // `Clear` first: since v3.3 the band lands on rows the board RESERVED and
    // has already drawn into (the hoisted section rule), and a `Block` style
    // alone only recolors cells — the dashes and caption underneath survived
    // and read straight through the bar. The band is opaque or it is a tint.
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(on_hot), area);

    let (name, _) = split_surname(&play.text);
    let mut head = format!("{MARK} {}", word_for(game, play));
    if !play.team.is_empty() {
        head.push_str(&format!(" · {}", play.team.to_uppercase()));
        if let Some(name) = name {
            head.push_str(&format!(" {}", name.to_uppercase()));
        }
    }
    head.push_str(&format!(
        " · {} {} {} {}",
        game.away.abbr, game.away_score, game.home.abbr, game.home_score
    ));
    frame.render_widget(
        Paragraph::new(Span::styled(truncate(&head, area.width as usize), bold)),
        Rect { height: 1, ..area },
    );
    if area.height >= 2 {
        // spec v3.3 §3: the second row is the affordance, not the play again.
        // The headline above already carries the word, the team, the scorer
        // and the score; a second helping of the same sentence told the
        // reader nothing, while what enter does and how long the band lasts
        // are the two things they cannot see anywhere else.
        //
        // Still the ground role, not `dim`: `dim` is a ground-relative gray
        // and vanishes on the hot fill (the v3.2 receipt). The affordance is
        // quieted by weight instead — the headline is bold, this row is not.
        let tail = format!("{} jump · {}", jump_key(), timer_text(cut, tick));
        frame.render_widget(
            Paragraph::new(Span::styled(truncate(&tail, area.width as usize), on_hot)),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

/// The key cap the band's affordance names. Read out of the board legend's
/// own `zoom` chord rather than spelled again here: the band adds a target to
/// that gesture, not a second key, and the two can never drift apart.
fn jump_key() -> &'static str {
    crate::keymap::BOARD_LEGEND
        .iter()
        .find(|(_, label)| *label == "zoom")
        .map_or("enter", |(key, _)| key)
}

/// `▲ SCORING PLAY · KC` (spec §3, ruling R33) — the takeover's one filled
/// element, the same ground-on-hot chip the hero's state chip wears. The
/// teams' own identities arrive right below it, in the score's colors.
fn chip_line(play: &Play) -> Line<'static> {
    let th = theme::current();
    let r = th.roles();
    let text = if play.team.is_empty() {
        format!(" {MARK} SCORING PLAY ")
    } else {
        format!(" {MARK} SCORING PLAY · {} ", play.team.to_uppercase())
    };
    Line::from(Span::styled(
        text,
        Style::default()
            .fg(r.ground)
            .bg(r.hot)
            .add_modifier(Modifier::BOLD),
    ))
}

/// `MAHOMES · 12 YD PASS TO KELCE · Q4 1:52` — every part from the
/// `Play` itself. The scorer's name comes out of the play text through
/// [`split_surname`], which knows ESPN's two sentence forms; a text with no
/// name-shaped token is simply printed whole rather than guessed at.
fn detail_line(game: &Game, play: &Play, width: u16) -> Line<'static> {
    let th = theme::current();
    let r = th.roles();
    let bold = Modifier::BOLD;
    let (name, rest) = split_surname(&play.text);
    let clock = format!("{} {}", play.period, play.clock).trim().to_string();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut budget = width as usize;
    if let Some(name) = name {
        let color = crate::app::App::team_color(game, &play.team);
        budget = budget.saturating_sub(name.chars().count() + 3);
        spans.push(Span::styled(
            name.to_uppercase(),
            Style::default().fg(color).add_modifier(bold),
        ));
        spans.push(Span::styled(" · ", Style::default().fg(r.dim)));
    }
    if !clock.is_empty() {
        budget = budget.saturating_sub(clock.chars().count() + 3);
    }
    if !rest.is_empty() {
        // Uppercase like the rest of the cut (spec §3's `4 YD RUSH`): ESPN
        // writes sentence case, and one lowercase clause under block letters
        // reads as a caption from another screen.
        spans.push(Span::styled(
            truncate(&rest.to_uppercase(), budget),
            Style::default().fg(r.ink),
        ));
    }
    if !clock.is_empty() {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", Style::default().fg(r.dim)));
        }
        spans.push(Span::styled(clock, Style::default().fg(r.dim)));
    }
    Line::from(spans)
}

/// The play text's name, and everything else. `None` when no token is
/// name-shaped — nothing is invented.
///
/// Two sentence forms, because ESPN writes two. Football is subject-first
/// ("Mahomes 12 Yd pass"), so the name is the leading token. Baseball is
/// subject-*last* behind a dash ("Strikeout — J. Ortiz"), and no stop list
/// can keep up with its play-type vocabulary (Strikeout, Walk, Single,
/// Double, Groundout, Sacrifice, …) — the live band painted `CHC STRIKEOUT`
/// as the scorer with `— J. ORTIZ` as the play. So the dash form is
/// recognized first, and only when the right side is actually name-shaped.
/// Both the band and the takeover's detail line come through here.
fn split_surname(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.trim();
    for sep in ['—', '–'] {
        if let Some((head, tail)) = trimmed.split_once(sep) {
            let (head, tail) = (head.trim(), tail.trim());
            if !head.is_empty() && is_person(tail) {
                return (Some(tail), head);
            }
        }
    }
    let head = trimmed.split_whitespace().next().unwrap_or_default();
    // A handful of capitalized words that open a play sentence without being
    // anybody's name ("End of quarter", "Safety, snap out of the end zone").
    // A stop list, not a parser: the cost of a miss is one word painted in a
    // team color for three seconds.
    const NOT_A_NAME: [&str; 8] = [
        "End", "Safety", "Timeout", "Penalty", "Blocked", "Missed", "Two", "Extra",
    ];
    let is_name = !NOT_A_NAME.iter().any(|w| w.eq_ignore_ascii_case(head))
        && head.len() > 1
        && head.chars().next().is_some_and(|c| c.is_uppercase())
        && head
            .chars()
            .all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '.');
    if is_name {
        (Some(head), trimmed[head.len()..].trim_start())
    } else {
        (None, trimmed)
    }
}

/// Is this whole fragment a person's name? Used for the right side of the
/// dash form only. Deliberately tight: one to three capitalized tokens of
/// letters, apostrophes, hyphens and initials' periods, ending in a token
/// long enough to be a surname. A right side that is a clause ("scores from
/// second") fails it and the sentence keeps its old, whole-text handling.
fn is_person(s: &str) -> bool {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.is_empty() || tokens.len() > 3 {
        return false;
    }
    tokens.last().is_some_and(|t| t.chars().count() > 1)
        && tokens.iter().all(|t| {
            t.chars().next().is_some_and(|c| c.is_uppercase())
                && t.chars()
                    .all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '.')
        })
}

/// The dim strip under the detail: who is playing, spelled out. The takeover
/// is the one surface with room for the full names.
fn strip_text(game: &Game) -> String {
    let name = |t: &crate::domain::Team| {
        if t.name.is_empty() {
            t.abbr.clone()
        } else {
            t.name.clone()
        }
        .to_uppercase()
    };
    format!("{} AT {}", name(&game.away), name(&game.home))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{League, PlayKind, Status, Team};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    /// Render one closure into a fresh buffer of this size.
    fn drawn(w: u16, h: u16, f: impl FnOnce(&mut Frame)) -> Buffer {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(f).unwrap();
        t.backend().buffer().clone()
    }

    /// The characters of one buffer row, as a string.
    fn row_text(b: &Buffer, y: u16) -> String {
        (0..b.area.width).map(|x| b[(x, y)].symbol()).collect()
    }

    fn a_cut(full: bool, tick: u64, p: &Play) -> Cut {
        Cut {
            game_id: "1".into(),
            play: p.clone(),
            full,
            until_tick: tick + if full { CUT_TICKS } else { BAND_TICKS },
        }
    }

    fn play(text: &str) -> Play {
        Play {
            clock: "1:52".into(),
            period: "Q4".into(),
            team: "KC".into(),
            text: text.into(),
            scoring: true,
            kind: crate::domain::PlayKind::Other,
            score_value: None,
        }
    }

    fn game() -> Game {
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: Team {
                abbr: "KC".into(),
                name: "Chiefs".into(),
                ..Default::default()
            },
            home: Team {
                abbr: "BUF".into(),
                name: "Bills".into(),
                ..Default::default()
            },
            away_score: 24,
            home_score: 21,
            status: Status::Live,
            period: "Q4".into(),
            clock: "1:52".into(),
            ..Game::default()
        }
    }

    #[test]
    fn fire_scoping_and_expiry() {
        let mut cuts = CutState::default();
        assert!(cuts.active(0).is_none(), "nothing fired, nothing on screen");

        // A band: the quiet size, 1.5 s.
        cuts.fire("1", &play("Butker 41 yd field goal"), false, 100);
        let band = cuts.active(100).expect("the band is up");
        assert!(!band.full);
        assert_eq!(band.until_tick, 100 + BAND_TICKS);
        assert_eq!(band.game_id, "1");

        // A band never replaces a band — the first one owns the window.
        cuts.fire("2", &play("Someone else scores"), false, 105);
        assert_eq!(
            cuts.active(105).unwrap().game_id,
            "1",
            "a band does not preempt a band"
        );

        // A takeover does: the size is an upgrade, not a second event.
        cuts.fire("2", &play("Mahomes 12 yd pass"), true, 106);
        let cut = cuts.active(106).expect("the takeover replaced the band");
        assert!(cut.full);
        assert_eq!(cut.game_id, "2");
        assert_eq!(cut.until_tick, 106 + CUT_TICKS);

        // ...and is never downgraded while it is up.
        cuts.fire("3", &play("A quieter score"), false, 110);
        assert_eq!(
            cuts.active(110).unwrap().game_id,
            "2",
            "a takeover is never downgraded"
        );

        // Expiry is read at the tick, not swept.
        assert!(cuts.active(106 + CUT_TICKS - 1).is_some());
        assert!(
            cuts.active(106 + CUT_TICKS).is_none(),
            "the cut ends on its deadline"
        );

        // Once expired, anything may fire again.
        cuts.fire("3", &play("Later score"), false, 200);
        assert_eq!(cuts.active(200).unwrap().game_id, "3");
    }

    /// `play()` with a specific structural kind — the word now reads the
    /// kind, not the sentence.
    fn kinded(text: &str, kind: PlayKind) -> Play {
        Play { kind, ..play(text) }
    }

    #[test]
    fn the_word_follows_the_plays_structural_kind() {
        // spec v3.4 §2: the kind decides, whatever the sentence says.
        assert_eq!(
            word_for(
                &game(),
                &kinded("Mahomes 12 Yd pass to Kelce", PlayKind::Touchdown)
            ),
            "TOUCHDOWN"
        );
        assert_eq!(
            word_for(
                &game(),
                &kinded("Butker 41 Yd Field Goal", PlayKind::FieldGoal)
            ),
            "FIELD GOAL"
        );
        assert_eq!(
            word_for(
                &game(),
                &kinded("Jones sacked in end zone for a Safety", PlayKind::Safety)
            ),
            "SAFETY"
        );
        // Ruling R34, now trivially right: ESPN really writes "Blocked Field
        // Goal returned 62 yards for a TOUCHDOWN", and the v3.3 text-priority
        // dance this used to require (touchdown must beat field goal in the
        // sentence, or the screen lies) is gone — the kind already says
        // Touchdown.
        assert_eq!(
            word_for(
                &game(),
                &kinded(
                    "Blocked Field Goal returned 62 yards for a TOUCHDOWN",
                    PlayKind::Touchdown
                )
            ),
            "TOUCHDOWN"
        );
        assert_eq!(
            word_for(
                &game(),
                &kinded(
                    "Fumble on the Safety, recovered for a Touchdown",
                    PlayKind::Touchdown
                )
            ),
            "TOUCHDOWN"
        );
        // An NFL play the mapper didn't classify: the honest league fallback,
        // not a guess from the sentence.
        assert_eq!(
            word_for(&game(), &play("Mahomes 12 Yd pass to Kelce")),
            "TOUCHDOWN"
        );

        let mut nba = game();
        nba.league = League::Nba;
        assert_eq!(
            word_for(
                &nba,
                &kinded("Jokic makes 3-pt field goal", PlayKind::ThreePointer)
            ),
            "BUCKET"
        ); // spec v3.3 §7
    }

    #[test]
    fn mlb_says_home_run_only_when_the_kind_was_one() {
        let mut mlb = game();
        mlb.league = League::Mlb;
        // The two real texts off the wire on 2026-09-02 that rendered
        // HOME RUN! in block letters (T16 live captures cut-live-1 and the
        // 22:47:42 band): a bases-loaded walk and a run scoring on a
        // strikeout. Neither is a home run, and both carry RunScoringPlay,
        // not HomeRun.
        assert_eq!(
            word_for(&mlb, &kinded("Walk — J. Sanoja", PlayKind::RunScoringPlay)),
            "RUN SCORES"
        );
        assert_eq!(
            word_for(
                &mlb,
                &kinded("Strikeout — J. Ortiz", PlayKind::RunScoringPlay)
            ),
            "RUN SCORES"
        );
        assert_eq!(
            word_for(
                &mlb,
                &kinded("Play Result — J. Marsee", PlayKind::RunScoringPlay)
            ),
            "RUN SCORES"
        );
        assert_eq!(
            word_for(&mlb, &kinded("Home Run — K. Schwarber", PlayKind::HomeRun)),
            "HOME RUN"
        );
        assert_eq!(
            word_for(
                &mlb,
                &kinded("A. Judge homers to left center", PlayKind::HomeRun)
            ),
            "HOME RUN"
        );
        // A wire text the mapper didn't classify: the league's honest
        // generic, not a guess from the sentence's "home run"/"homer".
        assert_eq!(word_for(&mlb, &play("Ruling under review")), "HOME RUN");
    }

    #[test]
    fn the_detail_line_is_built_from_the_play_alone() {
        let line = detail_line(&game(), &play("Mahomes pass to Kelce for 3 yards"), 80);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Uppercase throughout, like spec §3's `P. MAHOMES · 4 YD RUSH · Q4 1:52`.
        assert_eq!(text, "MAHOMES · PASS TO KELCE FOR 3 YARDS · Q4 1:52");

        // A text with no leading name invents none.
        let line = detail_line(&game(), &play("3 yard rush, touchdown"), 80);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "3 YARD RUSH, TOUCHDOWN · Q4 1:52");
    }

    #[test]
    fn a_capitalized_non_name_is_never_painted_as_a_scorer() {
        // The surname is a stop-listed guess, not a parser: these sentences
        // open with a capitalized word that is nobody.
        for text in [
            "Safety, snap out of the end zone",
            "End of quarter",
            "Blocked Field Goal returned 62 yards for a TOUCHDOWN",
        ] {
            assert_eq!(
                split_surname(text).0,
                None,
                "{text:?} has no scorer to name"
            );
            assert_eq!(
                split_surname(text).1,
                text,
                "the whole sentence survives: {text:?}"
            );
        }
        assert_eq!(split_surname("Mahomes 12 Yd pass").0, Some("Mahomes"));
    }

    #[test]
    fn mlbs_dash_form_names_the_player_not_the_play_type() {
        // The real texts off the wire (T16 live captures): the band rendered
        // `▲ HOME RUN! · CHC STRIKEOUT` with `— J. ORTIZ · T9` beneath it —
        // play type painted as a person, leaked em dash painted as the play.
        for (text, name, rest) in [
            ("Strikeout — J. Ortiz", "J. Ortiz", "Strikeout"),
            ("Walk — J. Sanoja", "J. Sanoja", "Walk"),
            ("Play Result — J. Marsee", "J. Marsee", "Play Result"),
            ("Sacrifice Fly — M. Betts", "M. Betts", "Sacrifice Fly"),
            ("Single – W. Contreras", "W. Contreras", "Single"), // en dash too
        ] {
            assert_eq!(split_surname(text), (Some(name), rest), "{text:?}");
        }

        // Football's subject-first form is untouched, dash or no dash.
        assert_eq!(
            split_surname("Mahomes 12 Yd pass to Kelce"),
            (Some("Mahomes"), "12 Yd pass to Kelce")
        );
        // A dash whose right side is a clause, not a name: nothing is
        // rearranged, and the sentence survives whole.
        let clause = "Kelce 3 Yd pass — no flag on the play";
        assert_eq!(split_surname(clause).0, Some("Kelce"));
        assert_eq!(
            split_surname("End of inning — runners left on"),
            (None, "End of inning — runners left on")
        );
    }

    #[test]
    fn the_mlb_band_reads_as_a_leaderboard_line() {
        // End to end on the captured band: word, team, player, then the play
        // type on the second row — no em dash, no play type as a name.
        let mut mlb = game();
        mlb.league = League::Mlb;
        let mut p = play("Strikeout — J. Ortiz");
        p.team = "CHC".into();
        p.period = "T9".into();
        p.clock = String::new();
        p.kind = PlayKind::RunScoringPlay; // spec v3.4 §2: the kind, not the sentence
        let (name, rest) = split_surname(&p.text);
        assert_eq!(name, Some("J. Ortiz"));
        assert_eq!(rest, "Strikeout");
        assert_eq!(word_for(&mlb, &p), "RUN SCORES"); // spec v3.3 §7
        let line = detail_line(&mlb, &p, 80);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "J. ORTIZ · STRIKEOUT · T9");
    }

    /// Contiguous non-space cells on a row, split wherever the color changes:
    /// `(x, text, fg)`. The cut's rows are short labels on a wide field, so
    /// this is how a test asks "what is painted here, and in what color".
    fn runs(b: &Buffer, y: u16) -> Vec<(u16, String, ratatui::style::Color)> {
        let mut out: Vec<(u16, String, ratatui::style::Color)> = Vec::new();
        for x in 0..b.area.width {
            let cell = &b[(x, y)];
            if cell.symbol() == " " {
                continue;
            }
            match out.last_mut() {
                Some((sx, s, fg)) if *sx + s.chars().count() as u16 == x && *fg == cell.fg => {
                    s.push_str(cell.symbol())
                }
                _ => out.push((x, cell.symbol().to_string(), cell.fg)),
            }
        }
        out
    }

    /// Ruling R42: the scoring word's ladder is block letters or a plain bold
    /// line — there is no rung in between. The deleted sextant rung fired at
    /// exactly the widths pinned here (40–71: `TOUCHDOWN` is 9 glyphs × 8 =
    /// 72 cells at block size, and the app's floor is 40 cols), and it drew
    /// from U+1FB00–1FB3B, which Terminal.app's default font renders as tofu.
    /// The loudest moment the app has must not be a row of boxes.
    #[test]
    fn under_the_block_width_the_scoring_word_is_a_bold_line_not_a_glyph() {
        let g = game();
        let p = play("Mahomes 12 Yd pass to Kelce");
        let word = word_for(&g, &p);
        assert_eq!(word, "TOUCHDOWN");
        let block_cols = word.chars().count() as u16 * crate::tiles::glyph_cell().0;
        assert_eq!(block_cols, 72, "the width receipt this test is pinned to");

        let cut = a_cut(true, 0, &p);
        for w in [40u16, 55, 60, 71] {
            for h in [12u16, 20, 40] {
                let area = Rect::new(0, 0, w, h);
                let plan = plan(area, word);
                assert_eq!(
                    plan.word_form,
                    WordForm::Text,
                    "{w}x{h}: {block_cols} cells of block letters cannot fit {w} columns"
                );
                assert_eq!(
                    plan.word_rect.height, 1,
                    "{w}x{h}: the text form is one row"
                );

                // Cell level: the word row spells TOUCHDOWN in `hot`, and no
                // cell anywhere on the frame is a legacy-computing glyph.
                let b = drawn(w, h, |f| draw_takeover(f, area, &g, &cut, 0));
                let row = row_text(&b, plan.word_rect.y);
                assert!(row.contains(word), "{w}x{h}: word row is {row:?}");
                let x = row.find(word).unwrap() as u16;
                let r = theme::current().roles();
                assert_eq!(
                    b[(x, plan.word_rect.y)].fg,
                    r.hot,
                    "{w}x{h}: the word wears hot"
                );
                assert!(
                    b[(x, plan.word_rect.y)].modifier.contains(Modifier::BOLD),
                    "{w}x{h}: the text form carries its weight in bold"
                );
                for y in 0..h {
                    for x in 0..w {
                        let ch = b[(x, y)].symbol().chars().next().unwrap_or(' ');
                        assert!(
                            !(0x1FB00..=0x1FBFF).contains(&(ch as u32)),
                            "{w}x{h}: legacy-computing glyph U+{:04X} at ({x},{y}) — R42 deleted \
                             the rung that drew them\n{}",
                            ch as u32,
                            (0..h)
                                .map(|y| row_text(&b, y))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                    }
                }
            }
        }

        // And the rung above still fires the moment the columns are there.
        let wide = Rect::new(0, 0, 72, 40);
        assert_eq!(
            plan(wide, word).word_form,
            WordForm::Full,
            "72 columns is exactly enough"
        );
    }

    #[test]
    fn the_takeover_names_both_teams_in_their_colors() {
        // spec v3.3 §3: the takeover names who is playing, and each abbr
        // wears the same color as its own digits — including the away side,
        // which v3.2 left neutral.
        let mut g = game();
        g.away.color = [227, 24, 55]; // KC red
        g.home.color = [0, 51, 141]; // BUF blue
        let p = play("Mahomes 12 Yd pass to Kelce");
        let cut = a_cut(true, 0, &p);
        let area = Rect::new(0, 0, 120, 39);
        let b = drawn(120, 39, |f| draw_takeover(f, area, &g, &cut, 0));

        let th = theme::current();
        let (away_color, home_color, fell) = theme::hero_pair(&th, g.away.color, g.home.color);
        assert!(!fell, "KC red against BUF blue is not a lookalike pair");
        // The labels hang off the score's own rect: the digits are placed
        // first and the labels take what is left, never the other way round.
        let (score, _) = score_slot(area, &g, &p);
        let labels = runs(&b, score.bottom());
        assert_eq!(
            labels
                .iter()
                .map(|(_, s, _)| s.as_str())
                .collect::<Vec<_>>(),
            ["KC", "BUF"],
            "both teams are named under the score, away first"
        );
        assert_eq!(
            labels[0].2, away_color,
            "the away label wears the away digits' color"
        );
        assert_eq!(
            labels[1].2, home_color,
            "the home label wears the home digits' color"
        );

        // A lookalike pair: the HOME side is the one that falls back to
        // amber (theme::hero_pair's rule) — the away side keeps its color.
        let mut look = game();
        look.away.color = [12, 44, 86]; // SEA navy
        look.home.color = [19, 41, 75]; // BOS navy
        let (away2, home2, fell2) = theme::hero_pair(&th, look.away.color, look.home.color);
        assert!(
            fell2 && home2 == th.roles().digits,
            "the lookalike rule fired"
        );
        let b = drawn(120, 39, |f| draw_takeover(f, area, &look, &cut, 0));
        let (score, _) = score_slot(area, &look, &p);
        let labels = runs(&b, score.bottom());
        assert_eq!(labels[0].2, away2, "away is never the neutral fallback");
        assert_ne!(
            labels[0].2,
            th.roles().digits,
            "away keeps its own color when home lifts"
        );
        assert_eq!(labels[1].2, home2);

        // Down the ladder: both abbrs or neither, never one, and never two
        // that touch. A label is the first thing the frame gives up.
        let mut named_at: Vec<(u16, u16)> = Vec::new();
        for w in [40u16, 60, 80, 100, 120] {
            for h in [12u16, 16, 24, 40] {
                let area = Rect::new(0, 0, w, h);
                let b = drawn(w, h, |f| draw_takeover(f, area, &g, &cut, 0));
                // Ask the plan where the label row IS, rather than assuming
                // it is the row under the score. Ruling R42 dropped the
                // word's sextant rung, which changed which sizes can afford
                // labels at all — at 40×12 the plan now spends the middle on
                // a bold word plus a full-height score band and has no label
                // row, so "the row under the score" is the detail line and
                // the old guess read `MAHOMES · 12 YD PASS…` as a label.
                let Some(row) = plan(area, word_for(&g, &p)).labels else {
                    continue;
                };
                let named = runs(&b, row.y);
                let named: Vec<&str> = named.iter().map(|(_, s, _)| s.as_str()).collect();
                assert!(
                    named.is_empty() || named == ["KC", "BUF"],
                    "{w}x{h} label row is {named:?}"
                );
                if !named.is_empty() {
                    named_at.push((w, h));
                }
            }
        }
        for size in [(80, 24), (120, 40)] {
            assert!(
                named_at.contains(&size),
                "{size:?} has room to name both: {named_at:?}"
            );
        }
    }

    #[test]
    fn the_takeover_has_a_dimmed_strip_and_timer() {
        // spec v3.3 §3: the bottom strip is dim, names the matchup, and
        // carries the clear timer right-aligned.
        let g = game();
        let p = play("Mahomes 12 Yd pass to Kelce");
        let cut = a_cut(true, 0, &p);
        let area = Rect::new(0, 0, 120, 39);
        let b = drawn(120, 39, |f| draw_takeover(f, area, &g, &cut, 0));
        let r = theme::current().roles();

        let y = area.bottom() - 1;
        let text = row_text(&b, y);
        assert!(
            text.contains("CHIEFS AT BILLS"),
            "the strip names the game: {text:?}"
        );
        // Zero ticks elapsed against CUT_TICKS (3 s at LIVE_TICKS_PER_SEC = 10).
        assert_eq!(cut.remaining_secs(0), 3);
        assert!(
            text.ends_with("clears in 3s"),
            "the timer is right-aligned: {text:?}"
        );
        for x in 0..area.width {
            if b[(x, y)].symbol() != " " {
                assert_eq!(b[(x, y)].fg, r.dim, "the whole strip is dim, at ({x},{y})");
            }
        }
        // The clock actually moves: 1.2 s in, the strip says two.
        let b = drawn(120, 39, |f| draw_takeover(f, area, &g, &cut, 12));
        assert!(row_text(&b, y).ends_with("clears in 2s"));
    }

    #[test]
    fn the_band_second_row_offers_the_jump_and_counts_down() {
        // spec v3.3 §3: the band's second row stops repeating the play and
        // becomes the affordance — what enter does, and how long it lasts.
        let p = play("Mahomes 12 Yd pass to Kelce");
        let cut = a_cut(false, 0, &p);
        assert_eq!(cut.until_tick, BAND_TICKS);
        // 15 ticks = 1.5 s left, and the countdown ceils so the band never
        // shows a 0 it is still on screen for.
        assert_eq!(cut.remaining_secs(0), 2);
        assert_eq!(cut.remaining_secs(5), 1);
        assert_eq!(cut.remaining_secs(14), 1);
        assert_eq!(
            cut.remaining_secs(15),
            0,
            "expired means zero, not underflow"
        );
        assert_eq!(cut.remaining_secs(9_999), 0);

        let area = Rect::new(0, 0, 120, 2);
        let b = drawn(120, 2, |f| draw_band(f, area, &game(), &cut, 0));
        let text = row_text(&b, 1);
        assert!(
            text.starts_with("enter jump · clears in 2s"),
            "the affordance is the second row: {text:?}"
        );
        assert!(
            !text.contains("KELCE"),
            "the play is not repeated on row two: {text:?}"
        );
        let r = theme::current().roles();
        // Ink stays the ground role on the hot fill — `dim` is a
        // ground-relative gray and vanishes on hot (the v3.2 receipt) — so
        // the affordance is quieted by weight instead: row 0 is bold, this
        // one is not.
        assert_eq!(b[(0, 1)].fg, r.ground);
        assert_eq!(b[(0, 1)].bg, r.hot);
        assert!(
            b[(0, 0)].modifier.contains(Modifier::BOLD),
            "the headline is bold"
        );
        assert!(
            !b[(0, 1)].modifier.contains(Modifier::BOLD),
            "the affordance is not"
        );

        // One second later it counts down with the clock.
        let b = drawn(120, 2, |f| draw_band(f, area, &game(), &cut, 10));
        assert!(row_text(&b, 1).starts_with("enter jump · clears in 1s"));
    }
}
