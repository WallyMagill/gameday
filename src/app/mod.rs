//! App state and the shared chrome. `keys` (the per-view key handlers),
//! `net` (connection truth), `chrome` (header/footer/help), `derive` (the
//! per-frame game lists), `draw` (the frame), `merge` (the data merge path),
//! `order` (the live band's ordering) and `persist` (config and pin writes)
//! are children of this module — everything they touch lives on `App`.

mod chrome;
mod derive;
mod draw;
mod keys;
mod merge;
pub mod net;
mod order;
mod persist;

pub use derive::Derived;

use crate::app::net::NetStatus;
use crate::config::{Config, Pin};
use crate::domain::{Game, GameStats, League, StandingsTable, Status};
use crate::input::{CompletionState, InputMode};
use crate::keymap;
use crate::theme;
use crate::views::View;
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// board never slides under the eye between events.
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
    /// TV: the game filling the screen. Set when `:tv`/`v` opens,
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
    /// `standings_max_scroll` so it can never run past what the pane shows.
    pub standings_scroll: usize,
    /// The largest offset the Standings pane could draw on its last frame (the
    /// renderer records it, like hit zones). `None` until the first draw, when
    /// the clamp falls back to the line count. Recorded rather than recomputed
    /// because it depends on the frame's width: the two-column table halves
    /// how far there is to scroll.
    pub standings_max_scroll: Option<usize>,
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
    /// re-sort: id -> (away, home, status, hot, men). A poll that only advanced
    /// the clock leaves every fingerprint equal, so no reorder happens — the
    /// order is frozen even though watchability keeps rising.
    ///
    /// `men` is soccer's on-field count. `hot` alone would not
    /// carry it: a match already hot on STOPPAGE that then loses a man would
    /// show the same fingerprint, and the sending-off — the biggest thing to
    /// happen to that match — would never move the board. The count, not a
    /// boolean, so a SECOND red is its own event too.
    rank_fingerprints: HashMap<String, order::RankFingerprint>,
    /// game id -> tick when its score last changed; drives the one-shot flash.
    flashes: HashMap<String, u64>,
    /// Favorite-score alert diff state (own score memory + per-game cooldown).
    alerts: crate::alerts::AlertState,
    /// The header banner currently showing, if any; expired by
    /// `advance_tick` once its `until_tick` passes.
    pub active_alert: Option<crate::alerts::Alert>,
    /// The scoring cut: a full-frame takeover for a game you care
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
            standings_max_scroll: None,
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
        if self
            .active_alert
            .as_ref()
            .is_some_and(|a| tick >= a.until_tick)
        {
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

    /// The filter the board is narrowed by right now: the open `/` prompt's
    /// buffer while typing (incremental), else the committed filter.
    pub(crate) fn active_filter(&self) -> Option<&str> {
        if let InputMode::Filter { buf } = &self.mode {
            return (!buf.is_empty()).then_some(buf.as_str());
        }
        self.filter.as_deref()
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
mod tests;
