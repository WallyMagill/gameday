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

/// The word this play earns: the league's scoring word, sharpened by the play
/// text where the text actually says something more specific (football's
/// field goals and safeties — see [`theme::scoring_word_for_play`]).
fn word_for(game: &Game, play: &Play) -> &'static str {
    theme::scoring_word_for_play(game.league, &play.text)
}

/// Which size the scoring word ended up at, and the rows it costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordForm {
    Full,
    Sextant,
    Text,
}

impl WordForm {
    fn rows(self) -> u16 {
        match self {
            WordForm::Full => crate::tiles::glyph_cell(true).1,
            WordForm::Sextant => crate::tiles::glyph_cell(false).1,
            WordForm::Text => 1,
        }
    }

    /// Cells `word` needs at this size.
    fn cols(self, word: &str) -> u16 {
        let glyphs = word.chars().count() as u16;
        match self {
            WordForm::Full => glyphs * crate::tiles::glyph_cell(true).0,
            WordForm::Sextant => glyphs * crate::tiles::glyph_cell(false).0,
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
    // (`tiles::glyph_cell`), so the longest word we ship — "TOUCHDOWN!", 10
    // glyphs including the "!" — needs 80 columns. (The brief's 72-col gate
    // counted a bare "TOUCHDOWN"; the word we actually ship carries the bang,
    // so 80 is the number.) Anything narrower steps down to sextant (4/glyph,
    // 40 columns) and then to a plain bold line. Never a clipped letter.
    let (word_form, score_full) = [
        (WordForm::Full, true),
        (WordForm::Full, false),
        (WordForm::Sextant, true),
        (WordForm::Sextant, false),
        (WordForm::Text, true),
        (WordForm::Text, false),
    ]
    .into_iter()
    .find(|(form, score_full)| {
        let score_rows = crate::tiles::glyph_cell(*score_full).1;
        form.cols(word) <= area.width && form.rows() + score_rows <= middle
    })
    .unwrap_or((WordForm::Text, false));

    let score_rows = crate::tiles::glyph_cell(score_full).1.min(middle);
    let stack = word_form.rows() + score_rows;
    let top = area.y + brackets.min(1);
    let word_y = top + middle.saturating_sub(stack) / 2;
    let word_rect = Rect { y: word_y, height: word_form.rows(), ..area };
    Plan {
        brackets,
        word_form,
        word_rect,
        score: Rect { y: word_rect.bottom(), height: score_rows, ..area },
        score_full,
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
pub fn draw_takeover(frame: &mut Frame, area: Rect, game: &Game, play: &Play) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();
    // The board behind is not drawn dimmed or blurred — it is gone. A cut
    // that leaves rows showing through reads as a rendering fault (spec §3).
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::default().bg(th.bg).fg(th.fg)), area);

    let word = word_for(game, play);
    let Plan { brackets, word_form, word_rect, score, score_full } = plan(area, word);

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
        form => {
            let cols = form.cols(word);
            let slot = Rect {
                x: word_rect.x + (word_rect.width - cols) / 2,
                width: cols,
                ..word_rect
            };
            crate::tiles::word_glyphs(frame, slot, word, r.hot, form == WordForm::Full);
        }
    }
    // The hard rule (spec §1): the takeover does not know how to draw a
    // score. It asks the hero's own formatter, so the cut and the board
    // behind it are the same digits, cell for cell.
    hero::score_block(frame, score, game, score_full);
    if brackets > 0 {
        let bottom = area.bottom();
        frame.render_widget(
            Paragraph::new(detail_line(game, play, area.width)).alignment(Alignment::Center),
            Rect { y: bottom - 2, height: 1, ..area },
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                strip_text(game),
                Style::default().fg(r.dim),
            ))
            .alignment(Alignment::Center),
            Rect { y: bottom - 1, height: 1, ..area },
        );
    }
}

/// Draw the band into the 2 rows above the list (spec §3, ruling R33): both
/// rows on the `hot` ground, `▲ HOME RUN · TEX Seager (32) · ATH 0 TEX 5` on
/// the first, the rest of the play on the second. Ink is the theme's ground
/// throughout — team color on a hot fill is unreadable, and the band's job is
/// to be a bar of the alert color that the eye catches above the list.
pub fn draw_band(frame: &mut Frame, area: Rect, game: &Game, play: &Play) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let th = theme::current();
    let r = th.roles();
    let on_hot = Style::default().fg(r.ground).bg(r.hot);
    let bold = on_hot.add_modifier(Modifier::BOLD);
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
        // The second row is the play itself. No dim role here: `dim` is a
        // ground-relative gray and vanishes on the hot fill.
        let (_, rest) = split_surname(&play.text);
        let clock = format!("{} {}", play.period, play.clock).trim().to_string();
        let tail = if clock.is_empty() {
            rest.to_uppercase()
        } else {
            format!("{} · {clock}", rest.to_uppercase())
        };
        frame.render_widget(
            Paragraph::new(Span::styled(truncate(&tail, area.width as usize), on_hot)),
            Rect { y: area.y + 1, height: 1, ..area },
        );
    }
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
        Style::default().fg(r.ground).bg(r.hot).add_modifier(Modifier::BOLD),
    ))
}

