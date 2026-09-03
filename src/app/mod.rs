//! App state, the key handlers, and the shared chrome. `net` (connection
//! truth), `chrome` (header/footer/help) and `derive` (the per-frame game
//! lists) are children of this module — everything they touch lives on `App`.

mod chrome;
mod derive;
pub mod net;

pub use derive::Derived;

use crate::app::net::NetStatus;
use crate::config::{prune_pins, save_pins, Config, Favorite, Pin};
use crate::domain::{Game, GameStats, League, StandingsTable, Status, Summary};
use crate::input::{CompletionState, InputMode};
use crate::keymap;
use crate::theme;
use crate::ticker;
use crate::views::{self, View, ZoomTab};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    League(League),
}

/// Render ticks per second while anything is live (main's `LIVE_TICK` is
/// 100ms). Every seconds→ticks conversion (score flash, alert cooldown and
/// banner) derives from this one number.
pub const LIVE_TICKS_PER_SEC: u64 = 10;

/// Render ticks a score flash stays lit: ≈1s at the live cadence.
pub const FLASH_TICKS: u64 = LIVE_TICKS_PER_SEC;

/// PgUp/PgDn jump in the PlaysFeed, in rows. A guess at "most of a screen":
/// the feed body is ~30 rows at the default 120x36 capture size, and key
/// handling can't see the real pane height (draw takes &App).
const FEED_PAGE_JUMP: isize = 10;

/// LIVE chip pulse phase, pure in the tick: ~1s bright then ~1s dim at the
/// 10 ticks/s live cadence. A luminance step, never a hue change.
pub fn live_pulse_bright(tick: u64) -> bool {
    (tick / 10).is_multiple_of(2)
}

/// How far `[`/`]` can step the viewed slate from today, in days (the spec's
/// "yesterday ↔ today ↔ tomorrow, ±7 max").
pub const DATE_TRAVEL_MAX_DAYS: i8 = 7;

/// Header label for a traveled date: "FRI AUG 29" (weekday + month + day, no
/// year — the ±7-day window never crosses far enough to need one).
pub fn date_label(d: time::Date) -> String {
    format!(
        "{} {} {}",
        &format!("{:?}", d.weekday()).to_uppercase()[..3],
        &format!("{:?}", d.month()).to_uppercase()[..3],
        d.day()
    )
}

pub struct App {
    pub tab: Tab,
    pub selected: usize,
    pub pins: Vec<Pin>,
    pub config: Config,
    pub boards: HashMap<League, Vec<Game>>,
    /// Display order of the live band. Re-sorts only when data arrives (a
    /// fresh board apply, or a summary that moved the scoring plays), so the
    /// board never slides under the eye between events (spec §2).
    pub order: crate::rank::OrderState,
    /// Box scores by game id, filled by the ~30s stats poll while that game
    /// is zoomed. Pruned with `last_scores` when a game leaves every board.
    pub stats: HashMap<String, GameStats>,
    /// League standings, fetched on demand when `:standings` opens (10-min
    /// cache in the provider). At most one small table per league — no
    /// pruning needed.
    pub standings: HashMap<League, StandingsTable>,
    /// Connection truth: what the header chip, the footer's UPD age and the
    /// board's offline message all read, so they can never disagree.
    pub net: NetStatus,
    pub should_quit: bool,
    pub refresh_now: bool,
    /// Which full-screen surface the body renders; Board is the mosaic.
    /// Replaces the old `focused_id` mechanism — the zoomed game id lives
    /// inside `View::Zoom`.
    pub view: View,
    /// Highlighted row in the Zoom Plays feed (j/k); reset when the zoom
    /// opens or its tab changes.
    pub zoom_scroll: usize,
    /// TV (spec §3): the game filling the screen. Set when `:tv`/`v` opens,
    /// then moved by `n` or by an EVENT — never by a timer. `None` falls
    /// back to the board's own hero.
    pub tv_shown: Option<String>,
    /// TV: the game `space` locked onto. While it is Some, no event switches
    /// the screen; `n` still walks (and carries the lock with it).
    pub tv_lock: Option<String>,
    /// Highlighted row in the global PlaysFeed (`:plays`); reset when the
    /// view opens.
    pub feed_scroll: usize,
    /// Top-line offset in the Standings view (j/k, no highlight — the table
    /// is read-only); reset when the view opens. Clamped against
    /// `standings_visible` so it can never run past what the pane shows.
    pub standings_scroll: usize,
    /// Table rows the Standings pane showed on its last draw (the renderer
    /// records it, like hit zones). 0 until the first draw, when the clamp
    /// falls back to the line count.
    pub standings_visible: usize,
    /// Selected row in the Config view, an index into
    /// `views::config_view::rows`; reset when the view opens.
    pub config_cursor: usize,
    /// The favorite-abbr editor the ADD FAVORITE row opens: Some while
    /// typing (keys go to the buffer, Enter commits, Esc cancels).
    pub config_edit: Option<String>,
    /// Theme picker cursor: an index into `theme::names()`. Moving it
    /// applies that theme (live preview); Enter persists, Esc restores
    /// `theme_prior`.
    pub theme_cursor: usize,
    /// The theme that was current when the picker opened — Esc's target.
    pub theme_prior: String,
    /// Which input mode keys route through; Command/Filter carry the prompt
    /// buffer the footer renders. See `input::handle_key`.
    pub mode: InputMode,
    /// One-line footer message (command errors, pin results). Cleared by the
    /// next Normal-mode key or by opening a prompt.
    pub status_line: Option<String>,
    /// Committed `/` filter: case-insensitive substring matched against the
    /// abbr/location/name of either team. Applied inside `visible_games`, so
    /// every derived list (mosaic, slate, selection) narrows together.
    pub filter: Option<String>,
    /// Slate time-travel: days from today each league tab is viewing
    /// (`[`/`]` on the board, clamped to ±[`DATE_TRAVEL_MAX_DAYS`]). Missing
    /// or 0 = live today. Per league so travel on NFL never moves NBA.
    pub viewed_date_offset: HashMap<League, i8>,
    /// Fetched non-today slates, keyed by (league, date). Filled by
    /// `merge_dated_board` when the on-demand dated fetch answers; bounded by
    /// the ±7-day travel window per league.
    dated_boards: HashMap<(League, time::Date), Vec<Game>>,
    /// Command-mode Tab-completion cursor; owned by `input::cycle_completion`.
    pub completion: Option<CompletionState>,
    /// '?' overlay. Modal: Esc closes it before Esc touches focus.
    pub help_open: bool,
    pub config_dir: PathBuf,
    /// Set when the config or pins on disk could not be parsed. While it is
    /// Some, every `persist_*` is a no-op that re-arms the status line — a
    /// typo is never silently overwritten with defaults.
    pub config_error: Option<String>,
    /// The last error from an on-demand fetch, per (league, what) — `what` is
    /// the request kind ("standings", "dated", "summary", "stats"). Scoreboard
    /// failures live in `net`, which the header chip reads; these have no chip
    /// of their own, so the view that asked for the fetch shows the error
    /// instead of pretending the answer is still on its way. Cleared by the
    /// next success for the same key.
    pub aux_errors: HashMap<(League, &'static str), String>,
    /// Monotonic render tick (~10/s while live, ~1/s idle). Every animation
    /// is a pure function of this counter plus app state — keyboard input
    /// redraws but never advances it, so keys can't animate anything.
    pub tick: u64,
    /// Last-seen (away, home) score per game id: apply_boards diffs against
    /// this to detect data-driven score changes.
    last_scores: HashMap<String, (u16, u16)>,
    /// What the live band looked like the last time it was allowed to
    /// re-sort: id -> (away, home, status, hot). A poll that only advanced the
    /// clock leaves every fingerprint equal, so no reorder happens — the order
    /// is frozen even though watchability keeps rising (R24 / spec §2).
    rank_fingerprints: HashMap<String, (u16, u16, Status, bool)>,
    /// game id -> tick when its score last changed; drives the one-shot flash.
    flashes: HashMap<String, u64>,
    /// Favorite-score alert diff state (own score memory + per-game cooldown).
    alerts: crate::alerts::AlertState,
    /// The header banner currently showing, if any; expired by
    /// `advance_tick` once its `until_tick` passes.
    pub active_alert: Option<crate::alerts::Alert>,
    /// The scoring cut (spec §3): a full-frame takeover for a game you care
    /// about, a quiet 2-row band for everything else. Fired from the score
    /// delta below; read once per draw.
    pub cuts: crate::board::cut::CutState,
    /// Set when a banner starts; main consumes it to write the terminal
    /// bell (`\x07`) — App never touches stdout itself.
    pub bell_pending: bool,
    /// Clickable regions, rebuilt from scratch on every draw by whichever
    /// view rendered (tiles, tab chips, slate rows, zoom tabs). A click
    /// resolves against the LAST frame's zones — stale for at most one
    /// render tick.
    pub hit_zones: Vec<(Rect, keymap::Hit)>,
    /// The local UTC offset, read once on the main thread at startup
    /// (`text::startup_offset`). Every clock the app renders goes through
    /// [`App::now`] so nothing calls `now_local()` off the main thread.
    pub offset: time::UtcOffset,
    /// Frozen clock: when set, [`App::now`] returns this instead of reading
    /// the wall clock. Dumps and draw tests set it so a capture of the same
    /// tick is the same pixels every run; the real app leaves it None.
    pub now_override: Option<OffsetDateTime>,
    /// This frame's derived lists — Some only between the first and last
    /// lines of [`App::draw`]. Widgets read it through `derived()`; nothing
    /// outside a draw may, which is why it is private and cleared.
    frame_cache: Option<Derived>,
}

impl App {
    pub fn new(
        config: Config,
        pins: Vec<Pin>,
        config_dir: PathBuf,
        offset: time::UtcOffset,
    ) -> Self {
        Self {
            tab: Tab::Home,
            selected: 0,
            pins,
            config,
            boards: HashMap::new(),
            order: crate::rank::OrderState::default(),
            stats: HashMap::new(),
            standings: HashMap::new(),
            net: NetStatus::default(),
            should_quit: false,
            refresh_now: false,
            view: View::Board,
            zoom_scroll: 0,
            tv_shown: None,
            tv_lock: None,
            feed_scroll: 0,
            standings_scroll: 0,
            standings_visible: 0,
            config_cursor: 0,
            config_edit: None,
            theme_cursor: 0,
            theme_prior: String::new(),
            mode: InputMode::Normal,
            status_line: None,
            filter: None,
            viewed_date_offset: HashMap::new(),
            dated_boards: HashMap::new(),
            completion: None,
            help_open: false,
            config_dir,
            config_error: None,
            aux_errors: HashMap::new(),
            tick: 0,
            last_scores: HashMap::new(),
            rank_fingerprints: HashMap::new(),
            flashes: HashMap::new(),
            alerts: crate::alerts::AlertState::default(),
            active_alert: None,
            cuts: crate::board::cut::CutState::default(),
            bell_pending: false,
            hit_zones: Vec::new(),
            offset,
            now_override: None,
            frame_cache: None,
        }
    }

    /// The only place config.toml is written. A config we could not parse is
    /// never overwritten: the save is skipped and the footer says why, every
    /// time, so the user can go fix the file.
    pub fn persist_config(&mut self) {
        if let Some(err) = &self.config_error {
            self.status_line = Some(format!("not saving: {err}"));
            return;
        }
        if let Err(e) = self.config.save_to(&self.config_dir) {
            self.status_line = Some(format!("config save failed: {e}"));
        }
    }

    /// The only place pins.json is written from a key the user pressed; same
    /// refusal as `persist_config`, and it says so.
    pub fn persist_pins(&mut self) {
        if let Some(err) = &self.config_error {
            self.status_line = Some(format!("not saving: {err}"));
            return;
        }
        self.persist_pins_quiet();
    }

    /// The same write from a background path (the prune inside `apply_boards`,
    /// which runs on every poll). A broken config skips it in silence: the
    /// startup status line already says saving is off, and re-toasting it
    /// every merge would stomp whatever the user's last key said.
    pub fn persist_pins_quiet(&mut self) {
        if self.config_error.is_some() {
            return;
        }
        if let Err(e) = save_pins(&self.config_dir, &self.pins) {
            self.status_line = Some(format!("pins save failed: {e}"));
        }
    }

    /// Record a config/pins parse failure: it blocks every save and takes the
    /// footer once, at startup, so the reason is on screen and not only on the
    /// stderr that the alternate screen swallowed.
    pub fn set_config_error(&mut self, err: Option<String>) {
        self.status_line = err
            .as_ref()
            .map(|e| format!("config error: {e} — not saving until fixed"));
        self.config_error = err;
    }

