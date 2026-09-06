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
    /// `standings_max_scroll` so it can never run past what the pane shows.
    pub standings_scroll: usize,
    /// The largest offset the Standings pane could draw on its last frame (the
    /// renderer records it, like hit zones). `None` until the first draw, when
    /// the clamp falls back to the line count. Recorded rather than recomputed
    /// because it depends on the frame's width: spec v3.3 §5's two-column
    /// table halves how far there is to scroll.
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
    /// order is frozen even though watchability keeps rising (R24 / spec §2).
    ///
    /// `men` is soccer's on-field count (spec v3.4 §5). `hot` alone would not
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
mod tests {
    use super::*;
    use crate::config::{Config, Favorite, Pin};
    use crate::domain::*;
    use crate::rank::SortKey;
    use crate::views::ZoomTab;
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
            app.status_line
                .as_deref()
                .unwrap_or("")
                .contains("not saving"),
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
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        let screen = |t: &mut ratatui::Terminal<ratatui::backend::TestBackend>, app: &mut App| {
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
        assert!(
            healthy.starts_with("theme ") && !healthy.contains("not saving"),
            "{healthy}"
        );
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
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
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
        assert_eq!(
            app.boards[&League::Nfl][0].last_plays[0].text,
            "from summary"
        );
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
            View::Zoom {
                game_id: "1".into(),
                tab: ZoomTab::Overview
            }
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
            Play {
                clock: "1:00".into(),
                team: "KC".into(),
                text: "a".into(),
                ..Default::default()
            },
            Play {
                clock: "2:00".into(),
                team: "TB".into(),
                text: "b".into(),
                ..Default::default()
            },
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
        assert_eq!(
            theme::current_name(),
            "broadcast",
            "a tab switch never commits a preview"
        );
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
        assert_eq!(
            app.theme_prior, "broadcast",
            "reopen keeps the real prior theme"
        );
        assert_eq!(
            theme::current_name(),
            "studio",
            "reopen leaves the preview in place"
        );
        // A header tab click pops the picker like the Tab key: revert first.
        app.on_hit(keymap::Hit::TabChip(Tab::League(League::Nfl)));
        assert_eq!(app.view, View::Board);
        assert_eq!(app.tab, Tab::League(League::Nfl));
        assert_eq!(
            theme::current_name(),
            "broadcast",
            "a tab click never commits a preview"
        );
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
        assert!(app
            .status_line
            .as_deref()
            .unwrap_or("")
            .contains("sort time"));
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
            vec![ranked("mine", "Q1", "15:00", 3, 0), {
                let mut other = ranked("other", "Q4", "5:00", 24, 21);
                other.away = team("DAL");
                other.home = team("PHI");
                other
            }],
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
        assert_eq!(
            app.tv_shown.as_deref(),
            Some("mine"),
            ":tv opens on my game"
        );

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
        // spec v3.4 §3: the hot flag reads `situation.isRedZone`, not the
        // meter the gauge draws from it.
        a.situation = Some(crate::domain::Situation {
            is_red_zone: Some(true),
            ..Default::default()
        });
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
            extras: crate::domain::Extras::None,
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
        assert!(
            app.flash_active("1"),
            "still lit one tick before the window ends"
        );
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
        assert!(
            app.scoring_events().is_empty(),
            "first sighting seeds silently"
        );
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
            extras: crate::domain::Extras::None,
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

    /// A live NHL game the way the scoreboard maps one: no strength, no
    /// meter — those exist only in the summary (spec v3.4 §4).
    fn nhl_live(id: &str) -> Game {
        let mut x = g(id, "PIT", "WSH", true);
        x.league = League::Nhl;
        x.period = "2ND".into();
        x.clock = "15:37".into();
        x.situation = None;
        x.meter = None;
        x
    }

    fn power_play_summary() -> Summary {
        Summary {
            last_plays: vec![Play {
                text: "Sidney Crosby Slap Shot saved by Logan Thompson".into(),
                team: "PIT".into(),
                ..Default::default()
            }],
            scoring_plays: vec![],
            // R49: the summary never carries the meter — the zoom derives it
            // from these extras.
            meter: None,
            extras: crate::domain::Extras::Hockey {
                strength: crate::domain::HockeyStrength::PowerPlay,
                penalties: vec![crate::domain::PenaltyEvent {
                    team: "WSH".into(),
                    minutes: 2,
                    kind: "Minor".into(),
                    period: 2,
                    clock: "15:37".into(),
                }],
            },
        }
    }

    #[test]
    fn summary_strength_and_penalty_meter_survive_the_next_scoreboard_poll() {
        // R49 / v3.4 §4: the scoreboard poll replaces the board wholesale
        // every 15s and carries no NHL strength at all. Without the carry
        // the zoom's chip and meter blink out until the next summary lands.
        let mut app = app_with(vec![], vec![]);
        app.apply_boards(League::Nhl, vec![nhl_live("n1")], false);
        app.merge_summary("n1", power_play_summary());
        let live = |app: &App| app.game_by_id("n1").unwrap();
        assert!(
            matches!(live(&app).extras, crate::domain::Extras::Hockey { .. }),
            "the summary lands the strength"
        );

        app.apply_boards(League::Nhl, vec![nhl_live("n1")], false);
        let after = live(&app);
        assert!(
            matches!(after.extras, crate::domain::Extras::Hockey { .. }),
            "the strength holds across the scoreboard replace: {:?}",
            after.extras
        );
        assert_eq!(
            after.extras.penalty_meter(),
            Some(Meter::Penalty {
                team_abbr: "WSH".into(),
                seconds: 120
            }),
            "so the zoom's penalty meter is still derivable"
        );
        assert_eq!(
            after.meter, None,
            "and the shared meter field stays empty (R49)"
        );

        // A game going Final drops it: no power play survives the horn.
        let mut done = nhl_live("n1");
        done.status = Status::Final;
        app.apply_boards(League::Nhl, vec![done], false);
        assert_eq!(
            app.game_by_id("n1").unwrap().extras,
            crate::domain::Extras::None
        );
    }

    #[test]
    fn a_zoomed_power_play_never_reaches_the_board_ranking() {
        // R49: the summary lands only for the zoomed game, so scoring its
        // strength would give that one row a chip, the hot flag and a rank
        // bonus no identical unzoomed power play could earn.
        let mut app = app_with(vec![], vec![]);
        app.apply_boards(League::Nhl, vec![nhl_live("n1"), nhl_live("n2")], false);
        let before = ord(&app);
        let fps = app.rank_fingerprints.clone();
        app.merge_summary("n1", power_play_summary());
        let now = app.now();
        let watch = crate::rank::watchability(&app.game_by_id("n1").unwrap(), now);
        assert_eq!(
            watch.chip, None,
            "no board chip for a summary-derived power play"
        );
        assert!(!watch.hot, "and it does not read hot");

        // Another poll: the order and the fingerprint set are untouched, so
        // zooming a game can never move the board (R24).
        app.apply_boards(League::Nhl, vec![nhl_live("n1"), nhl_live("n2")], false);
        assert_eq!(ord(&app), before, "zooming did not reorder the board");
        assert_eq!(app.rank_fingerprints, fps, "nor did it move a fingerprint");

        // Demo and sim penalty meters carry no Extras::Hockey — they still
        // light the chip, so the gallery keeps its showcase.
        let mut demo = nhl_live("n3");
        demo.meter = Some(Meter::Penalty {
            team_abbr: "DAL".into(),
            seconds: 42,
        });
        assert_eq!(
            crate::rank::watchability(&demo, now).chip,
            Some("POWER PLAY")
        );
    }

    /// A live EPL match the scoreboard maps: minute in `period`, no clock,
    /// eleven a side.
    fn epl_live(id: &str, minute: &str, away: u16, home: u16) -> Game {
        let mut x = g(id, "AVL", "BHA", true);
        x.league = League::Epl;
        x.period = minute.into();
        x.clock = String::new();
        x.away_score = away;
        x.home_score = home;
        x.situation = None;
        x.meter = None;
        x.extras = crate::domain::Extras::Soccer {
            events: vec![],
            men: None,
        };
        x
    }

    /// Spec v3.4 §5 + R24: a sending-off is a real event, so it earns exactly
    /// ONE reorder. The men state is board-wide (it comes off the scoreboard
    /// every game already has), so unlike the NHL's summary strength it has
    /// no zoom asymmetry to defend against — what it must defend against is
    /// re-firing on every poll that repeats the same card.
    #[test]
    fn a_red_card_reorders_once_and_freezes() {
        let mut app = app_with(vec![], vec![]);
        app.apply_boards(
            League::Epl,
            vec![epl_live("a", "20'", 1, 0), epl_live("b", "63'", 1, 0)],
            false,
        );
        assert_eq!(ord(&app), vec!["b", "a"], "the later close match leads");

        // The card lands: "a" is down to ten and jumps the board.
        let mut carded = epl_live("a", "20'", 1, 0);
        carded.extras = crate::domain::Extras::Soccer {
            events: vec![crate::domain::MatchEvent {
                minute: "20'".into(),
                kind: crate::domain::EventKind::Red,
                team: "AVL".into(),
                player: "J. Gomes".into(),
                athlete_id: Some("301524".into()),
            }],
            men: Some((10, 11)),
        };
        let now = app.now();
        let w = crate::rank::watchability(&carded, now);
        assert_eq!(w.chip, Some("10 MEN"), "the card names the state");
        assert!(w.hot, "a red card is hot by definition");

        app.apply_boards(
            League::Epl,
            vec![carded.clone(), epl_live("b", "63'", 1, 0)],
            false,
        );
        assert_eq!(ord(&app), vec!["a", "b"], "one honest reorder");
        let after = ord(&app);
        let fps = app.rank_fingerprints.clone();

        // The same card, poll after poll, is not an event. The minute keeps
        // climbing on both matches and the board holds.
        for minute in ["21'", "22'", "23'"] {
            let mut still = carded.clone();
            still.period = minute.into();
            app.apply_boards(League::Epl, vec![still, epl_live("b", "63'", 1, 0)], false);
            assert_eq!(
                ord(&app),
                after,
                "the card fires once, not once per poll (R24)"
            );
        }
        assert_eq!(app.rank_fingerprints, fps, "nor did the fingerprint move");

        // A SECOND sending-off is a new event: the fingerprint carries the
        // count, not merely "somebody is short-handed".
        let mut two = carded.clone();
        two.extras = crate::domain::Extras::Soccer {
            events: vec![],
            men: Some((9, 11)),
        };
        app.apply_boards(League::Epl, vec![two, epl_live("b", "63'", 1, 0)], false);
        assert_ne!(app.rank_fingerprints, fps, "nine men is not ten men");
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
        let ids =
            |app: &App| -> Vec<String> { app.ticker_live().into_iter().map(|x| x.id).collect() };
        assert_eq!(ids(&app), vec!["1", "2"], "the ticker ignores the tab");
        app.filter = Some("den".into());
        assert_eq!(
            ids(&app),
            vec!["2"],
            "a typed filter narrows the ticker too"
        );
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
        assert!(
            matches!(app.view, View::Zoom { .. }),
            "help closes before the view"
        );
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
        assert_eq!(
            app.selected, 0,
            "the selection can never point past the end"
        );
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
        assert_eq!(
            app.last_scores.len(),
            1,
            "only games on the boards are remembered"
        );
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
        assert!(
            app.active_alert.is_none(),
            "no banner from a cached payload"
        );
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
        assert!(matches!(
            app.net.chip(now, app.stale_after()),
            crate::app::net::NetChip::Live
        ));
        assert!(
            app.net.upd_label(now, app.stale_after()).is_some(),
            "app_with applies a board"
        );
        app.apply_boards(League::Nba, vec![], true);
        assert!(
            matches!(
                app.net.chip(now, app.stale_after()),
                crate::app::net::NetChip::Stale { .. }
            ),
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