/// `MAHOMES · 12 YD PASS TO KELCE · Q4 1:52` — every part from the
/// `Play` itself. The scorer's surname is the play text's own first token:
/// ESPN writes these subject-first ("Mahomes pass to …", "Kelce 3 Yd pass
/// from …"), and a token that isn't a plain word is simply not treated as a
/// name rather than guessed at.
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
        spans.push(Span::styled(name.to_uppercase(), Style::default().fg(color).add_modifier(bold)));
        spans.push(Span::styled(" · ", Style::default().fg(r.dim)));
    }
    if !clock.is_empty() {
        budget = budget.saturating_sub(clock.chars().count() + 3);
    }
    if !rest.is_empty() {
        // Uppercase like the rest of the cut (spec §3's `4 YD RUSH`): ESPN
        // writes sentence case, and one lowercase clause under block letters
        // reads as a caption from another screen.
        spans.push(Span::styled(truncate(&rest.to_uppercase(), budget), Style::default().fg(r.ink)));
    }
    if !clock.is_empty() {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", Style::default().fg(r.dim)));
        }
        spans.push(Span::styled(clock, Style::default().fg(r.dim)));
    }
    Line::from(spans)
}

/// The play text's leading name token, and everything after it. `None` when
/// the first token isn't a plain capitalized word — nothing is invented.
fn split_surname(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.trim();
    let head = trimmed.split_whitespace().next().unwrap_or_default();
    // A handful of capitalized words that open a play sentence without being
    // anybody's name ("End of quarter", "Safety, snap out of the end zone").
    // A stop list, not a parser: the cost of a miss is one word painted in a
    // team color for three seconds.
    const NOT_A_NAME: [&str; 8] =
        ["End", "Safety", "Timeout", "Penalty", "Blocked", "Missed", "Two", "Extra"];
    let is_name = !NOT_A_NAME.iter().any(|w| w.eq_ignore_ascii_case(head))
        && head.len() > 1
        && head.chars().next().is_some_and(|c| c.is_uppercase())
        && head.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '.');
    if is_name {
        (Some(head), trimmed[head.len()..].trim_start())
    } else {
        (None, trimmed)
    }
}

/// The dim strip under the detail: who is playing, spelled out. The takeover
/// is the one surface with room for the full names.
fn strip_text(game: &Game) -> String {
    let name = |t: &crate::domain::Team| {
        if t.name.is_empty() { t.abbr.clone() } else { t.name.clone() }.to_uppercase()
    };
    format!("{} AT {}", name(&game.away), name(&game.home))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{League, Status, Team};

    fn play(text: &str) -> Play {
        Play {
            clock: "1:52".into(),
            period: "Q4".into(),
            team: "KC".into(),
            text: text.into(),
            scoring: true,
        }
    }

    fn game() -> Game {
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: Team { abbr: "KC".into(), name: "Chiefs".into(), ..Default::default() },
            home: Team { abbr: "BUF".into(), name: "Bills".into(), ..Default::default() },
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
        assert_eq!(cuts.active(105).unwrap().game_id, "1", "a band does not preempt a band");

        // A takeover does: the size is an upgrade, not a second event.
        cuts.fire("2", &play("Mahomes 12 yd pass"), true, 106);
        let cut = cuts.active(106).expect("the takeover replaced the band");
        assert!(cut.full);
        assert_eq!(cut.game_id, "2");
        assert_eq!(cut.until_tick, 106 + CUT_TICKS);

        // ...and is never downgraded while it is up.
        cuts.fire("3", &play("A quieter score"), false, 110);
        assert_eq!(cuts.active(110).unwrap().game_id, "2", "a takeover is never downgraded");

        // Expiry is read at the tick, not swept.
        assert!(cuts.active(106 + CUT_TICKS - 1).is_some());
        assert!(cuts.active(106 + CUT_TICKS).is_none(), "the cut ends on its deadline");

        // Once expired, anything may fire again.
        cuts.fire("3", &play("Later score"), false, 200);
        assert_eq!(cuts.active(200).unwrap().game_id, "3");
    }

    #[test]
    fn the_word_follows_the_play_text_where_the_text_is_specific() {
        assert_eq!(word_for(&game(), &play("Mahomes 12 Yd pass to Kelce")), "TOUCHDOWN!");
        assert_eq!(word_for(&game(), &play("Butker 41 Yd Field Goal")), "FIELD GOAL!");
        assert_eq!(word_for(&game(), &play("Jones sacked in end zone for a Safety")), "SAFETY!");
        // Ruling R34: touchdown wins over every other word in the sentence.
        // ESPN really writes these, and FIELD GOAL! on a return score is a
        // lie the screen tells for three seconds.
        assert_eq!(
            word_for(&game(), &play("Blocked Field Goal returned 62 yards for a TOUCHDOWN")),
            "TOUCHDOWN!"
        );
        assert_eq!(
            word_for(&game(), &play("Fumble on the Safety, recovered for a Touchdown")),
            "TOUCHDOWN!"
        );
        let mut nba = game();
        nba.league = League::Nba;
        // Basketball has no field goals in this sense: "field goal" arms are
        // football-only, so a basketball play keeps the league word.
        assert_eq!(word_for(&nba, &play("Jokic makes 3-pt field goal")), "BUCKET!");
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
            assert_eq!(split_surname(text).0, None, "{text:?} has no scorer to name");
            assert_eq!(split_surname(text).1, text, "the whole sentence survives: {text:?}");
        }
        assert_eq!(split_surname("Mahomes 12 Yd pass").0, Some("Mahomes"));
    }
}