    /// Now, in the user's local offset — or the frozen clock when one is set.
    /// The one clock the app reads.
    pub fn now(&self) -> OffsetDateTime {
        self.now_override
            .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(self.offset))
    }

    /// The registered hit under `pos`, last-drawn zone winning (later
    /// registrations sit on top of earlier ones).
    pub fn hit_at(&self, pos: ratatui::layout::Position) -> Option<keymap::Hit> {
        self.hit_zones
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, hit)| *hit)
    }

    /// Apply one resolved mouse gesture. Clicks mirror the keyboard verbs
    /// (select, switch tab); the wheel scrolls whatever j/k scrolls in the
    /// current view.
    pub fn on_hit(&mut self, hit: keymap::Hit) {
        use keymap::Hit;
        match hit {
            Hit::Row(i) => {
                self.selected = i;
                self.clamp_selected();
            }
            // A header click pops the picker like the Tab key: the preview
            // must not leak out as the live theme, so it reverts first.
            Hit::TabChip(tab) => {
                if self.view == View::ThemePicker {
                    self.revert_theme_preview();
                }
                self.set_tab(tab);
            }
            Hit::ZoomTab(t) => {
                if let View::Zoom { tab, .. } = &mut self.view {
                    *tab = t;
                    self.zoom_scroll = 0;
                }
            }
            Hit::ScrollUp | Hit::ScrollDown => {
                let delta = if hit == Hit::ScrollUp { -1 } else { 1 };
                match self.view {
                    View::Board => self.move_selected(delta),
                    View::Zoom { .. } => self.move_zoom_scroll(delta),
                    View::PlaysFeed => self.move_feed_scroll(delta),
                    View::Standings(_) => self.move_standings_scroll(delta),
                    View::ConfigView => self.move_config_cursor(delta),
                    View::ThemePicker => self.move_theme_cursor(delta),
                    View::Tv => {}
                }
            }
        }
    }

    /// One render tick. Expired flashes are dropped here, so a settled score
    /// never re-flashes (one-shot).
    pub fn advance_tick(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        self.flashes
            .retain(|_, start| tick.saturating_sub(*start) < FLASH_TICKS);
        // The alert banner is one-shot too: past its lifetime it vanishes
        // and only a fresh score delta can bring one back.
        if self.active_alert.as_ref().is_some_and(|a| tick >= a.until_tick) {
            self.active_alert = None;
        }
    }

    /// Is `game_id` inside its ~1s score-flash window? Pure in (tick, flashes).
    pub fn flash_active(&self, game_id: &str) -> bool {
        self.flashes
            .get(game_id)
            .is_some_and(|start| self.tick.saturating_sub(*start) < FLASH_TICKS)
    }

    /// Any live game on any board — the render loop runs at ~10fps while true
    /// and drops to ~1fps otherwise. An active cut counts: its 3 s / 1.5 s
    /// lifetimes are written in 10-tick seconds (`cut::CUT_TICKS`), so the
    /// overlay pins the loop to the live cadence for as long as it is up,
    /// exactly like a live board does.
    pub fn any_live(&self) -> bool {
        self.cuts.active(self.tick).is_some()
            || self
                .boards
                .values()
                .flatten()
                .any(|g| g.status == Status::Live)
    }

    /// The cut refuses to fire at all during the first 30 s of a session
    /// (the boards arrive with history, and every one of those scores would
    /// otherwise take the screen) and while a prompt or the help overlay is
    /// open — an overlay over a prompt eats the keystroke the user is in the
    /// middle of (spec §3).
    fn cut_suppressed(&self) -> bool {
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

    /// Does this game earn the whole screen? Pinned, favorited, or TV mode —
    /// where the one game on screen is the only thing there is.
    fn cut_is_full(&self, game: &Game) -> bool {
        matches!(self.view, View::Tv)
            || self.pins.iter().any(|p| p.game_id == game.id)
            || self.config.favorites.iter().any(|f| {
                f.league == game.league
                    && (f.team_abbr.eq_ignore_ascii_case(&game.away.abbr)
                        || f.team_abbr.eq_ignore_ascii_case(&game.home.abbr))
            })
    }

    pub fn tab_list(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Home];
        tabs.extend(self.config.enabled_tabs.iter().copied().map(Tab::League));
        tabs
    }

    pub fn current_tab_index(&self) -> usize {
        self.tab_list()
            .iter()
            .position(|t| *t == self.tab)
            .unwrap_or(0)
    }

    /// The date `league`'s tab is viewing, None when it's live today.
    pub fn viewed_date(&self, league: League) -> Option<time::Date> {
        let off = self.viewed_date_offset.get(&league).copied().unwrap_or(0);
        if off == 0 {
            return None;
        }
        self.now().date().checked_add(time::Duration::days(off as i64))
    }

    /// `[`/`]` on the board: step the current league tab's viewed date,
    /// clamped to ±[`DATE_TRAVEL_MAX_DAYS`]. No-op on Home (no league).
    fn step_viewed_date(&mut self, delta: i8) {
        let Tab::League(league) = self.tab else {
            return;
        };
        let off = self.viewed_date_offset.entry(league).or_insert(0);
        *off = (*off + delta).clamp(-DATE_TRAVEL_MAX_DAYS, DATE_TRAVEL_MAX_DAYS);
        self.clamp_selected();
    }

    /// The (league, date) the on-demand dated fetch should answer for — the
    /// current tab while it's date-traveled and that slate isn't loaded yet.
    /// Same handshake shape as `stats_target`/`standings_target`.
    pub fn dated_target(&self) -> Option<(League, time::Date)> {
        let Tab::League(league) = self.tab else {
            return None;
        };
        let date = self.viewed_date(league)?;
        (!self.dated_boards.contains_key(&(league, date))).then_some((league, date))
    }

    /// A fetched non-today slate. Replaces wholesale (a dated board is a
    /// snapshot) and never touches flash/score state — traveled slates are
    /// read-only history/preview, not live data.
    pub fn merge_dated_board(&mut self, league: League, date: time::Date, games: Vec<Game>) {
        self.clear_aux_error(league, "dated");
        self.dated_boards.insert((league, date), games);
        self.clamp_selected();
    }

    /// The filter the board is narrowed by right now: the open `/` prompt's
    /// buffer while typing (incremental), else the committed filter.
    pub(crate) fn active_filter(&self) -> Option<&str> {
        if let InputMode::Filter { buf } = &self.mode {
            return (!buf.is_empty()).then_some(buf.as_str());
        }
        self.filter.as_deref()
    }

    pub fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // Raw mode swallows SIGINT, so Ctrl+C must be an explicit quit.
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // Help is modal: Esc, '?' and 'q' all close it — 'q' inside help
        // dismisses the overlay, never the app (quitting from behind a
        // modal you opened by accident is the wrong surprise). Ctrl+C above
        // is still the escape hatch. Everything else is inert while open.
        if self.help_open {
            match code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.help_open = false,
                _ => {}
            }
            return;
        }
        match self.view {
            View::Board => self.on_key_board(code),
            View::Zoom { .. } => self.on_key_zoom(code),
            View::PlaysFeed => self.on_key_plays_feed(code),
            View::Standings(_) => self.on_key_standings(code),
            View::ConfigView => self.on_key_config(code),
            View::ThemePicker => self.on_key_theme_picker(code),
            View::Tv => self.on_key_tv(code),
        }
    }

    /// TV mode (spec §3): `space` locks the shown game, `n` walks the slate
    /// by hand, Esc/`v` pop back to the board. `q` quits — TV is a mode you
    /// leave the app from (the footer says `esc board  q quit`), unlike the
    /// read-only views where `q` only pops.
    fn on_key_tv(&mut self, code: KeyCode) {
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

    /// Open TV on whatever the board is featuring right now, unlocked.
    pub(crate) fn open_tv(&mut self) {
        self.view = View::Tv;
        self.tv_shown = self.derive().hero_id;
        self.tv_lock = None;
    }

    /// The game TV is showing: what an event or `n` last put there, falling
    /// back to the board's hero (`:tv` opened before any event landed, or the
    /// shown game left every board).
    pub(crate) fn tv_game_id(&self) -> Option<String> {
        self.tv_shown_in(&self.derive())
    }

    /// Same answer against a frame's already-derived lists — the draw path
    /// takes this one so a TV frame still derives exactly once.
    ///
    /// Ruling R36: there is ONE slate. A shown id is kept only while it is
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

    /// The game the next event will cut to, when that isn't the game already
    /// on screen. The ranking is recomputed here rather than read off the
    /// frozen order — between events the order is deliberately stale, and
    /// naming the next cut is the whole point of not switching yet. It is
    /// the same rule `tv_follow` will apply when the event lands (R35), so
    /// the caption can never advertise a cut that then doesn't happen.
    pub(crate) fn tv_next_cut_in(&self, d: &Derived) -> Option<Game> {
        if self.tv_lock.is_some() {
            return None;
        }
        let shown = self.tv_shown_in(d);
        // Ruling R35, re-ranked: MY GAMES' top wins outright while it is
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

    /// After an event re-derived the order: TV follows the board's hero.
    /// Called only from the two `OrderState::on_event` sites, which is what
    /// makes "switches on the next event, never on a timer" (spec §3) true by
    /// construction — no timer can reach this.
    ///
    /// Ruling R35: the rule is `Derived::hero_id` — MY GAMES' top while it is
    /// live, else the ranking's top — and not `OrderState`'s own top, which
    /// excludes MY GAMES by design (that exclusion exists to keep pins out of
    /// the IN PLAY band, not to define a ranking). Following it meant the
    /// first event cut away from your own team and, because the shown id
    /// could then never match, pinned `next cut:` on screen forever.
    fn tv_follow(&mut self) {
        if !matches!(self.view, View::Tv) || self.tv_lock.is_some() {
            return;
        }
        if let Some(hero) = self.derive().hero_id {
            self.tv_shown = Some(hero);
        }
    }

    /// Ruling R36, the other half: what TV was holding onto can leave the
    /// slate without any rank event at all — a MY GAMES game going final
    /// never moves the rank fingerprint (`live_all` excludes it), so
    /// `tv_follow` is never called for it. A lock that outlives its game is a
    /// trap (auto-cut off, footer still offering `space unlock`, nothing on
    /// screen explaining why), and a shown id that outlives its game leaves
    /// the state disagreeing with the picture. Both re-anchor to the hero
    /// rule here, on data arrival — never on a tick.
    fn tv_hygiene(&mut self) {
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

    /// `:theme` with no argument: remember the current theme (Esc's target),
    /// land the cursor on it, and show the picker over the board.
    pub fn open_theme_picker(&mut self) {
        // Already open: the current theme is a preview, not the prior.
        if self.view == View::ThemePicker {
            return;
        }
        self.theme_prior = theme::current_name();
        self.theme_cursor = theme::names()
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&self.theme_prior))
            .unwrap_or(0);
        self.view = View::ThemePicker;
    }

    /// Keys in the theme picker: j/k move the cursor and apply that theme at
    /// once (the board underneath is the preview), Enter keeps it and
    /// persists, Esc/q put the prior theme back. Tab still switches league
    /// tabs — that pops the picker, so it reverts first. The picker is modal
    /// otherwise: `input.rs` keeps ':' and '/' inert while it is up, so no
    /// command can pop it with the preview still live.
    fn on_key_theme_picker(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_theme_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_theme_cursor(-1),
            KeyCode::Enter => {
                self.config.theme = theme::current_name();
                self.persist_config();
                self.view = View::Board;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.revert_theme_preview();
                self.view = View::Board;
            }
            KeyCode::Tab => {
                self.revert_theme_preview();
                self.cycle_tab(1);
            }
            KeyCode::BackTab => {
                self.revert_theme_preview();
                self.cycle_tab(-1);
            }
            KeyCode::Char('?') => self.help_open = true,
            _ => {}
        }
    }

    fn revert_theme_preview(&mut self) {
        // The prior theme is always a loaded name (it was current); if a
        // user file vanished mid-session, broadcast is the honest fallback.
        if theme::set_current(&self.theme_prior).is_err() {
            let _ = theme::set_current("broadcast");
        }
    }

    /// Move the picker cursor `delta` rows (wrapping) and preview that theme.
    fn move_theme_cursor(&mut self, delta: isize) {
        let names = theme::names();
        let n = names.len() as isize;
        self.theme_cursor = (self.theme_cursor as isize + delta).rem_euclid(n) as usize;
        let _ = theme::set_current(&names[self.theme_cursor]);
    }

    /// Keys in the Config view: j/k move the row cursor, space/enter activate
    /// (toggle a tab, remove a favorite, open the abbr editor), h/l cycle the
    /// display rows, Esc/q pop to the board. An open abbr editor captures
    /// everything first (Enter commits, Esc cancels).
    fn on_key_config(&mut self, code: KeyCode) {
        use crate::views::config_view::{rows, ConfigRow};
        if self.config_edit.is_some() {
            self.on_key_config_edit(code);
            return;
        }
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_config_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_config_cursor(-1),
            KeyCode::Char(' ') | KeyCode::Enter => {
                let rows = rows(self);
                match rows[self.config_cursor.min(rows.len() - 1)] {
                    ConfigRow::Tab(league) => self.config_toggle_tab(league),
                    ConfigRow::Favorite(i) => self.config_remove_favorite(i),
                    ConfigRow::AddFavorite => self.config_edit = Some(String::new()),
                    // Enter on a cycler steps it forward, same as l.
                    ConfigRow::Theme | ConfigRow::Sort => self.config_cycle(1),
                }
            }
            KeyCode::Char('h') | KeyCode::Left => self.config_cycle(-1),
            KeyCode::Char('l') | KeyCode::Right => self.config_cycle(1),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('?') => self.help_open = true,
            _ => {}
        }
    }

    /// Keys while the favorite-abbr editor is open.
    fn on_key_config_edit(&mut self, code: KeyCode) {
        let Some(buf) = &mut self.config_edit else {
            return;
        };
        match code {
            KeyCode::Esc => self.config_edit = None,
            KeyCode::Enter => {
                let text = self.config_edit.take().unwrap_or_default();
                let text = text.trim().to_string();
                if !text.is_empty() {
                    self.config_add_favorite(&text);
                }
            }
            // Backspacing past the start closes the editor (prompt habit).
            KeyCode::Backspace => {
                if buf.pop().is_none() {
                    self.config_edit = None;
                }
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        }
    }

    fn move_config_cursor(&mut self, delta: isize) {
        let n = crate::views::config_view::rows(self).len();
        let next = self.config_cursor as isize + delta;
        self.config_cursor = next.clamp(0, n as isize - 1) as usize;
    }

    /// Space on a TABS row: toggle the league in `enabled_tabs`. The list is
    /// always re-sorted into `League::ALL` order, so toggling a league off
    /// and back on returns it to its place in the bar instead of parking it
    /// at the end — the tab order is a property of the league, not of the
    /// order you happened to click. If the current tab was just disabled,
    /// fall back to Home.
    pub(crate) fn config_toggle_tab(&mut self, league: League) {
        if let Some(i) = self.config.enabled_tabs.iter().position(|l| *l == league) {
            self.config.enabled_tabs.remove(i);
            if self.tab == Tab::League(league) {
                self.tab = Tab::Home;
            }
        } else {
            self.config.enabled_tabs.push(league);
        }
        self.config
            .enabled_tabs
            .sort_by_key(|l| League::ALL.iter().position(|x| x == l));
        self.persist_config();
    }

    fn config_remove_favorite(&mut self, i: usize) {
        if i < self.config.favorites.len() {
            self.config.favorites.remove(i);
            self.persist_config();
            self.force_reorder(); // membership change — see `toggle_pin`
        }
        self.move_config_cursor(0); // re-clamp against the shrunk row list
    }

    /// Commit the typed favorite. `kc` resolves its league from the enabled
    /// boards (like `:pin`); `nhl edm` names it directly for teams not
    /// currently playing.
    fn config_add_favorite(&mut self, text: &str) {
        let parts: Vec<&str> = text.split_whitespace().collect();
        let (league, abbr) = match parts.as_slice() {
            [abbr] => {
                let found = self.config.enabled_tabs.iter().find_map(|lg| {
                    self.boards.get(lg).into_iter().flatten().find_map(|g| {
                        [&g.away, &g.home]
                            .into_iter()
                            .find(|t| t.abbr.eq_ignore_ascii_case(abbr))
                            .map(|t| (g.league, t.abbr.clone()))
                    })
                });
                match found {
                    Some(hit) => hit,
                    None => {
                        self.status_line = Some(format!(
                            "no team {abbr:?} on enabled boards; use \"<league> <abbr>\" like \"nfl kc\""
                        ));
                        return;
                    }
                }
            }
            [slug, abbr] => match League::from_slug(&slug.to_lowercase()) {
                Some(league) => (league, abbr.to_string()),
                None => {
                    self.status_line = Some(format!(
                        "unknown league {slug:?}, valid: {}",
                        League::ALL.map(League::slug).join("|")
                    ));
                    return;
                }
            },
            _ => {
                self.status_line = Some(format!(
                    "expected \"<abbr>\" or \"<league> <abbr>\", got {text:?}"
                ));
                return;
            }
        };
        let abbr = abbr.to_uppercase();
        if self
            .config
            .favorites
            .iter()
            .any(|f| f.league == league && f.team_abbr.eq_ignore_ascii_case(&abbr))
        {
            self.status_line = Some(format!(
                "{} {} is already a favorite",
                league.slug().to_uppercase(),
                abbr
            ));
            return;
        }
        self.status_line = Some(format!(
            "favorited {} {}",
            league.slug().to_uppercase(),
            abbr
        ));
        self.config.favorites.push(Favorite {
            league,
            team_abbr: abbr,
        });
        self.persist_config();
        self.force_reorder(); // membership change — see `toggle_pin`
    }

    /// h/l on a display row: cycle its value and persist. No-op on rows that
    /// don't cycle.
    fn config_cycle(&mut self, delta: isize) {
        use crate::views::config_view::{rows, ConfigRow};
        let rows = rows(self);
        match rows[self.config_cursor.min(rows.len() - 1)] {
            ConfigRow::Theme => {
                let next = theme::next_name(&theme::current_name(), delta);
                let _ = theme::set_current(&next);
                self.config.theme = next;
            }
            ConfigRow::Sort => {
                self.config.sort = if delta >= 0 {
                    self.config.sort.cycled()
                } else {
                    // Only three values: cycling forward twice is cycling
                    // back once.
                    self.config.sort.cycled().cycled()
                };
                self.force_reorder();
            }
            ConfigRow::Tab(_) | ConfigRow::Favorite(_) | ConfigRow::AddFavorite => return,
        }
        self.persist_config();
    }

    /// Keys in the Standings view: j/k scroll the table one line, PgUp/PgDn
    /// jump, Tab switches league (landing on the board, as in Zoom), Esc/q
    /// pop back to the board (q quits ONLY there).
    fn on_key_standings(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_standings_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_standings_scroll(-1),
            KeyCode::PageDown => self.move_standings_scroll(FEED_PAGE_JUMP),
            KeyCode::PageUp => self.move_standings_scroll(-FEED_PAGE_JUMP),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('r') => self.refresh_now = true,
            _ => {}
        }
    }

    /// Scroll the Standings table. The offset is clamped so the last line
    /// lands on the last pane row (`standings_visible`, recorded by the last
    /// draw) — the stored value never runs past what is shown, so `k` after
    /// the bottom moves the table on the first press. Before any draw the
    /// pane height is unknown and the clamp falls back to the line count.
    fn move_standings_scroll(&mut self, delta: isize) {
        let lines = self
            .standings_target()
            .and_then(|l| self.standings.get(&l))
            .map(crate::views::standings::line_count)
            .unwrap_or(0);
        if lines == 0 {
            self.standings_scroll = 0;
            return;
        }
        let max = match self.standings_visible {
            0 => lines - 1,
            visible => lines.saturating_sub(visible),
        };
        let next = self.standings_scroll as isize + delta;
        self.standings_scroll = next.clamp(0, max as isize) as usize;
    }

    /// Keys inside the global PlaysFeed: j/k move the highlight one row,
    /// PgUp/PgDn jump, Tab switches league (landing on the board), Esc/q pop
    /// back to the board (q quits ONLY there).
    fn on_key_plays_feed(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_feed_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_feed_scroll(-1),
            KeyCode::PageDown => self.move_feed_scroll(FEED_PAGE_JUMP),
            KeyCode::PageUp => self.move_feed_scroll(-FEED_PAGE_JUMP),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('r') => self.refresh_now = true,
            _ => {}
        }
    }

    /// j/k/PgUp/PgDn in the PlaysFeed: move the highlight, clamped to the
    /// current scoring-event list (the renderer re-clamps if boards shrink
    /// between a keypress and the next draw).
    fn move_feed_scroll(&mut self, delta: isize) {
        let len = self.scoring_events().len();
        if len == 0 {
            self.feed_scroll = 0;
            return;
        }
        let next = self.feed_scroll as isize + delta;
        self.feed_scroll = next.clamp(0, len as isize - 1) as usize;
    }

    fn on_key_board(&mut self, code: KeyCode) {
        match code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => self.cycle_tab(1),
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => self.cycle_tab(-1),
            KeyCode::Char('j') | KeyCode::Down => self.move_selected(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selected(-1),
            KeyCode::Char(' ') => self.toggle_pin(),
            KeyCode::Enter | KeyCode::Char('z') => self.zoom_selected(),
            // Esc on the board clears an active filter (modes pop in input.rs).
            KeyCode::Esc => {
                if self.filter.is_some() {
                    self.filter = None;
                    self.filter_changed();
                }
            }
            KeyCode::Char('t') => self.toggle_favorite(),
            KeyCode::Char('[') => self.step_viewed_date(-1),
            KeyCode::Char(']') => self.step_viewed_date(1),
            // v3.2 §7: n/p paging is deleted — the board is one scrolling
            // list, so PgDn/PgUp have nothing to page and n/p are free again.
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('v') => self.open_tv(),
            KeyCode::Char('c') => self.cycle_theme(),
            KeyCode::Char('r') => self.refresh_now = true,
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    /// Keys inside the zoomed view: h/l and [/] cycle the tab, j/k move the
    /// Plays highlight, Esc/q/z pop back to the board (q quits ONLY from the
    /// board), Tab still switches league tabs (which pops the zoom).
    fn on_key_zoom(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => self.cycle_zoom_tab(-1),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => self.cycle_zoom_tab(1),
            KeyCode::Char('j') | KeyCode::Down => self.move_zoom_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_zoom_scroll(-1),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('z') => self.view = View::Board,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('r') => self.refresh_now = true,
            _ => {}
        }
    }

    /// `z`/Enter on the board: zoom the selected game, opening on Overview.
    fn zoom_selected(&mut self) {
        if let Some(game) = self.selected_game() {
            self.zoom_game_id(&game.id);
        }
    }

    /// Zoom a game by id, opening on Overview. The selection is not the only
    /// way in any more: the band's `enter` jump (spec v3.3 §3) names the game
    /// that just scored, which is rarely the one under the cursor.
    pub(crate) fn zoom_game_id(&mut self, game_id: &str) {
        self.view = View::Zoom {
            game_id: game_id.to_string(),
            tab: ZoomTab::Overview,
        };
        self.zoom_scroll = 0;
    }

    fn cycle_zoom_tab(&mut self, delta: isize) {
        if let View::Zoom { tab, .. } = &mut self.view {
            *tab = tab.cycled(delta);
            self.zoom_scroll = 0;
        }
    }

    /// j/k in the Zoom Plays/Stats tabs: move the highlight/window, clamped
    /// to whichever list the active tab shows.
    fn move_zoom_scroll(&mut self, delta: isize) {
        let len = match &self.view {
            View::Zoom { game_id, tab: ZoomTab::Stats } => self
                .stats
                .get(game_id)
                .map(|s| s.rows.len())
                .unwrap_or(0),
            _ => self
                .zoomed_game()
                .map(|g| g.last_plays.len())
                .unwrap_or(0),
        };
        if len == 0 {
            self.zoom_scroll = 0;
            return;
        }
        let next = self.zoom_scroll as isize + delta;
        self.zoom_scroll = next.clamp(0, len as isize - 1) as usize;
    }

    /// Every live game on an enabled board, minus the viewer's own. Pins AND
    /// favorites live in the MY GAMES band and never re-sort (spec §1, ruling
    /// R26), so `OrderState` is never told about either.
    pub fn live_all(&self) -> Vec<Game> {
        self.config
            .enabled_tabs
            .iter()
            .filter_map(|l| self.boards.get(l))
            .flatten()
            .filter(|g| g.status == Status::Live)
            .filter(|g| !self.is_my_game(g))
            .cloned()
            .collect()
    }

    /// Re-sort the live band, but only if the data behind the order actually
    /// moved: the id set changed, or some game's score, status or hot flag
    /// did. A clock that merely advanced is not an event (R24) — spec §2:
    /// "Between events the order is frozen even though L keeps rising."
    fn maybe_reorder(&mut self) {
        let live = self.live_all();
        let now = self.now();
        let fps: HashMap<String, (u16, u16, Status, bool)> = live
            .iter()
            .map(|g| {
                (
                    g.id.clone(),
                    (
                        g.away_score,
                        g.home_score,
                        g.status,
                        crate::rank::watchability(g, now).hot,
                    ),
                )
            })
            .collect();
        let changed = fps.len() != self.rank_fingerprints.len()
            || fps
                .iter()
                .any(|(id, f)| self.rank_fingerprints.get(id) != Some(f));
        if changed {
            self.order.on_event(
                &live,
                self.config.sort,
                &self.config.enabled_tabs,
                now,
                self.tick,
            );
            self.tv_follow();
        }
        self.rank_fingerprints = fps;
    }

    /// A sort-key change (`s`, `:sort`, the config editor's SORT row) is a
    /// real event on its own: unlike a clock tick, it must re-derive the
    /// order right away rather than waiting for `maybe_reorder`'s fingerprint
    /// gate to see a score/status change.
    pub fn force_reorder(&mut self) {
        let live = self.live_all();
        let now = self.now();
        self.order.on_event(
            &live,
            self.config.sort,
            &self.config.enabled_tabs,
            now,
            self.tick,
        );
        // A new sort key is a new ranking, and TV shows the ranking's top.
        self.tv_follow();
    }

    pub fn apply_boards(&mut self, league: League, mut games: Vec<Game>, stale: bool) {
        let now = OffsetDateTime::now_utc();
        let prev_board = self.boards.get(&league).cloned().unwrap_or_default();
        // Score-change flash fires ONLY here — from data. A first sighting
        // (startup, new game) seeds last_scores without flashing.
        for g in &mut games {
            // Carry the accumulated scoring plays across the wholesale replace.
            if g.scoring_plays.is_empty() {
                if let Some(prev) = prev_board.iter().find(|p| p.id == g.id) {
                    g.scoring_plays = prev.scoring_plays.clone();
                }
            }
            // A cached payload is an OLDER snapshot, not news: its diff
            // against the last fresh scores is backwards and its lastPlay is
            // whatever was on screen then. Capturing that would write a bogus
            // scoring play that outlives the outage, so a stale apply is
            // scores-only — no flash, no capture, no last_scores rewrite.
            if stale {
                continue;
            }
            let score = (g.away_score, g.home_score);
            if let Some(prev) = self.last_scores.get(&g.id) {
                if *prev != score {
                    self.flashes.insert(g.id.clone(), self.tick);
                    // The scoreboard's lastPlay at the moment the score moved
                    // IS the scoring play (spec §1); dedupe on text.
                    if let Some(p) = g.last_plays.first() {
                        if !g.scoring_plays.iter().any(|s| s.text == p.text) {
                            let mut p = p.clone();
                            p.scoring = true;
                            g.scoring_plays.push(p.clone());
                            // The cut (spec §3): a newly captured scoring
                            // play IS the firing. Size is decided here, not
                            // in `CutState` — pinned/favorited/TV takes the
                            // screen, everything else is the quiet band.
                            if !self.cut_suppressed() {
                                let full = self.cut_is_full(g);
                                self.cuts.fire(&g.id, &p, full, self.tick);
                                if full {
                                    // Only a takeover rings; the band is
                                    // quiet by definition.
                                    self.bell_pending = true;
                                }
                            }
                        }
                    }
                }
            }
            self.last_scores.insert(g.id.clone(), score);
        }
        for pin in &mut self.pins {
            if pin.final_at.is_none()
                && games
                    .iter()
                    .any(|g| g.id == pin.game_id && g.status == Status::Final)
            {
                pin.final_at = Some(now);
            }
        }
        self.boards.insert(league, games);
        // Favorite-score alerts diff the freshly merged boards; a hit starts
        // the header banner and queues the bell for main to ring. A cached
        // payload is skipped whole — not checked and discarded: AlertState
        // diffs on inequality, so an older snapshot reads as a score change
        // (a banner and a bell for a score going backwards), and consuming
        // its delta would swallow the real one when the fresh board lands.
        if !stale {
            if let Some(alert) =
                self.alerts
                    .check(&self.config.favorites, &self.boards, self.tick)
            {
                self.active_alert = Some(alert);
                self.bell_pending = true;
            }
            // Fresh data landed: the live band may re-sort, if the data that
            // decides the order actually moved (`maybe_reorder`). This sits
            // inside the `!stale` guard on purpose — a cached payload is not
            // news and must never move the board.
            self.maybe_reorder();
            // …and TV lets go of anything that just left the live slate
            // (R36). After `maybe_reorder`, so the hero it re-anchors to is
            // this event's, not the last one's.
            self.tv_hygiene();
        }
        // Drop score memory for games no board carries any more: unbounded
        // growth over a days-long session, and a recycled id would flash on
        // first sighting instead of seeding silently.
        let mut last_scores = std::mem::take(&mut self.last_scores);
        last_scores.retain(|id, _| self.boards.values().flatten().any(|g| g.id == *id));
        self.last_scores = last_scores;
        let mut stats = std::mem::take(&mut self.stats);
        stats.retain(|id, _| self.boards.values().flatten().any(|g| g.id == *id));
        self.stats = stats;
        self.net.ok(Instant::now(), stale);
        self.pins = prune_pins(std::mem::take(&mut self.pins), now);
        self.persist_pins_quiet();
        self.clamp_selected();
    }

    pub fn merge_summary(&mut self, game_id: &str, summary: Summary) {
        if let Some(league) = self.league_of(game_id) {
            self.clear_aux_error(league, "summary");
        }
        // Non-football summaries carry no "drives", so they can map to zero
        // plays; keep the scoreboard's lastPlay instead of blanking the tile.
        if summary.last_plays.is_empty() && summary.scoring_plays.is_empty() {
            return;
        }
        // A summary lands only for the zoomed game, and it can carry a
        // scoring play the scoreboard never showed us. That is news exactly
        // once: the play whose text is new to `game.scoring_plays` fires a
        // cut, and the rest of the list is history being backfilled.
        let mut fresh: Option<(String, crate::domain::Play)> = None;
        for board in self.boards.values_mut() {
            if let Some(game) = board.iter_mut().find(|g| g.id == game_id) {
                let known: Vec<String> =
                    game.scoring_plays.iter().map(|p| p.text.clone()).collect();
                if !summary.scoring_plays.is_empty() {
                    // Summary order differs by source: football's
                    // `scoringPlays` is oldest-first, a list derived from the
                    // play-by-play is newest-first. Normalize to oldest-first
                    // by asking `last_plays` (newest-first) where the ends of
                    // the list sit — a smaller index means newer.
                    let mut sp = summary.scoring_plays.clone();
                    let newest_first = sp.len() > 1 && {
                        let pos = |t: &str| summary.last_plays.iter().position(|p| p.text == t);
                        match (pos(&sp[0].text), pos(&sp[sp.len() - 1].text)) {
                            (Some(a), Some(b)) => a < b,
                            // Nothing to compare against: ESPN's own
                            // `scoringPlays` is oldest-first already.
                            _ => false,
                        }
                    };
                    if newest_first {
                        sp.reverse();
                    }
                    game.scoring_plays = sp;
                }
                if !summary.last_plays.is_empty() {
                    let mut last_plays = summary.last_plays;
                    for play in &mut last_plays {
                        if summary.scoring_plays.iter().any(|s| s.text == play.text) {
                            play.scoring = true;
                        }
                    }
                    game.last_plays = last_plays;
                }
                // Newest new scoring play, if any. `known` is empty on the
                // very first summary for a game, and a whole game's scoring
                // history is not a cut — only an append to a list we already
                // had is.
                if !known.is_empty() {
                    fresh = game
                        .scoring_plays
                        .iter()
                        .rev()
                        .find(|p| !known.contains(&p.text))
                        .map(|p| (game.id.clone(), p.clone()));
                }
                break;
            }
        }
        if let Some((id, play)) = fresh {
            if !self.cut_suppressed() {
                let full = self
                    .game_by_id(&id)
                    .is_some_and(|g| self.cut_is_full(&g));
                self.cuts.fire(&id, &play, full, self.tick);
                if full {
                    self.bell_pending = true;
                }
            }
        }
        // No reorder here: a Summary carries nothing the rank fingerprint reads
        // (scores/status/meters all arrive via the scoreboard). Re-add when
        // sub-project 3 maps NHL power plays from the summary.
    }

    /// The zoomed game's (league, id) — the stats poll's only target. None
    /// unless the Zoom view is open and its game is still on a board.
    pub fn stats_target(&self) -> Option<(League, String)> {
        self.zoomed_game().map(|g| (g.league, g.id))
    }

    /// Latest box score for `game_id`, from the stats poll (or a fixture in
    /// tests/dump). Replaces wholesale — rows are a snapshot, not a delta.
    pub fn merge_stats(&mut self, game_id: &str, stats: GameStats) {
        if let Some(league) = self.league_of(game_id) {
            self.clear_aux_error(league, "stats");
        }
        self.stats.insert(game_id.to_string(), stats);
    }

    /// Record a failed scoreboard fetch: which league, the provider's short
    /// error (`ESPN 403 nfl scoreboard`), and how long until the scheduler
    /// retries. The chip and the board message read it through `net`.
    pub fn note_failure(&mut self, league: League, error: String, retry_in: Option<Duration>) {
        self.net.failed(Instant::now(), error.clone(), retry_in);
        // A populated board keeps its scores and the header chip says the
        // rest — a toast on top would nag. With nothing on the board, the
        // failure IS the news, so it also gets the footer line.
        if !self.boards.values().any(|b| !b.is_empty()) {
            self.status_line = Some(match retry_in {
                Some(d) => format!("{} · {} · retry in {}s", league.slug(), error, d.as_secs()),
                None => format!("{} · {error}", league.slug()),
            });
        }
    }

    /// The league the Standings view wants a table for — the on-demand
    /// standings fetch's only target. None unless the view is open.
    pub fn standings_target(&self) -> Option<League> {
        match self.view {
            View::Standings(league) => Some(league),
            _ => None,
        }
    }

    /// Latest standings for one league, from the on-demand fetch (or a
    /// fixture in tests/dump). Replaces wholesale — a table is a snapshot.
    /// Stamped with the moment we took it: a table the feed doesn't label
    /// with a season is labeled with its own age instead, so it never reads
    /// as live when it isn't.
    /// How old the last fresh board may get before the header stops claiming
    /// the numbers are live — derived from the cadence actually in use, so
    /// the chip can never contradict the scheduler. Live: 3 × the 15 s live
    /// cadence (one missed poll is noise, three in a row is a problem). Idle:
    /// one 60 s cadence plus one live window, because at a minute between
    /// polls a 45 s cutoff would call every healthy board stale.
    pub fn stale_after(&self) -> Duration {
        if self.any_live() {
            3 * crate::poll::SCOREBOARD_LIVE
        } else {
            crate::poll::SCOREBOARD_IDLE + crate::poll::SCOREBOARD_LIVE
        }
    }

    pub fn merge_standings(&mut self, mut table: StandingsTable) {
        table.fetched_at = Some(self.now());
        self.clear_aux_error(table.league, "standings");
        self.standings.insert(table.league, table);
    }

    /// An on-demand fetch failed. `what` names the request kind, so the view
    /// that asked can say which fetch is missing rather than showing an empty
    /// pane that reads like "no data exists".
    pub fn note_aux_failure(&mut self, league: League, what: &'static str, error: String) {
        self.aux_errors.insert((league, what), error);
    }

    /// The matching success: the error stops being true the moment data lands.
    pub fn clear_aux_error(&mut self, league: League, what: &'static str) {
        self.aux_errors.remove(&(league, what));
    }

    pub fn aux_error(&self, league: League, what: &'static str) -> Option<&str> {
        self.aux_errors.get(&(league, what)).map(|s| s.as_str())
    }

    /// Which board carries `game_id` — the zoom-driven fetches (summary,
    /// stats) are addressed by game id, and `aux_errors` is keyed by league.
    fn league_of(&self, game_id: &str) -> Option<League> {
        self.boards
            .iter()
            .find(|(_, games)| games.iter().any(|g| g.id == game_id))
            .map(|(league, _)| *league)
    }

    fn clamp_selected(&mut self) {
        let n = self.selection_len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    fn cycle_tab(&mut self, delta: isize) {
        let tabs = self.tab_list();
        if tabs.is_empty() {
            return;
        }
        let n = tabs.len() as isize;
        let next = (self.current_tab_index() as isize + delta).rem_euclid(n) as usize;
        self.set_tab(tabs[next]);
    }

    /// Direct tab jump (`:nfl`, `:home`): same reset a cycled switch does.
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.selected = 0;
        self.view = View::Board;
    }

    /// j/k: walk `Derived::selection` — one list, hero included, wrapping at
    /// both ends. The board scrolls itself to keep the selection visible
    /// (`board::first_visible`), so nothing here has a window to move.
    fn move_selected(&mut self, delta: isize) {
        let n = self.selection_len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
    }

    fn toggle_pin(&mut self) {
        let Some(game) = self.selected_game() else {
            return;
        };
        let matchup = format!("{}@{}", game.away.abbr, game.home.abbr);
        if let Some(idx) = self.pins.iter().position(|p| p.game_id == game.id) {
            self.pins.remove(idx);
            self.status_line = Some(format!("unpinned {matchup}"));
        } else {
            self.pins.push(Pin {
                game_id: game.id,
                league: game.league,
                final_at: None,
            });
            self.status_line = Some(format!("pinned {matchup}"));
        }
        self.persist_pins();
        // Membership in MY GAMES changed, so the game just entered or left
        // the ranked list: re-rank now. Without this the released game
        // re-enters `live_all` and `OrderState::ordered`'s append-unseen
        // fallback parks it *last* in IN PLAY until the next fresh apply
        // changes a fingerprint — the board reading as if it punished the
        // unpin. A user keystroke is an event under spec §2's gate, same as
        // `s` or `:sort`, so R24 is untouched.
        self.force_reorder();
        self.clamp_selected();
    }

    fn toggle_favorite(&mut self) {
        let Some(game) = self.selected_game() else {
            return;
        };
        let abbr = game.home.abbr;
        if let Some(idx) = self
            .config
            .favorites
            .iter()
            .position(|f| f.league == game.league && f.team_abbr.eq_ignore_ascii_case(&abbr))
        {
            self.config.favorites.remove(idx);
            self.status_line =
                Some(format!("unfavorited {} {abbr}", game.league.slug().to_uppercase()));
        } else {
            self.status_line =
                Some(format!("favorited {} {abbr}", game.league.slug().to_uppercase()));
            self.config.favorites.push(Favorite {
                league: game.league,
                team_abbr: abbr,
            });
        }
        self.persist_config();
        self.force_reorder(); // membership change — see `toggle_pin`
        self.clamp_selected();
    }

    /// 'c': step to the next loaded theme in picker order (built-ins, then
    /// user files), wrapping; persisted like layout.
    fn cycle_theme(&mut self) {
        let next = theme::next_name(&theme::current_name(), 1);
        let _ = theme::set_current(&next);
        self.config.theme = next.clone();
        // One keypress, one line. `persist_config` writes its own refusal
        // toast, and the old order let "theme X" overwrite it — so the toast
        // is composed here, after the save, and says both halves.
        self.status_line = None;
        self.persist_config();
        let save_error = self.status_line.take();
        self.status_line = Some(match (&self.config_error, save_error) {
            (Some(_), _) => format!("theme {next} · not saving (config error)"),
            (None, Some(err)) => err,
            (None, None) => format!("theme {next}"),
        });
    }

    /// 's': cycle WATCH → TIME → LEAGUE → WATCH and re-derive the order right
    /// away — a sort-key change is an event, not something the next score
    /// tick should gate (spec §9).
    fn cycle_sort(&mut self) {
        let next = self.config.sort.cycled();
        self.config.sort = next;
        self.force_reorder();
        self.status_line = None;
        self.persist_config();
        let save_error = self.status_line.take();
        self.status_line = Some(match (&self.config_error, save_error) {
            (Some(_), _) => format!(
                "sort {} · not saving (config error)",
                next.label().to_ascii_lowercase()
            ),
            (None, Some(err)) => err,
            (None, None) => format!("sort {}", next.label().to_ascii_lowercase()),
        });
    }

    /// One frame. The game lists are derived ONCE here and parked in
    /// `frame_cache`; every widget below reads them through `derived()`
    /// instead of re-walking the boards. The cache is dropped again on the
    /// way out — a list read outside a draw is a stale list, so `derived()`
    /// panics there rather than answering.
    pub fn draw(&mut self, frame: &mut Frame) {
        self.frame_cache = Some(self.derive());
        self.draw_frame(frame);
        self.frame_cache = None;
    }

    fn draw_frame(&mut self, frame: &mut Frame) {
        // Mouse zones are rebuilt from scratch every frame: whatever this
        // draw doesn't register is not clickable.
        self.hit_zones.clear();
        let th = theme::current();
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(th.bg).fg(th.fg)),
            area,
        );
        if area.width < 40 || area.height < 12 {
            // Walter's rule: a limit someone can hit must name the actual and
            // expected values — "need more columns" didn't say how many, or
            // whether it was rows that were short.
            frame.render_widget(
                Paragraph::new(format!(
                    "need 40×12, have {}×{}",
                    area.width, area.height
                ))
                .style(Style::default().fg(th.muted).bg(th.bg)),
                area,
            );
            return;
        }
        // v3.2 §1: the Board has no separate ticker rows at all — it draws
        // its own one-row SCORES lane inline, inside the body, only when
        // something didn't fit (one lane, one owner; see `board::mod`'s
        // `draw_lane`). Every other view gets the same off-screen lane at
        // the bottom of the frame, gated by the SAME truncation the Board
        // would show at this size: `layout::plan(...).scores_lane`, run
        // against the current tab's counts.
        // TV is on the same footing as the Board here for the same reason:
        // it draws its own bottom strip of everything else that is live, and
        // a SCORES lane under that would be two lanes saying one thing.
        let ticker_h = if matches!(self.view, View::Board | View::ThemePicker | View::Tv) {
            0
        } else {
            let d = self.derived();
            let hero_in_band = d.my_games.iter().any(|g| Some(&g.id) == d.hero_id.as_ref());
            let band_rows = d.my_games.len() - usize::from(hero_in_band);
            let body_h = area.height.saturating_sub(2); // header + footer
            let plan = crate::board::layout::plan(
                area.width,
                body_h,
                d.in_play.len(),
                d.finals.len(),
                d.later.len(),
                band_rows,
            );
            if plan.scores_lane {
                ticker::LANE_HEIGHT
            } else {
                0
            }
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(ticker_h),
                Constraint::Length(1),
            ])
            .split(area);
        self.draw_header(frame, chunks[0]);
        // The cut (spec §3). The takeover owns everything under the header —
        // the board is not drawn behind it at all — and the band is two rows
        // inserted above whatever the view was going to draw.
        let cut = self.cuts.active(self.tick).cloned();
        if let Some(cut) = cut.filter(|c| c.full) {
            if let Some(game) = self.game_by_id(&cut.game_id) {
                let below = Rect {
                    y: area.y + 1,
                    height: area.height - 1,
                    ..area
                };
                crate::board::cut::draw_takeover(frame, below, &game, &cut, self.tick);
                return;
            }
        }
        let mut body = chunks[1];
        // `!c.full` is explicit rather than implied by the early return above:
        // a full cut whose game left the board falls through to here, and a
        // takeover must never degrade into a band.
        if let Some(cut) = self.cuts.active(self.tick).filter(|c| !c.full).cloned() {
            if let Some(game) = self.game_by_id(&cut.game_id) {
                if body.height > crate::board::cut::BAND_ROWS {
                    let band = Rect { height: crate::board::cut::BAND_ROWS, ..body };
                    body = Rect {
                        y: band.bottom(),
                        height: body.height - crate::board::cut::BAND_ROWS,
                        ..body
                    };
                    crate::board::cut::draw_band(frame, band, &game, &cut, self.tick);
                }
            }
        }
        views::draw(self, frame, body);
        if ticker_h > 0 {
            self.draw_ticker(frame, chunks[2]);
        }
        self.draw_footer(frame, chunks[3]);
        if self.help_open {
            self.draw_help(frame, area);
        }
    }

    /// Color for a team abbr on a play/event row: the team's color when the
    /// theme's discipline allows color on play text, else `fg`.
    pub(crate) fn team_color(game: &Game, abbr: &str) -> ratatui::style::Color {
        let th = theme::current();
        if game.away.abbr.eq_ignore_ascii_case(abbr) {
            th.team_text(game.away.color)
        } else if game.home.abbr.eq_ignore_ascii_case(abbr) {
            th.team_text(game.home.color)
        } else {
            th.fg
        }
    }

    /// Selection indices shift whenever the filter narrows the lists; callers
    /// that change the filter re-clamp through here.
    pub(crate) fn filter_changed(&mut self) {
        self.clamp_selected();
    }

    /// The game the Zoom view is showing, looked up across every board so a
    /// zoom opened from Home survives regardless of the tab it came from.
    pub(crate) fn zoomed_game(&self) -> Option<Game> {
        let View::Zoom { game_id, .. } = &self.view else {
            return None;
        };
        self.game_by_id(game_id)
    }

    pub(crate) fn game_by_id(&self, id: &str) -> Option<Game> {
        // Dated slates included so zooming a traveled game isn't a dead view.
        self.boards
            .values()
            .flatten()
            .chain(self.dated_boards.values().flatten())
            .find(|g| g.id == id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Favorite, Pin};
    use crate::domain::*;
    use crate::rank::SortKey;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn team(abbr: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            color: [1, 2, 3],
            logo_key: format!("nfl/{}", abbr.to_lowercase()),
            ..Default::default()
        }
    }

    pub(crate) fn g(id: &str, away: &str, home: &str, live: bool) -> Game {
        Game {
            id: id.into(),
            league: League::Nfl,
            away: team(away),
            home: team(home),
            away_score: 7,
            home_score: 3,
            status: if live { Status::Live } else { Status::Pre },
            period: "Q2".into(),
            clock: "5:00".into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            ..Game::default()
        }
    }

    pub(crate) fn app_with(games: Vec<Game>, pins: Vec<Pin>) -> App {
        let dir = std::env::temp_dir().join(format!("gd-app-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), pins, dir, time::UtcOffset::UTC);
        app.apply_boards(League::Nfl, games, false);
        app
    }

    #[test]
    fn home_shows_pinned_first_then_live() {
        let app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
            vec![Pin {
                game_id: "2".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        let ids: Vec<_> = app.visible_games().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["2", "1"]);
    }

    #[test]
    fn nfl_tab_shows_all_league_games() {
        let mut app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", false)],
            vec![],
        );
        app.tab = Tab::League(League::Nfl);
        assert_eq!(app.visible_games().len(), 2);
        // v3.2 §1 retired live_games/slate_games: the board's sections come
        // out of derive() now, and a league tab is the same board filtered.
        let d = app.derive();
        assert_eq!(d.in_play.len(), 1);
        assert_eq!(d.later.len(), 1);
    }

    #[test]
    fn filter_matches_abbr_location_and_name_case_insensitively() {
        let mut chiefs = team("KC");
        chiefs.location = "KANSAS CITY".into();
        chiefs.name = "Chiefs".into();
        let mut game = g("1", "KC", "TB", true);
        game.away = chiefs;
        let mut app = app_with(vec![game, g("2", "DAL", "PHI", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        for needle in ["kc", "kansas", "chiefs", "CHIEFS", "tb"] {
            app.filter = Some(needle.into());
            let ids: Vec<_> = app.visible_games().into_iter().map(|x| x.id).collect();
            assert_eq!(ids, vec!["1"], "needle {needle:?}");
        }
        app.filter = Some("phi".into());
        let ids: Vec<_> = app.visible_games().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["2"]);
    }

    #[test]
    fn filter_narrows_selection_and_esc_restores() {
        let mut app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
            vec![],
        );
        app.tab = Tab::League(League::Nfl);
        app.selected = 1;
        app.filter = Some("kc".into());
        app.filter_changed();
        assert_eq!(app.selected, 0, "selection clamps to the narrowed list");
        // v3.2 §1 retired live_games(): the filtered live list is in_play.
        assert_eq!(app.derive().in_play.len(), 1);
        // Esc in Normal mode clears the committed filter.
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.filter, None);
        assert_eq!(app.derive().in_play.len(), 2);
    }

    /// A broken config turns saving off, but the poll loop must not keep
    /// saying so: the prune inside `apply_boards` runs every merge and would
    /// stomp whatever the user's last key put in the footer. Only a key the
    /// user pressed re-arms the message.
    #[test]
    fn a_broken_config_silences_the_prune_but_still_answers_a_keypress() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.set_config_error(Some("config.toml:1: unknown variant `NFLL`".into()));
        app.status_line = Some("filter cleared".into());
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        assert_eq!(
            app.status_line.as_deref(),
            Some("filter cleared"),
            "the background prune must not toast"
        );
        // Space is the user asking for a save; that one has to answer.
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(
            app.status_line.as_deref().unwrap_or("").contains("not saving"),
            "{:?}",
            app.status_line
        );
    }

    /// A failed standings fetch has no header chip of its own, so the view
    /// that asked for it has to say so — an empty table otherwise reads as
    /// "ESPN has no standings for this league".
    #[test]
    fn a_failed_standings_fetch_shows_the_error_where_the_table_would_be() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.view = View::Standings(League::Cfb);
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        let screen = |t: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
                      app: &mut App| {
            t.draw(|f| app.draw(f)).unwrap();
            t.backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
        };
        assert!(
            !screen(&mut term, &mut app).contains("standings unavailable"),
            "nothing has failed yet"
        );
        app.note_aux_failure(League::Cfb, "standings", "ESPN 503 cfb standings".into());
        let s = screen(&mut term, &mut app);
        assert!(
            s.contains("standings unavailable") && s.contains("503") && s.contains("retrying"),
            "the empty state names the failure and promises the retry: {s:?}"
        );
        // The next success is the error's end.
        app.merge_standings(crate::domain::StandingsTable {
            league: League::Cfb,
            season: None,
            groups: vec![],
            fetched_at: None,
        });
        assert_eq!(app.aux_error(League::Cfb, "standings"), None);
    }

    /// One keypress writes one status line: `t` under a broken config used to
    /// toast "not saving: …" and then immediately overwrite it with "theme X",
    /// so the refusal never reached the user's eye.
    #[test]
    fn cycling_the_theme_with_a_broken_config_says_both_halves_in_one_line() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        let healthy = app.status_line.clone().unwrap_or_default();
        assert!(healthy.starts_with("theme ") && !healthy.contains("not saving"), "{healthy}");
        app.set_config_error(Some("config.toml:7: unknown variant `NFLL`".into()));
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        let line = app.status_line.clone().unwrap_or_default();
        assert!(
            line.starts_with("theme ") && line.ends_with("· not saving (config error)"),
            "one line, both halves: {line:?}"
        );
    }

    /// The stderr note is gone the instant the alternate screen opens, so the
    /// parse error has to be on the board itself.
    #[test]
    fn the_parse_error_is_in_the_footer_at_startup() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.set_config_error(Some("config.toml:7: unknown variant `NFLL`".into()));
        assert_eq!(
            app.config_error.as_deref(),
            Some("config.toml:7: unknown variant `NFLL`")
        );
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        let screen: String = (0..30)
            .map(|y| {
                (0..120)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("config.toml:7") && screen.contains("not saving until fixed"),
            "{screen}"
        );
    }

    #[test]
    fn space_toggles_pin() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.pins.len(), 1);
        assert_eq!(app.pins[0].game_id, "1");
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.pins.is_empty());
    }

    #[test]
    fn empty_summary_keeps_scoreboard_plays() {
        let mut game = g("1", "KC", "TB", true);
        game.last_plays = vec![crate::domain::Play {
            clock: "1:27".into(),
            team: "KC".into(),
            text: "from scoreboard".into(),
            ..Default::default()
        }];
        let mut app = app_with(vec![game], vec![]);
        // MLB/NBA summaries have no drives => zero mapped plays; don't blank the tile.
        app.merge_summary("1", crate::domain::Summary::default());
        let board = &app.boards[&League::Nfl];
        assert_eq!(board[0].last_plays[0].text, "from scoreboard");
        // A real summary still replaces them.
        let s = crate::domain::Summary {
            last_plays: vec![crate::domain::Play {
                clock: "0:55".into(),
                team: "TB".into(),
                text: "from summary".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        app.merge_summary("1", s);
        assert_eq!(app.boards[&League::Nfl][0].last_plays[0].text, "from summary");
    }

    #[test]
    fn q_quits() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);
    }

    #[test]
    fn tab_cycles_home_then_nfl() {
        let mut app = app_with(vec![], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        assert_eq!(app.tab, Tab::Home);
        app.on_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::League(League::Nfl));
        app.on_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home);
    }

    #[test]
    fn t_favorites_home_team() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(
            app.config.favorites,
            vec![Favorite {
                league: League::Nfl,
                team_abbr: "TB".into()
            }]
        );
        app.on_key(KeyCode::Char('t'), KeyModifiers::NONE);
        assert!(app.config.favorites.is_empty());
    }

    #[test]
    fn unpinning_re_ranks_at_once_instead_of_parking_the_game_last() {
        // The best game on the board, pinned and then released. Without a
        // re-rank on the membership change, `OrderState::ordered`'s
        // append-unseen fallback puts it *last* in IN PLAY until the next
        // fresh apply changes a fingerprint — the board reading as if the
        // unpin demoted it.
        let mut best = g("2", "DAL", "PHI", true); // tied, late: most watchable
        best.period = "Q4".into();
        best.clock = "0:45".into();
        best.away_score = 21;
        best.home_score = 21;
        let games = vec![g("1", "KC", "TB", true), best];
        let mut app = app_with(games.clone(), vec![]);
        app.tab = Tab::League(League::Nfl);
        let ids = |app: &App| -> Vec<String> {
            app.derive().in_play.iter().map(|x| x.id.clone()).collect()
        };
        assert_eq!(
            ids(&app),
            vec!["2", "1"],
            "the tied Q4 game leads IN PLAY to begin with"
        );

        // Select it (it is the hero, index 0) and pin, then unpin.
        app.selected = 0;
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.derive().my_games.len(), 1, "pinned into MY GAMES");
        assert_eq!(ids(&app), vec!["1"]);
        // One poll lands while it is pinned: the ranked order now has no
        // memory of it at all.
        app.apply_boards(League::Nfl, games, false);
        assert_eq!(ids(&app), vec!["1"]);
        app.selected = 0;
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.pins.is_empty(), "unpinned");
        assert_eq!(
            ids(&app),
            vec!["2", "1"],
            "released to its own rank, not to the bottom of the list"
        );
    }

    #[test]
    fn enter_zooms_and_esc_pops() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.view,
            View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview }
        );
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        // 'z' aliases Enter, and 'z' inside the zoom restores the board.
        app.on_key(KeyCode::Char('z'), KeyModifiers::NONE);
        assert!(matches!(app.view, View::Zoom { .. }));
        app.on_key(KeyCode::Char('z'), KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn tab_pops_zoom_and_home_carries_the_live_game() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.view, View::Zoom { .. }));
        app.on_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home);
        assert_eq!(app.view, View::Board);
        let ids: Vec<_> = app.visible_games().into_iter().map(|g| g.id).collect();
        // Home is every live game now, pinned or not.
        assert_eq!(ids, vec!["1"]);
    }

    #[test]
    fn zoom_tabs_cycle_with_hl_and_brackets_and_jk_clamp() {
        let mut game = g("1", "KC", "TB", true);
        game.last_plays = vec![
            Play { clock: "1:00".into(), team: "KC".into(), text: "a".into(), ..Default::default() },
            Play { clock: "2:00".into(), team: "TB".into(), text: "b".into(), ..Default::default() },
        ];
        let mut app = app_with(vec![game], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char('z'), KeyModifiers::NONE);
        let tab_of = |app: &App| match &app.view {
            View::Zoom { tab, .. } => *tab,
            other => panic!("expected zoom, got {other:?}"),
        };
        app.on_key(KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(tab_of(&app), ZoomTab::Plays);
        app.on_key(KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(tab_of(&app), ZoomTab::Stats);
        app.on_key(KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(tab_of(&app), ZoomTab::Overview, "wraps forward");
        app.on_key(KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(tab_of(&app), ZoomTab::Stats, "h/[ wrap backward");
        app.on_key(KeyCode::Char('['), KeyModifiers::NONE);
        assert_eq!(tab_of(&app), ZoomTab::Plays);
        // j/k clamp to the feed: 2 plays => indices 0..=1, never past.
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.zoom_scroll, 1, "clamped at last play");
        app.on_key(KeyCode::Char('k'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.zoom_scroll, 0, "clamped at first play");
        // Switching tabs resets the highlight.
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(app.zoom_scroll, 0);
    }

    #[test]
    fn placeholder_views_pop_with_esc_or_q() {
        for key in [KeyCode::Esc, KeyCode::Char('q')] {
            let mut app = app_with(vec![], vec![]);
            app.view = View::PlaysFeed;
            app.on_key(key, KeyModifiers::NONE);
            assert_eq!(app.view, View::Board, "{key:?} pops the placeholder");
            assert!(!app.should_quit, "{key:?} must not quit outside Board");
        }
    }

    #[test]
    fn c_cycles_theme_and_persists() {
        use crate::theme;
        theme::set_current("broadcast").unwrap();
        // Own dir: app_with's shared dir is also written by other tests' saves.
        let dir = std::env::temp_dir().join(format!("gd-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "studio");
        assert_eq!(app.config.theme, "studio");
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.theme, "studio");
        // The whole loaded set cycles back to the start.
        for _ in 1..theme::names().len() {
            app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        }
        assert_eq!(theme::current_name(), "broadcast");
        assert_eq!(app.config.theme, "broadcast");
    }

    #[test]
    fn theme_picker_wheel_previews_and_tab_reverts_before_switching() {
        use crate::theme;
        theme::set_current("broadcast").unwrap();
        let mut app = app_with(vec![], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        app.open_theme_picker();
        app.on_hit(keymap::Hit::ScrollDown);
        assert_eq!(theme::current_name(), "studio", "wheel previews like j");
        app.on_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board, "Tab pops the picker onto the board");
        assert_eq!(app.tab, Tab::League(League::Nfl));
        assert_eq!(theme::current_name(), "broadcast", "a tab switch never commits a preview");
    }

    #[test]
    fn theme_picker_tab_chip_click_reverts_and_reopen_keeps_the_prior() {
        use crate::theme;
        theme::set_current("broadcast").unwrap();
        let mut app = app_with(vec![], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        app.open_theme_picker();
        app.on_hit(keymap::Hit::ScrollDown);
        assert_eq!(theme::current_name(), "studio");
        // Opening again while open must not adopt the preview as "prior".
        app.open_theme_picker();
        assert_eq!(app.theme_prior, "broadcast", "reopen keeps the real prior theme");
        assert_eq!(theme::current_name(), "studio", "reopen leaves the preview in place");
        // A header tab click pops the picker like the Tab key: revert first.
        app.on_hit(keymap::Hit::TabChip(Tab::League(League::Nfl)));
        assert_eq!(app.view, View::Board);
        assert_eq!(app.tab, Tab::League(League::Nfl));
        assert_eq!(theme::current_name(), "broadcast", "a tab click never commits a preview");
        assert_eq!(app.config.theme, "broadcast");
    }

    // Task 10 (spec §9): 's' cycles the sort key and re-derives the order
    // immediately (not gated on the next score event), and the header's
    // sort chip reads it straight from config so it follows without a
    // second wire-up.
    #[test]
    fn s_cycles_the_sort_and_persists_and_the_header_follows() {
        let mut app = app_with(six_live(), vec![]);
        assert_eq!(app.config.sort, SortKey::Watch);
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(app.config.sort, SortKey::Time);
        assert!(app.status_line.as_deref().unwrap_or("").contains("sort time"));
    }

    #[test]
    fn v_enters_tv_and_esc_leaves() {
        let mut app = app_with(six_live(), vec![]);
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        assert!(matches!(app.view, View::Tv));
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn tv_auto_cuts_on_event_not_on_timer_and_lock_holds() {
        // Spec §3: TV switches to the ranking's top on the next EVENT, never
        // on a timer. The fingerprint gate (R24) is what makes that true —
        // a clock that merely advanced is not news, so nothing switches.
        let mut app = app_with(
            vec![
                ranked("a", "Q3", "10:00", 20, 17),
                ranked("b", "Q1", "15:00", 24, 21),
                ranked("c", "Q1", "15:00", 30, 10),
            ],
            vec![],
        );
        assert_eq!(ord(&app), vec!["a", "b", "c"], "the late close game leads");
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        assert_eq!(app.tv_shown.as_deref(), Some("a"), ":tv opens on the hero");

        // "b" walks to Q4 with the same score: watchability rises past "a",
        // but no score, status or hot flag moved, so it is not an event.
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "10:00", 20, 17),
                ranked("b", "Q4", "10:00", 24, 21),
                ranked("c", "Q1", "15:00", 30, 10),
            ],
            false,
        );
        assert_eq!(app.tv_shown.as_deref(), Some("a"), "no event, no switch");
        assert_eq!(
            app.tv_next_cut_in(&app.derive()).map(|g| g.id),
            Some("b".to_string()),
            "the screen says who is next instead of switching"
        );

        // A real event anywhere on the board (c scores) re-derives the order,
        // and TV cuts to its top.
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "10:00", 20, 17),
                ranked("b", "Q4", "10:00", 24, 21),
                ranked("c", "Q1", "15:00", 31, 10),
            ],
            false,
        );
        assert_eq!(app.tv_shown.as_deref(), Some("b"), "the event cut to b");
        assert_eq!(
            app.tv_next_cut_in(&app.derive()).map(|g| g.id),
            None,
            "b IS the top now"
        );

        // space locks the shown game: a later event that puts "c" on top
        // leaves the screen where it is.
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.tv_lock.as_deref(), Some("b"), "space locks b");
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "10:00", 20, 17),
                ranked("b", "Q4", "10:00", 24, 21),
                ranked("c", "Q4", "5:00", 31, 31),
            ],
            false,
        );
        assert_eq!(ord(&app), vec!["c", "b", "a"], "the event did reorder");
        assert_eq!(app.tv_shown.as_deref(), Some("b"), "a locked game holds");
        // …and space again releases it.
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.tv_lock, None, "space unlocks");
    }

    /// A favorited KC game that ranks LAST, and a stranger game that ranks
    /// first — the pair ruling R35 is about.
    fn my_game_and_a_better_one() -> App {
        let mut app = app_with(
            vec![
                ranked("mine", "Q1", "15:00", 3, 0),
                {
                    let mut other = ranked("other", "Q4", "5:00", 24, 21);
                    other.away = team("DAL");
                    other.home = team("PHI");
                    other
                },
            ],
            vec![],
        );
        app.config.favorites.push(Favorite {
            league: League::Nfl,
            team_abbr: "KC".into(),
        });
        app
    }

    #[test]
    fn tv_follows_the_hero_rule_and_never_cuts_away_from_my_game() {
        // Ruling R35: TV follows `Derived::hero_id` — MY GAMES' top while it
        // is live, else the ranking's top. Following `OrderState`'s top
        // instead (which excludes MY GAMES by design) meant the first event
        // cut away from your own team and could never cut back.
        let mut app = my_game_and_a_better_one();
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        assert_eq!(app.tv_shown.as_deref(), Some("mine"), ":tv opens on my game");

        // "other" outranks it by a mile, and an event lands. The screen holds.
        let mut better = ranked("other", "Q4", "5:00", 24, 24);
        better.away = team("DAL");
        better.home = team("PHI");
        app.apply_boards(
            League::Nfl,
            vec![ranked("mine", "Q1", "15:00", 3, 0), better.clone()],
            false,
        );
        assert_eq!(
            app.tv_shown.as_deref(),
            Some("mine"),
            "an event must never cut away from a live MY GAMES top"
        );
        assert_eq!(
            app.tv_next_cut_in(&app.derive()).map(|g| g.id),
            None,
            "and nothing is advertised as next: my game IS the rule's top"
        );

        // My game goes final: now the ranking's top takes the screen.
        let mut done = ranked("mine", "Q4", "0:00", 3, 0);
        done.status = Status::Final;
        app.apply_boards(League::Nfl, vec![done, better], false);
        assert_eq!(
            app.tv_shown.as_deref(),
            Some("other"),
            "a final MY GAMES top hands the screen to the ranking"
        );
    }

    #[test]
    fn both_tv_entries_clear_a_stale_lock() {
        let mut app = my_game_and_a_better_one();
        let lock_then_leave = |app: &mut App| {
            app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
            assert!(app.tv_lock.is_some(), "space locks");
            app.on_key(KeyCode::Esc, KeyModifiers::NONE);
            assert_eq!(app.view, View::Board);
        };
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        lock_then_leave(&mut app);
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        assert_eq!(app.tv_lock, None, "v reopens unlocked");

        // The command form is the same door: `:tv` used to set the view
        // only, so TV reopened still locked on a game from the last visit.
        lock_then_leave(&mut app);
        for c in ":tv".chars() {
            crate::input::handle_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        crate::input::handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.view, View::Tv), ":tv opens TV");
        assert_eq!(app.tv_lock, None, ":tv reopens unlocked");
    }

    #[test]
    fn a_lock_on_a_game_that_leaves_the_slate_releases_itself() {
        // Ruling R36: a lock is one slate's worth of intent. When its game
        // goes final the lock would otherwise hold a dead id — auto-cut off,
        // footer still offering `space unlock`, screen stuck on a game that
        // is not live.
        let mut app = my_game_and_a_better_one();
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.tv_lock.as_deref(), Some("mine"));

        let mut done = ranked("mine", "Q4", "0:00", 3, 0);
        done.status = Status::Final;
        let mut better = ranked("other", "Q4", "5:00", 24, 21);
        better.away = team("DAL");
        better.home = team("PHI");
        app.apply_boards(League::Nfl, vec![done, better], false);
        assert_eq!(app.tv_lock, None, "the lock released with its game");
        assert_eq!(
            app.tv_shown.as_deref(),
            Some("other"),
            "and the hero rule took over in the same event"
        );
    }

    #[test]
    fn n_walks_the_tv_slate_by_hand_and_wraps() {
        let mut app = app_with(
            vec![
                ranked("a", "Q3", "10:00", 20, 17),
                ranked("b", "Q1", "15:00", 24, 21),
                ranked("c", "Q1", "15:00", 30, 10),
            ],
            vec![],
        );
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        for want in ["b", "c", "a"] {
            app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
            assert_eq!(app.tv_shown.as_deref(), Some(want), "n walks the slate");
        }
    }

    /// A live NFL game with an explicit clock and score — the ordering tests
    /// need all four, and `g` fixes them.
    fn ranked(id: &str, period: &str, clock: &str, away: u16, home: u16) -> Game {
        let mut x = g(id, "KC", "TB", true);
        x.period = period.into();
        x.clock = clock.into();
        x.away_score = away;
        x.home_score = home;
        x
    }

    /// The live band in display order.
    fn ord(app: &App) -> Vec<String> {
        app.order
            .ordered(&app.live_all())
            .iter()
            .map(|x| x.id.clone())
            .collect()
    }

    /// Two live games: "a" is early (ranks low), "b" is later and close.
    fn ordering_app() -> App {
        let app = app_with(
            vec![
                ranked("a", "Q1", "15:00", 14, 10),
                ranked("b", "Q2", "5:00", 24, 21),
            ],
            vec![],
        );
        assert_eq!(ord(&app), vec!["b", "a"], "the close later game leads");
        app
    }

    #[test]
    fn a_clock_that_merely_advanced_never_reorders_the_board() {
        // Spec §2's headline: between events the order is frozen even though
        // watchability keeps rising. "a" moving Q1 -> Q3 outranks "b" on
        // score, but nothing about the DATA changed, so the board holds.
        let mut app = ordering_app();
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "5:00", 14, 10),
                ranked("b", "Q2", "5:00", 24, 21),
            ],
            false,
        );
        assert_eq!(
            ord(&app),
            vec!["b", "a"],
            "clock drift alone must not reorder"
        );
    }

    #[test]
    fn a_score_delta_reorders_the_board() {
        let mut app = ordering_app();
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "5:00", 14, 14), // tied: now the better watch
                ranked("b", "Q2", "5:00", 24, 21),
            ],
            false,
        );
        assert_eq!(ord(&app), vec!["a", "b"], "a score change is an event");
    }

    #[test]
    fn a_hot_flip_reorders_the_board_with_no_score_change() {
        // Red zone appears on "a" — same score, same clock, but the game is
        // hot now, and that is news the order has to answer to.
        let mut app = ordering_app();
        let mut a = ranked("a", "Q1", "15:00", 14, 10);
        a.meter = Some(crate::domain::Meter::RedZone { yards_to_goal: 6 });
        app.apply_boards(
            League::Nfl,
            vec![a, ranked("b", "Q2", "5:00", 24, 21)],
            false,
        );
        assert_eq!(ord(&app), vec!["a", "b"], "a hot flip is an event");
    }

    #[test]
    fn a_cached_apply_never_reorders_the_board() {
        let mut app = ordering_app();
        app.apply_boards(
            League::Nfl,
            vec![
                ranked("a", "Q3", "5:00", 14, 14),
                ranked("b", "Q2", "5:00", 24, 21),
            ],
            true, // cached: an older snapshot, not news
        );
        assert_eq!(
            ord(&app),
            vec!["b", "a"],
            "a stale payload must not move the board"
        );
    }

    #[test]
    fn a_pinned_game_is_not_in_the_ordered_live_band() {
        // Pins live in the MY GAMES band and never re-sort (spec §1), so
        // OrderState is never told about them.
        let app = app_with(
            vec![
                ranked("a", "Q1", "15:00", 14, 10),
                ranked("b", "Q2", "5:00", 24, 21),
            ],
            vec![Pin {
                game_id: "b".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        assert_eq!(ord(&app), vec!["a"], "the pinned game is not ordered here");
    }

    #[test]
    fn a_summary_never_reorders_the_board() {
        // A Summary carries only plays — no score, status or meter — so it can
        // never move the rank fingerprint. Scoring plays landing on a game
        // leave the order exactly where the last scoreboard apply put it. The
        // real event coverage lives in the apply_boards tests above
        // (clock-drift freeze, score delta, hot flip).
        let mut app = ordering_app();
        let before = ord(&app);
        let score = |text: &str| Summary {
            last_plays: vec![],
            scoring_plays: vec![Play {
                text: text.into(),
                team: "KC".into(),
                scoring: true,
                ..Default::default()
            }],
            meter: None,
        };
        app.merge_summary("a", score("Mahomes 20 yd TD pass"));
        assert_eq!(ord(&app), before, "a summary is not a reorder event");
        app.merge_summary("a", score("Kelce 8 yd TD pass"));
        assert_eq!(ord(&app), before, "nor is a second one");
        assert_eq!(
            app.boards[&League::Nfl]
                .iter()
                .find(|x| x.id == "a")
                .unwrap()
                .scoring_plays
                .len(),
            1,
            "the summary still did its own job"
        );
    }

    #[test]
    fn apply_boards_stamps_final_at() {
        let mut app = app_with(
            vec![g("1", "KC", "TB", true)],
            vec![Pin {
                game_id: "1".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        let mut done = g("1", "KC", "TB", false);
        done.status = Status::Final;
        app.apply_boards(League::Nfl, vec![done], false);
        assert!(app.pins[0].final_at.is_some());
    }

    #[test]
    fn enabled_cfb_tab_appears() {
        let dir = std::env::temp_dir().join(format!("gd-cfb-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let cfg = Config {
            enabled_tabs: vec![League::Nfl, League::Cfb],
            favorites: vec![],
            theme: "broadcast".into(),
            sort: Default::default(),
        };
        let app = App::new(cfg, vec![], dir, time::UtcOffset::UTC);
        assert_eq!(
            app.tab_list(),
            vec![
                Tab::Home,
                Tab::League(League::Nfl),
                Tab::League(League::Cfb)
            ]
        );
    }

    #[test]
    fn score_change_flashes_then_settles_once() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        assert!(!app.flash_active("1"), "first sighting must not flash");
        for _ in 0..5 {
            app.advance_tick();
        }
        let mut scored = g("1", "KC", "TB", true);
        scored.away_score = 13;
        app.apply_boards(League::Nfl, vec![scored.clone()], false);
        assert!(app.flash_active("1"), "data-driven score change flashes");
        for _ in 0..FLASH_TICKS - 1 {
            app.advance_tick();
        }
        assert!(app.flash_active("1"), "still lit one tick before the window ends");
        app.advance_tick();
        assert!(!app.flash_active("1"), "settles after FLASH_TICKS");
        // One-shot: the same score arriving again never re-flashes.
        app.apply_boards(League::Nfl, vec![scored], false);
        app.advance_tick();
        assert!(!app.flash_active("1"));
    }

    #[test]
    fn a_score_delta_captures_the_scoreboard_last_play_as_a_scoring_play() {
        let mut app = app_with(vec![], vec![]);
        let mut g1 = g("1", "SEA", "BOS", true);
        g1.away_score = 7;
        g1.home_score = 7;
        g1.last_plays = vec![Play {
            text: "Raleigh flies out".into(),
            team: "SEA".into(),
            ..Default::default()
        }];
        app.apply_boards(League::Nfl, vec![g1.clone()], false);
        assert!(app.scoring_events().is_empty(), "first sighting seeds silently");
        let mut g2 = g1.clone();
        g2.away_score = 8;
        g2.last_plays = vec![Play {
            text: "Rodríguez homers to left (18)".into(),
            team: "SEA".into(),
            ..Default::default()
        }];
        app.apply_boards(League::Nfl, vec![g2], false);
        let ev = app.scoring_events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].1.text, "Rodríguez homers to left (18)");
        assert!(ev[0].1.scoring);
        // The next poll (no delta) keeps it — boards are replaced wholesale.
        let mut g3 = g1.clone();
        g3.away_score = 8;
        app.apply_boards(League::Nfl, vec![g3], false);
        assert_eq!(
            app.scoring_events().len(),
            1,
            "carried across the board replacement"
        );
    }

    #[test]
    fn summary_scoring_plays_replace_the_delta_derived_list_and_survive_truncation() {
        let mut app = app_with(vec![g("1", "SEA", "BOS", true)], vec![]);
        let plays: Vec<Play> = (0..20)
            .map(|i| Play {
                text: format!("play {i}"),
                team: "SEA".into(),
                scoring: i == 3,
                ..Default::default()
            })
            .collect();
        let summary = Summary {
            last_plays: plays.clone(),
            scoring_plays: vec![plays[3].clone()],
            meter: None,
        };
        app.merge_summary("1", summary);
        let ev = app.scoring_events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].1.text, "play 3");
        assert_eq!(
            app.game_by_id("1").unwrap().last_plays.len(),
            20,
            "no 8-row truncation in the model"
        );
    }

    #[test]
    fn final_games_keep_their_scoring_plays_on_the_board() {
        let mut app = app_with(vec![], vec![]);
        let mut g1 = g("1", "SEA", "BOS", true);
        g1.away_score = 0;
        app.apply_boards(League::Nfl, vec![g1.clone()], false);
        let mut g2 = g1.clone();
        g2.away_score = 7;
        g2.last_plays = vec![Play {
            text: "TD".into(),
            team: "SEA".into(),
            ..Default::default()
        }];
        app.apply_boards(League::Nfl, vec![g2.clone()], false);
        let mut g3 = g2.clone();
        g3.status = Status::Final;
        app.apply_boards(League::Nfl, vec![g3], false);
        assert_eq!(
            app.scoring_events().len(),
            1,
            "a final's TD is still on the ticker"
        );
    }

    #[test]
    fn keyboard_never_starts_a_flash() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        for key in [
            KeyCode::Tab,
            KeyCode::Char('h'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char(' '),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('t'),
            KeyCode::Char('c'),
            KeyCode::Char('2'),
        ] {
            app.on_key(key, KeyModifiers::NONE);
            assert!(!app.flash_active("1"), "{key:?} must not animate");
        }
        app.advance_tick();
        assert!(!app.flash_active("1"));
    }

    #[test]
    fn live_pulse_is_a_pure_one_second_cadence() {
        // 10 render ticks bright, 10 dim, repeating.
        assert!(live_pulse_bright(0));
        assert!(live_pulse_bright(9));
        assert!(!live_pulse_bright(10));
        assert!(!live_pulse_bright(19));
        assert!(live_pulse_bright(20));
    }

    #[test]
    fn any_live_reflects_all_boards() {
        let mut app = app_with(vec![g("1", "KC", "TB", false)], vec![]);
        assert!(!app.any_live());
        app.apply_boards(League::Nba, vec![g("2", "DEN", "BOS", true)], false);
        assert!(app.any_live(), "a live game on any board counts");
    }

    #[test]
    fn ticker_scores_lane_is_every_board_but_honors_the_filter() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.apply_boards(League::Nba, vec![g("2", "DEN", "BOS", true)], false);
        app.tab = Tab::League(League::Nfl);
        let ids = |app: &App| -> Vec<String> { app.ticker_live().into_iter().map(|x| x.id).collect() };
        assert_eq!(ids(&app), vec!["1", "2"], "the ticker ignores the tab");
        app.filter = Some("den".into());
        assert_eq!(ids(&app), vec!["2"], "a typed filter narrows the ticker too");
    }

    #[test]
    fn arrow_keys_mirror_vim_keys() {
        let mut app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
            vec![],
        );
        app.config.enabled_tabs = vec![League::Nfl];
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.selected, 1, "Down == j");
        app.on_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.selected, 0, "Up == k");
        app.on_key(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home, "Right == l (wraps)");
        app.on_key(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::League(League::Nfl), "Left == h");
        app.on_key(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home, "Shift+Tab cycles back");
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.should_quit, "Ctrl+C must quit, not cycle the theme");
        assert_eq!(app.config.theme, "broadcast");
    }

    #[test]
    fn question_mark_toggles_help_and_esc_closes_topmost() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.view, View::Zoom { .. }));
        app.on_key(KeyCode::Char('?'), KeyModifiers::NONE);
        assert!(app.help_open);
        // While help is open other bindings are inert.
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.selected, 0);
        // Esc closes help FIRST; the zoom survives. A second Esc pops it.
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.help_open);
        assert!(matches!(app.view, View::Zoom { .. }), "help closes before the view");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        // '?' also closes it.
        app.on_key(KeyCode::Char('?'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('?'), KeyModifiers::NONE);
        assert!(!app.help_open);
    }

    #[test]
    fn q_inside_help_closes_help_not_the_app() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.on_key(KeyCode::Char('?'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!app.help_open && !app.should_quit);
    }

    #[test]
    fn toggling_a_league_off_and_on_keeps_canonical_tab_order() {
        let mut app = app_with(vec![], vec![]);
        app.config_toggle_tab(League::Nfl);
        app.config_toggle_tab(League::Nfl);
        assert_eq!(app.config.enabled_tabs, League::ALL.to_vec());
    }

    #[test]
    fn filter_miss_names_its_scope_and_the_ticker_match() {
        let mut app = app_with(vec![], vec![]);
        let mut sea = g("9", "SEA", "BOS", true);
        sea.league = League::Mlb;
        app.apply_boards(League::Mlb, vec![sea], false);
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.tab = Tab::League(League::Nfl);
        app.filter = Some("sea".into());
        assert_eq!(
            app.filter_miss_message(),
            "no games match \"sea\" on NFL · ticker matches SEA@BOS · esc clears"
        );
    }

    pub(crate) fn six_live() -> Vec<Game> {
        (0..6)
            .map(|i| g(&format!("g{i}"), "KC", "TB", true))
            .collect()
    }

    /// v3.2 §7 retired paging (`n`/`p`, `PAGE x/y`, the whole page index):
    /// the board is one list that scrolls. What replaces those four tests is
    /// the walk itself — j/k move through `Derived::selection` and wrap, and
    /// a board that shrinks under the selection re-clamps it.
    #[test]
    fn selection_walks_one_list_and_wraps() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        for _ in 0..5 {
            app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        }
        assert_eq!(app.selected, 5);
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.selected, 0, "j past the end wraps");
        app.on_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.selected, 5, "k from the top wraps to the end");
        // n/p are dead keys now, not paging.
        app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('p'), KeyModifiers::NONE);
        app.on_key(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(app.selected, 5, "no page keys left to move it");
    }

    #[test]
    fn a_shrinking_board_clamps_the_selection() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        app.selected = 5;
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        assert_eq!(app.selected, 0, "the selection can never point past the end");
    }

    #[test]
    fn last_scores_forget_games_that_left_the_boards() {
        // A game id that vanishes and later returns is a first sighting
        // again: seed silently, never flash.
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.apply_boards(League::Nfl, vec![], false);
        app.advance_tick();
        let mut back = g("1", "KC", "TB", true);
        back.away_score = 99; // different score than first seen
        app.apply_boards(League::Nfl, vec![back], false);
        assert!(
            !app.flash_active("1"),
            "re-appearing game must seed, not flash a stale diff"
        );
        assert_eq!(app.last_scores.len(), 1, "only games on the boards are remembered");
    }

    #[test]
    fn selection_continues_from_live_tiles_into_the_slate() {
        let mut app = app_with(
            vec![
                g("live1", "KC", "TB", true),
                g("pre1", "DAL", "PHI", false),
                g("pre2", "NYG", "WSH", false),
            ],
            vec![],
        );
        app.tab = Tab::League(League::Nfl);
        // j walks live tile -> slate row 1 -> slate row 2, then wraps.
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.selected, 1);
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.selected, 2);
        // Space pins the selected SLATE game.
        app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.pins.len(), 1);
        assert_eq!(app.pins[0].game_id, "pre2");
        app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.selected, 0, "wraps back to the live tile");
    }

    #[test]
    fn r_requests_refresh() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(app.refresh_now);
    }

    #[test]
    fn a_cached_board_never_rings_the_bell() {
        // A cached 14-10 -> 7-10 is a score going BACKWARDS. AlertState
        // diffs on inequality, so without the guard it banners "KC SCORES
        // 7-10" and rings — then rings again when the real board returns.
        let mut app = app_with(vec![], vec![]);
        app.config.favorites = vec![Favorite {
            league: League::Nfl,
            team_abbr: "KC".into(),
        }];
        let mut first = g("1", "KC", "TB", true);
        first.away_score = 14;
        first.home_score = 10;
        app.apply_boards(League::Nfl, vec![first.clone()], false);
        app.active_alert = None;
        app.bell_pending = false;

        let mut cached = first.clone();
        cached.away_score = 7;
        app.apply_boards(League::Nfl, vec![cached], true);
        assert!(app.active_alert.is_none(), "no banner from a cached payload");
        assert!(!app.bell_pending, "no bell from a cached payload");

        let mut next = first.clone();
        next.away_score = 21;
        app.apply_boards(League::Nfl, vec![next], false);
        assert!(
            app.active_alert.is_some(),
            "the real score change still alerts"
        );
        assert!(app.bell_pending);
    }

    #[test]
    fn apply_boards_marks_the_connection_live() {
        let mut app = app_with(vec![], vec![]);
        let now = std::time::Instant::now();
        assert!(matches!(app.net.chip(now, app.stale_after()), crate::app::net::NetChip::Live));
        assert!(app.net.upd_label(now, app.stale_after()).is_some(), "app_with applies a board");
        app.apply_boards(League::Nba, vec![], true);
        assert!(
            matches!(app.net.chip(now, app.stale_after()), crate::app::net::NetChip::Stale { .. }),
            "a cached apply is stale on arrival"
        );
    }

    #[test]
    fn a_cached_board_never_writes_a_scoring_play() {
        // A stale payload is an OLDER snapshot: its "delta" against the last
        // fresh scores is backwards, and the lastPlay it carries is not a
        // scoring play. Capturing it would put a bogus TD in the rail
        // forever, so the whole delta block is skipped when stale.
        let mut scored = g("1", "KC", "TB", true);
        scored.away_score = 14;
        scored.home_score = 10;
        scored.last_plays = vec![Play {
            clock: "5:00".into(),
            team: "KC".into(),
            text: "Mahomes 20 yd TD pass".into(),
            scoring: false,
            ..Default::default()
        }];
        let mut app = app_with(vec![scored.clone()], vec![]);
        app.advance_tick();
        assert_eq!(app.boards[&League::Nfl][0].scoring_plays.len(), 0);

        let mut cached = scored.clone();
        cached.away_score = 7; // an older, cached snapshot
        cached.last_plays = vec![Play {
            clock: "9:00".into(),
            team: "KC".into(),
            text: "Pacheco run for 3 yards".into(),
            scoring: false,
            ..Default::default()
        }];
        app.apply_boards(League::Nfl, vec![cached], true);
        assert!(!app.flash_active("1"), "a cached payload must not flash");
        assert_eq!(
            app.boards[&League::Nfl][0].scoring_plays.len(),
            0,
            "no scoring play from a cached payload"
        );
        assert_eq!(
            app.last_scores.get("1"),
            Some(&(14, 10)),
            "the fresh scores survive a cached apply"
        );

        let mut next = scored.clone();
        next.away_score = 21;
        next.last_plays = vec![Play {
            clock: "1:00".into(),
            team: "KC".into(),
            text: "Kelce 8 yd TD pass".into(),
            scoring: false,
            ..Default::default()
        }];
        app.apply_boards(League::Nfl, vec![next], false);
        let plays = &app.boards[&League::Nfl][0].scoring_plays;
        assert_eq!(plays.len(), 1, "exactly one scoring play: {plays:?}");
        assert!(plays[0].text.contains("Kelce"));
    }

    #[test]
    fn cfb_board_is_separate_from_nfl() {
        let dir = std::env::temp_dir().join(format!("gd-cfb2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(
            Config {
                enabled_tabs: vec![League::Nfl, League::Cfb],
                favorites: vec![],
                theme: "broadcast".into(),
                sort: Default::default(),
            },
            vec![],
            dir,
            time::UtcOffset::UTC,
        );
        let mut game = g("c1", "ALA", "UGA", true);
        game.league = League::Cfb;
        app.apply_boards(League::Cfb, vec![game], false);
        app.tab = Tab::League(League::Cfb);
        assert_eq!(app.visible_games()[0].id, "c1");
        app.tab = Tab::League(League::Nfl);
        assert!(app.visible_games().is_empty());
    }
}
