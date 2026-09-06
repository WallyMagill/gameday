//! Keys and cut rules for TV mode: the slate `n` walks, the follow and
//! hygiene passes data arrival runs, and the takeover's suppression gate.

use crate::app::{App, Derived, LIVE_TICKS_PER_SEC};
use crate::domain::{Game, Status};
use crate::input::InputMode;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
    /// TV mode: `space` locks the shown game, `n` walks the slate
    /// by hand, Esc/`v` pop back to the board. `q` quits — TV is a mode you
    /// leave the app from (the footer says `esc board  q quit`), unlike the
    /// read-only views where `q` only pops.
    pub(super) fn on_key_tv(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('v') => self.view = View::Board,
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char(' ') => {
                self.tv_lock = match self.tv_lock {
                    Some(_) => None,
                    None => self.tv_game_id(),
                };
            }
            KeyCode::Char('n') => self.tv_step(),
            _ => {}
        }
    }

    /// `n`: the next live game after the shown one, wrapping. A lock travels
    /// with it — you asked for this game, so it is the one that holds.
    fn tv_step(&mut self) {
        let d = self.derive();
        let slate = Self::tv_slate(&d);
        if slate.is_empty() {
            return;
        }
        // A shown game that is not on the slate has no "next" — the first
        // game is where `n` lands, not the second (an `unwrap_or(0)` here
        // stepped past `slate[0]` and made it unreachable in one press).
        let next = match self
            .tv_shown_in(&d)
            .and_then(|id| slate.iter().position(|g| g.id == id))
        {
            Some(at) => slate[(at + 1) % slate.len()].id.clone(),
            None => slate[0].id.clone(),
        };
        if self.tv_lock.is_some() {
            self.tv_lock = Some(next.clone());
        }
        self.tv_shown = Some(next);
    }

    /// After an event re-derived the order: TV follows the board's hero.
    /// Called only from the two `OrderState::on_event` sites, which is what
    /// makes "switches on the next event, never on a timer" true by
    /// construction — no timer can reach this.
    ///
    /// The rule is `Derived::hero_id` — MY GAMES' top while it is
    /// live, else the ranking's top — and not `OrderState`'s own top, which
    /// excludes MY GAMES by design (that exclusion exists to keep pins out of
    /// the IN PLAY band, not to define a ranking). Following it meant the
    /// first event cut away from your own team and, because the shown id
    /// could then never match, pinned `next cut:` on screen forever.
    pub(in crate::app) fn tv_follow(&mut self) {
        if !matches!(self.view, View::Tv) || self.tv_lock.is_some() {
            return;
        }
        if let Some(hero) = self.derive().hero_id {
            self.tv_shown = Some(hero);
        }
    }

    /// The other half: what TV was holding onto can leave the
    /// slate without any rank event at all — a MY GAMES game going final
    /// never moves the rank fingerprint (`live_all` excludes it), so
    /// `tv_follow` is never called for it. A lock that outlives its game is a
    /// trap (auto-cut off, footer still offering `space unlock`, nothing on
    /// screen explaining why), and a shown id that outlives its game leaves
    /// the state disagreeing with the picture. Both re-anchor to the hero
    /// rule here, on data arrival — never on a tick.
    pub(in crate::app) fn tv_hygiene(&mut self) {
        if !matches!(self.view, View::Tv) {
            return;
        }
        let d = self.derive();
        let slate = Self::tv_slate(&d);
        let gone = |id: &Option<String>| {
            id.as_ref()
                .is_some_and(|id| !slate.iter().any(|g| g.id == *id))
        };
        if gone(&self.tv_lock) {
            self.tv_lock = None;
        }
        if gone(&self.tv_shown) && self.tv_lock.is_none() {
            self.tv_shown = d.hero_id.clone();
        }
    }

    /// Open TV on whatever the board is featuring right now, unlocked.
    pub(crate) fn open_tv(&mut self) {
        self.view = View::Tv;
        self.tv_shown = self.derive().hero_id;
        self.tv_lock = None;
    }

    /// The game the next event will cut to, when that isn't the game already
    /// on screen. The ranking is recomputed here rather than read off the
    /// frozen order — between events the order is deliberately stale, and
    /// naming the next cut is the whole point of not switching yet. It is
    /// the same rule `tv_follow` will apply when the event lands, so
    /// the caption can never advertise a cut that then doesn't happen.
    pub(crate) fn tv_next_cut_in(&self, d: &Derived) -> Option<Game> {
        if self.tv_lock.is_some() {
            return None;
        }
        let shown = self.tv_shown_in(d);
        // Re-ranked: MY GAMES' top wins outright while it is
        // live; otherwise whichever IN PLAY game the ranking would lead with
        // right now. `d.in_play` (not `live_all`) is what keeps the caption
        // inside the tab and the `/` filter — a cut TV cannot make is worse
        // than no warning at all.
        let next = match d.my_games.first().filter(|g| g.status == Status::Live) {
            Some(game) => game.id.clone(),
            None => crate::rank::top_id(
                &d.in_play,
                self.config.sort,
                &self.config.enabled_tabs,
                self.now(),
            )?,
        };
        if Some(&next) == shown.as_ref() {
            return None;
        }
        Self::tv_slate(d)
            .into_iter()
            .find(|g| g.id == next)
            .cloned()
    }

    /// Same answer against a frame's already-derived lists — the draw path
    /// takes this one so a TV frame still derives exactly once.
    ///
    /// There is ONE slate. A shown id is kept only while it is
    /// still live and still on this tab; anything else falls back to the
    /// hero rule. Validating against `d.selection` (which carries finals and
    /// later games) let a game that had gone final stay "shown" while the
    /// draw looked it up in the live slate and printed "nothing is live"
    /// over a board with five live games.
    pub(crate) fn tv_shown_in(&self, d: &Derived) -> Option<String> {
        let slate = Self::tv_slate(d);
        let on_slate = |id: &String| slate.iter().any(|g| g.id == *id);
        self.tv_shown
            .clone()
            .filter(on_slate)
            .or_else(|| d.hero_id.clone().filter(on_slate))
    }

    /// The slate `n` walks: every live game, board order — the shown game
    /// plus the ALSO LIVE strip, in the order TV draws them.
    pub(crate) fn tv_slate(d: &Derived) -> Vec<&Game> {
        d.my_games
            .iter()
            .chain(d.in_play.iter())
            .filter(|g| g.status == Status::Live)
            .collect()
    }

    /// The game TV is showing: what an event or `n` last put there, falling
    /// back to the board's hero (`:tv` opened before any event landed, or the
    /// shown game left every board).
    pub(crate) fn tv_game_id(&self) -> Option<String> {
        self.tv_shown_in(&self.derive())
    }

    /// The cut refuses to fire at all during the first 30 s of a session
    /// (the boards arrive with history, and every one of those scores would
    /// otherwise take the screen) and while a prompt or the help overlay is
    /// open — an overlay over a prompt eats the keystroke the user is in the
    /// middle of.
    pub(in crate::app) fn cut_suppressed(&self) -> bool {
        self.tick < 30 * LIVE_TICKS_PER_SEC
            || !matches!(self.mode, InputMode::Normal)
            || self.help_open
            // The config view's favorite-abbr editor is a text prompt in
            // everything but the enum: keys land in `config_edit` char by
            // char, so a takeover over it means typing blind into a buffer
            // that is no longer on screen. The static config view and Zoom
            // stay coverable — they are read-and-arrow surfaces.
            || self.config_edit.is_some()
            // The theme picker is modal too (`input.rs` classes it next to
            // `help_open`) and its whole point is a live preview a takeover
            // would hide.
            || matches!(self.view, View::ThemePicker)
    }

    /// Does this game earn the whole screen? Pinned or favorited games always
    /// do. In TV mode, the game currently shown on screen does too — but any
    /// other game is a band drawn over TV, not a takeover: only the shown game
    /// and MY GAMES teams take the full screen in TV.
    pub(in crate::app) fn cut_is_full(&self, game: &Game) -> bool {
        self.is_my_game(game)
            || (matches!(self.view, View::Tv)
                && self.tv_shown_in(&self.derive()).as_deref() == Some(game.id.as_str()))
    }
}
