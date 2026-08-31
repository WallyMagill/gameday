use crate::config::{prune_pins, save_pins, Config, Favorite, Pin};
use crate::domain::{Game, GameStats, League, StandingsTable, Status, Summary};
use crate::home::home_games;
use crate::input::{CompletionState, InputMode};
use crate::keymap;
use crate::theme;
use crate::tiles::packer::{page_size, LayoutPref};
use crate::tiles::TileFx;
use crate::views::{self, View, ZoomTab};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
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
    (tick / 10) % 2 == 0
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

/// Local calendar date, the anchor date travel steps from.
fn today_local() -> time::Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

/// Does either team match the `/` filter? Case-insensitive substring on
/// abbr ("KC"), location ("KANSAS CITY"), and name ("Chiefs").
fn game_matches(game: &Game, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    [&game.away, &game.home].into_iter().any(|t| {
        t.abbr.to_lowercase().contains(&needle)
            || t.location.to_lowercase().contains(&needle)
            || t.name.to_lowercase().contains(&needle)
    })
}

pub struct App {
    pub tab: Tab,
    pub page: usize,
    pub selected: usize,
    pub pins: Vec<Pin>,
    pub config: Config,
    pub boards: HashMap<League, Vec<Game>>,
    /// Box scores by game id, filled by the ~30s stats poll while that game
    /// is zoomed. Pruned with `last_scores` when a game leaves every board.
    pub stats: HashMap<String, GameStats>,
    /// League standings, fetched on demand when `:standings` opens (10-min
    /// cache in the provider). At most one small table per league — no
    /// pruning needed.
    pub standings: HashMap<League, StandingsTable>,
    pub stale: bool,
    pub should_quit: bool,
    pub refresh_now: bool,
    /// Which full-screen surface the body renders; Board is the mosaic.
    /// Replaces the old `focused_id` mechanism — the zoomed game id lives
    /// inside `View::Zoom`.
    pub view: View,
    /// Highlighted row in the Zoom Plays feed (j/k); reset when the zoom
    /// opens or its tab changes.
    pub zoom_scroll: usize,
    /// Highlighted row in the global PlaysFeed (`:plays`); reset when the
    /// view opens.
    pub feed_scroll: usize,
    /// Top-line offset in the Standings view (j/k, no highlight — the table
    /// is read-only); reset when the view opens. The renderer re-clamps
    /// against the real pane height.
    pub standings_scroll: usize,
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
    /// Wall-clock moment of the last successful `apply_boards` — drives the
    /// footer's "UPD 12s" freshness age.
    pub last_update: Option<Instant>,
    pub config_dir: PathBuf,
    /// Monotonic render tick (~10/s while live, ~1/s idle). Every animation
    /// is a pure function of this counter plus app state — keyboard input
    /// redraws but never advances it, so keys can't animate anything.
    pub tick: u64,
    /// Last-seen (away, home) score per game id: apply_boards diffs against
    /// this to detect data-driven score changes.
    last_scores: HashMap<String, (u16, u16)>,
    /// game id -> tick when its score last changed; drives the one-shot flash.
    flashes: HashMap<String, u64>,
    /// Favorite-score alert diff state (own score memory + per-game cooldown).
    alerts: crate::alerts::AlertState,
    /// The header banner currently showing, if any; expired by
    /// `advance_tick` once its `until_tick` passes.
    pub active_alert: Option<crate::alerts::Alert>,
    /// Set when a banner starts; main consumes it to write the terminal
    /// bell (`\x07`) — App never touches stdout itself.
    pub bell_pending: bool,
    /// Clickable regions, rebuilt from scratch on every draw by whichever
    /// view rendered (tiles, tab chips, slate rows, zoom tabs). A click
    /// resolves against the LAST frame's zones — stale for at most one
    /// render tick.
    pub hit_zones: Vec<(Rect, keymap::Hit)>,
}

impl App {
    pub fn new(config: Config, pins: Vec<Pin>, config_dir: PathBuf) -> Self {
        Self {
            tab: Tab::Home,
            page: 0,
            selected: 0,
            pins,
            config,
            boards: HashMap::new(),
            stats: HashMap::new(),
            standings: HashMap::new(),
            stale: false,
            should_quit: false,
            refresh_now: false,
            view: View::Board,
            zoom_scroll: 0,
            feed_scroll: 0,
            standings_scroll: 0,
            mode: InputMode::Normal,
            status_line: None,
            filter: None,
            viewed_date_offset: HashMap::new(),
            dated_boards: HashMap::new(),
            completion: None,
            help_open: false,
            last_update: None,
            config_dir,
            tick: 0,
            last_scores: HashMap::new(),
            flashes: HashMap::new(),
            alerts: crate::alerts::AlertState::default(),
            active_alert: None,
            bell_pending: false,
            hit_zones: Vec::new(),
        }
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
            Hit::Tile(i) => {
                self.selected = i;
                self.clamp_selected();
            }
            Hit::SlateRow(i) => {
                self.selected = self.live_games().len() + i;
                self.clamp_selected();
            }
            Hit::TabChip(tab) => self.set_tab(tab),
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
                    View::ConfigView => {}
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
    /// and drops to ~1fps otherwise.
    pub fn any_live(&self) -> bool {
        self.boards
            .values()
            .flatten()
            .any(|g| g.status == Status::Live)
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

    pub fn visible_games(&self) -> Vec<Game> {
        let games = match self.tab {
            Tab::Home => {
                let concat = self.concat_boards();
                home_games(
                    &self.pins,
                    &self.config.favorites,
                    &concat,
                    OffsetDateTime::now_utc(),
                )
                .into_iter()
                .cloned()
                .collect()
            }
            Tab::League(league) => self.league_games(league),
        };
        match self.active_filter() {
            Some(needle) => games
                .into_iter()
                .filter(|g| game_matches(g, needle))
                .collect(),
            None => games,
        }
    }

    /// One league's board as the tab shows it: today's live board, or — while
    /// date-traveled — the fetched slate for the viewed date (empty until the
    /// on-demand fetch answers).
    fn league_games(&self, league: League) -> Vec<Game> {
        match self.viewed_date(league) {
            Some(date) => self
                .dated_boards
                .get(&(league, date))
                .cloned()
                .unwrap_or_default(),
            None => self.boards.get(&league).cloned().unwrap_or_default(),
        }
    }

    /// The date `league`'s tab is viewing, None when it's live today.
    pub fn viewed_date(&self, league: League) -> Option<time::Date> {
        let off = self.viewed_date_offset.get(&league).copied().unwrap_or(0);
        if off == 0 {
            return None;
        }
        today_local().checked_add(time::Duration::days(off as i64))
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

    pub fn live_games(&self) -> Vec<Game> {
        self.visible_games()
            .into_iter()
            .filter(|g| g.status == Status::Live)
            .collect()
    }

    pub fn slate_games(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => vec![],
            Tab::League(_) => self
                .visible_games()
                .into_iter()
                .filter(|g| g.status == Status::Pre || g.status == Status::Final)
                .collect(),
        }
    }

    pub fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // Raw mode swallows SIGINT, so Ctrl+C must be an explicit quit.
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // Help is modal: Esc closes the topmost layer (help before views),
        // '?' toggles, q still quits; everything else is inert while open.
        if self.help_open {
            match code {
                KeyCode::Esc | KeyCode::Char('?') => self.help_open = false,
                KeyCode::Char('q') => self.should_quit = true,
                _ => {}
            }
            return;
        }
        match self.view {
            View::Board => self.on_key_board(code),
            View::Zoom { .. } => self.on_key_zoom(code),
            View::PlaysFeed => self.on_key_plays_feed(code),
            View::Standings(_) => self.on_key_standings(code),
            // Placeholder view (its task lands later in this plan): only the
            // ways out are wired.
            View::ConfigView => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
                KeyCode::Char('?') => self.help_open = true,
                _ => {}
            },
        }
    }

    /// Keys in the Standings view: j/k scroll the table one line, PgUp/PgDn
    /// jump, Esc/q pop back to the board (q quits ONLY there).
    fn on_key_standings(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_standings_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_standings_scroll(-1),
            KeyCode::PageDown => self.move_standings_scroll(FEED_PAGE_JUMP),
            KeyCode::PageUp => self.move_standings_scroll(-FEED_PAGE_JUMP),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('r') => self.refresh_now = true,
            _ => {}
        }
    }

    /// Scroll the Standings table, clamped to its composed line count (the
    /// renderer re-clamps against the pane height so the last page can't
    /// scroll into blank space).
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
        let next = self.standings_scroll as isize + delta;
        self.standings_scroll = next.clamp(0, lines as isize - 1) as usize;
    }

    /// Keys inside the global PlaysFeed: j/k move the highlight one row,
    /// PgUp/PgDn jump, Esc/q pop back to the board (q quits ONLY there).
    fn on_key_plays_feed(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_feed_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_feed_scroll(-1),
            KeyCode::PageDown => self.move_feed_scroll(FEED_PAGE_JUMP),
            KeyCode::PageUp => self.move_feed_scroll(-FEED_PAGE_JUMP),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
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
            KeyCode::Char('n') | KeyCode::PageDown => self.change_page(1),
            KeyCode::Char('p') | KeyCode::PageUp => self.change_page(-1),
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('1') => self.set_layout(LayoutPref::One),
            KeyCode::Char('2') => self.set_layout(LayoutPref::Two),
            KeyCode::Char('4') => self.set_layout(LayoutPref::Four),
            KeyCode::Char('s') => self.set_layout(LayoutPref::Sidebar),
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
            self.view = View::Zoom {
                game_id: game.id,
                tab: ZoomTab::Overview,
            };
            self.zoom_scroll = 0;
        }
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

    pub fn apply_boards(&mut self, league: League, games: Vec<Game>, stale: bool) {
        let now = OffsetDateTime::now_utc();
        // Score-change flash fires ONLY here — from data. A first sighting
        // (startup, new game) seeds last_scores without flashing.
        for g in &games {
            let score = (g.away_score, g.home_score);
            if let Some(prev) = self.last_scores.get(&g.id) {
                if *prev != score {
                    self.flashes.insert(g.id.clone(), self.tick);
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
        // the header banner and queues the bell for main to ring.
        if let Some(alert) =
            self.alerts
                .check(&self.config.favorites, &self.boards, self.tick)
        {
            self.active_alert = Some(alert);
            self.bell_pending = true;
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
        self.stale = stale;
        self.last_update = Some(Instant::now());
        self.pins = prune_pins(std::mem::take(&mut self.pins), now);
        let _ = save_pins(&self.config_dir, &self.pins);
        self.clamp_selected();
    }

    pub fn merge_summary(&mut self, game_id: &str, summary: Summary) {
        // Non-football summaries carry no "drives", so they map to zero plays;
        // keep the scoreboard's lastPlay instead of blanking the tile.
        if summary.last_plays.is_empty() {
            return;
        }
        for board in self.boards.values_mut() {
            if let Some(game) = board.iter_mut().find(|g| g.id == game_id) {
                let mut last_plays = summary.last_plays;
                for play in &mut last_plays {
                    if summary.scoring_plays.iter().any(|s| s.text == play.text) {
                        play.scoring = true;
                    }
                }
                game.last_plays = last_plays;
                return;
            }
        }
    }

    /// The zoomed game's (league, id) — the stats poll's only target. None
    /// unless the Zoom view is open and its game is still on a board.
    pub fn stats_target(&self) -> Option<(League, String)> {
        self.zoomed_game().map(|g| (g.league, g.id))
    }

    pub fn poll_plan(&self) -> crate::poll::PollPlan {
        crate::poll::plan(
            &self.visible_for_poll(),
            &self.config.enabled_tabs,
            self.stats_target(),
        )
    }

    /// Latest box score for `game_id`, from the stats poll (or a fixture in
    /// tests/dump). Replaces wholesale — rows are a snapshot, not a delta.
    pub fn merge_stats(&mut self, game_id: &str, stats: GameStats) {
        self.stats.insert(game_id.to_string(), stats);
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
    pub fn merge_standings(&mut self, table: StandingsTable) {
        self.standings.insert(table.league, table);
    }

    pub fn effective_layout(&self) -> LayoutPref {
        if matches!(self.view, View::Zoom { .. }) {
            LayoutPref::One
        } else {
            self.config.layout
        }
    }

    fn concat_boards(&self) -> Vec<Game> {
        let mut out = Vec::new();
        for league in &self.config.enabled_tabs {
            if let Some(games) = self.boards.get(league) {
                out.extend(games.iter().cloned());
            }
        }
        out
    }

    fn visible_for_poll(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(league) => self.boards.get(&league).cloned().unwrap_or_default(),
        }
    }

    /// Everything j/k can land on. On a league tab the selection runs through
    /// the live mosaic tiles first, then continues into the slate rows below.
    fn selection_list(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let mut list = self.live_games();
                list.extend(self.slate_games());
                list
            }
        }
    }

    fn selected_game(&self) -> Option<Game> {
        let list = self.selection_list();
        list.get(self.selected).cloned()
    }

    fn clamp_selected(&mut self) {
        let n = self.selection_list().len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
        // A shrunk board must never leave the page index past the end.
        self.page = self.page.min(self.page_count() - 1);
    }

    /// Tiles per mosaic page under the current layout.
    fn page_len(&self) -> usize {
        page_size(self.effective_layout(), self.mosaic_games().len().max(1))
    }

    /// Number of mosaic pages, always >= 1.
    pub fn page_count(&self) -> usize {
        let n = self.mosaic_games().len();
        n.max(1).div_ceil(self.page_len())
    }

    /// n/p and PgDn/PgUp: wrap around the known page count (never a blank
    /// page past the end) and land the selection on the page's first tile.
    fn change_page(&mut self, delta: isize) {
        let count = self.page_count() as isize;
        self.page = (self.page as isize + delta).rem_euclid(count) as usize;
        let sel_len = self.selection_list().len();
        self.selected = (self.page * self.page_len()).min(sel_len.saturating_sub(1));
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
        self.page = 0;
        self.selected = 0;
        self.view = View::Board;
    }

    fn move_selected(&mut self, delta: isize) {
        let n = self.selection_list().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
        // Page follows the selection so the highlighted tile is always on
        // screen; a selection down in the slate leaves the mosaic page alone.
        if self.selected < self.mosaic_games().len() {
            self.page = self.selected / self.page_len();
        }
    }

    fn toggle_pin(&mut self) {
        let Some(game) = self.selected_game() else {
            return;
        };
        if let Some(idx) = self.pins.iter().position(|p| p.game_id == game.id) {
            self.pins.remove(idx);
        } else {
            self.pins.push(Pin {
                game_id: game.id,
                league: game.league,
                final_at: None,
            });
        }
        let _ = save_pins(&self.config_dir, &self.pins);
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
        } else {
            self.config.favorites.push(Favorite {
                league: game.league,
                team_abbr: abbr,
            });
        }
        let _ = self.config.save_to(&self.config_dir);
        self.clamp_selected();
    }

    fn set_layout(&mut self, layout: LayoutPref) {
        self.config.layout = layout;
        let _ = self.config.save_to(&self.config_dir);
    }

    /// 'c': broadcast -> ceefax -> phosphor -> broadcast, persisted like layout.
    fn cycle_theme(&mut self) {
        let next = theme::current_name().next();
        theme::set_current(next);
        self.config.theme = next.as_str().to_string();
        let _ = self.config.save_to(&self.config_dir);
    }

    pub fn draw(&mut self, frame: &mut Frame) {
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
            frame.render_widget(
                Paragraph::new("need more columns")
                    .style(Style::default().fg(th.muted).bg(th.bg)),
                area,
            );
            return;
        }
        let ticker_h = if area.height >= 24 { 4 } else { 0 };
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
        views::draw(self, frame, chunks[1]);
        if ticker_h > 0 {
            self.draw_ticker(frame, chunks[2]);
        }
        self.draw_footer(frame, chunks[3]);
        if self.help_open {
            self.draw_help(frame, area);
        }
    }

    fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let mut spans = vec![
            Span::styled(
                " GAMEDAY ",
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  FILTER: ", Style::default().fg(th.muted)),
        ];
        for tab in self.tab_list() {
            let label = match tab {
                Tab::Home => "ALL".to_string(),
                Tab::League(league) => league.slug().to_uppercase(),
            };
            let chip = if tab == self.tab {
                Span::styled(
                    format!("[{label}]"),
                    Style::default()
                        .fg(th.bg)
                        .bg(th.star)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(format!("[ {label} ]"), Style::default().fg(th.muted))
            };
            // Register the chip as a click zone at its rendered columns (the
            // header is all single-width chars, so chars == cells).
            let x: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            let w = chip.content.chars().count();
            if x + w <= area.width as usize {
                self.hit_zones.push((
                    Rect {
                        x: area.x + x as u16,
                        y: area.y,
                        width: w as u16,
                        height: 1,
                    },
                    keymap::Hit::TabChip(tab),
                ));
            }
            spans.push(chip);
            spans.push(Span::raw(" "));
        }
        if self.stale {
            spans.push(Span::styled(" STALE", Style::default().fg(th.star)));
        }
        // Favorite-score banner: earned red — the live role, spec's color
        // discipline — for its short lifetime, then advance_tick drops it.
        if let Some(alert) = &self.active_alert {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                alert.text.clone(),
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            ));
        }
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        // While the current tab is date-traveled the viewed date replaces the
        // live one, marked ‹ › so a past/future slate can't pass for today.
        let traveled = match self.tab {
            Tab::League(league) => self.viewed_date(league),
            Tab::Home => None,
        };
        let (date, date_style) = match traveled {
            Some(d) => (
                format!("‹ {} ›", date_label(d)),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ),
            None => (
                format!("{} {}", date_label(now.date()), now.year()),
                Style::default().fg(th.green).add_modifier(Modifier::BOLD),
            ),
        };
        let (h12, ampm) = match now.hour() {
            0 => (12, "AM"),
            h if h < 12 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        let clock = format!("{}:{:02}:{:02} {}", h12, now.minute(), now.second(), ampm);
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let right_len = date.chars().count() + 2 + clock.len() + 1;
        let spacer = (area.width as usize).saturating_sub(left_len + right_len);
        spans.push(Span::raw(" ".repeat(spacer)));
        spans.push(Span::styled(date, date_style));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(clock, Style::default().fg(th.cyan).add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    /// Scoring plays across every visible board, newest-ish first: (game, play).
    pub(crate) fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let mut out = Vec::new();
        for game in self.concat_boards() {
            if game.status != Status::Live {
                continue;
            }
            for play in game.last_plays.iter().filter(|p| p.scoring) {
                out.push((game.clone(), play.clone()));
            }
        }
        out
    }

    pub(crate) fn team_color(game: &Game, abbr: &str) -> ratatui::style::Color {
        let th = theme::current();
        if game.away.abbr.eq_ignore_ascii_case(abbr) {
            theme::rgb(game.away.color)
        } else if game.home.abbr.eq_ignore_ascii_case(abbr) {
            theme::rgb(game.home.color)
        } else {
            th.fg
        }
    }

    /// Games shown as mosaic tiles. With no live games on a league tab the
    /// slate games fill the mosaic as tiles — never a blank pane — while the
    /// slate strip below still lists them departure-board style.
    pub(crate) fn mosaic_games(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let live = self.live_games();
                if !live.is_empty() {
                    live
                } else {
                    self.slate_games()
                }
            }
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

    /// Per-tile animation state: pure in (tick, flash table) so a dump at a
    /// fixed tick always renders the same frame.
    pub(crate) fn tile_fx(&self, game: &Game) -> TileFx {
        TileFx {
            flash: self.flash_active(&game.id),
            live_bright: live_pulse_bright(self.tick),
        }
    }

    /// Ticker content, alternated into two rows. No cap: overflow scrolls
    /// (marquee), so every event eventually comes into view.
    fn ticker_rows(&self) -> [Vec<(char, Style)>; 2] {
        let th = theme::current();
        let events = self.scoring_events();
        let mut rows: [Vec<(char, Style)>; 2] = [Vec::new(), Vec::new()];
        if events.is_empty() {
            push_cells(
                &mut rows[0],
                "no scoring plays yet",
                Style::default().fg(th.dim),
            );
        }
        for (i, (game, play)) in events.iter().enumerate() {
            let row = &mut rows[i % 2];
            if !row.is_empty() {
                push_cells(row, "  |  ", Style::default().fg(th.dim));
            }
            push_cells(
                row,
                &format!("{} ", play.clock),
                Style::default().fg(th.cyan),
            );
            push_cells(
                row,
                &format!("{} ", play.team),
                Style::default()
                    .fg(Self::team_color(game, &play.team))
                    .add_modifier(Modifier::BOLD),
            );
            push_cells(
                row,
                &format!("{} ", theme::scoring_word(game.league)),
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            );
            push_cells(row, &play.text, Style::default().fg(th.fg));
            let leader = if game.away_score >= game.home_score {
                format!(" {}-{} {}", game.away_score, game.home_score, game.away.abbr)
            } else {
                format!(" {}-{} {}", game.home_score, game.away_score, game.home.abbr)
            };
            push_cells(row, &leader, Style::default().fg(th.bright));
        }
        rows
    }

    fn draw_ticker(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th.live));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 {
            return;
        }
        let labels = [" GAMEDAY  ", " TICKER   "];
        let label_style = Style::default().fg(th.live).add_modifier(Modifier::BOLD);
        let content_w = (inner.width as usize).saturating_sub(labels[0].chars().count());
        let rows = self.ticker_rows();
        let mut lines = Vec::new();
        for (label, row) in labels.iter().zip(rows.iter()).take(inner.height as usize) {
            let mut spans = vec![Span::styled(*label, label_style)];
            spans.extend(marquee_spans(row, content_w, self.tick));
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Context-aware footer: the TOP chords from the keymap table (the full
    /// set lives in the '?' overlay) plus position + freshness on the right.
    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        // An open prompt owns the whole footer row; a status line (command
        // error, pin result) owns it until the next keypress dismisses it.
        let prompt = match &self.mode {
            InputMode::Command { buf } => Some((':', buf)),
            InputMode::Filter { buf } => Some(('/', buf)),
            InputMode::Normal => None,
        };
        if let Some((sigil, buf)) = prompt {
            let line = Line::from(vec![
                Span::styled(
                    format!(" {sigil}"),
                    Style::default().fg(th.star).add_modifier(Modifier::BOLD),
                ),
                Span::styled(buf.clone(), Style::default().fg(th.bright)),
                Span::styled("▌", Style::default().fg(th.star)),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(Style::default().bg(th.bg)),
                area,
            );
            return;
        }
        if let Some(status) = &self.status_line {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {status}"),
                    Style::default().fg(th.star),
                )))
                .style(Style::default().bg(th.bg)),
                area,
            );
            return;
        }
        // Footer chords follow the view: the board advertises zoom + quit,
        // every other view advertises the way back (and zoom its tab cycle).
        let zoomed = self.view != View::Board;
        // Narrow terminals can't hold every chord: shed the low-value ones in
        // keymap's declared order so HELP and QUIT are never the ones clipped.
        let mut chords = keymap::footer_chords(zoomed);
        // " /kc" steals footer columns, so it counts toward the shed budget.
        let filter_width = self.filter.as_ref().map_or(0, |f| f.chars().count() + 2);
        let chords_width = |cs: &[(&str, &str)]| -> usize {
            filter_width
                + 5
                + cs.iter()
                    .map(|(k, a)| 4 + k.chars().count() + a.chars().count())
                    .sum::<usize>()
        };
        for drop in keymap::FOOTER_DROP_ORDER {
            if chords_width(&chords) < area.width as usize {
                break;
            }
            chords.retain(|(_, a)| a != drop);
        }
        let mut spans = Vec::new();
        // An active committed filter stays visible so a narrowed board is
        // never mistaken for a quiet one.
        if let Some(f) = &self.filter {
            spans.push(Span::styled(
                format!(" /{f}"),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ));
        }
        spans.push(Span::styled(
            " NAV:",
            Style::default().fg(th.fg).add_modifier(Modifier::BOLD),
        ));
        for (key, action) in chords {
            spans.push(Span::styled(format!(" [{key}]"), Style::default().fg(th.fg)));
            spans.push(Span::styled(format!(" {action}"), Style::default().fg(th.muted)));
        }

        // Right side, dropped piecewise if the row runs out of columns:
        // GAME 3/8 goes first, PAGE and UPD stay.
        let mut right: Vec<String> = Vec::new();
        if zoomed {
            if let Some(g) = self.zoomed_game() {
                right.push(format!("FOCUS {}@{}", g.away.abbr, g.home.abbr));
            }
        } else {
            let sel_len = self.selection_list().len();
            if sel_len > 1 {
                right.push(format!("GAME {}/{}", self.selected + 1, sel_len));
            }
        }
        let pages = self.page_count();
        if pages > 1 && !zoomed {
            right.push(format!("PAGE {}/{}", self.page.min(pages - 1) + 1, pages));
        }
        if let Some(upd) = self.last_update.map(|t| age_label(t.elapsed().as_secs())) {
            right.push(upd);
        }
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let width = area.width as usize;
        while !right.is_empty() && left_len + right.join("  ").chars().count() + 2 > width {
            right.remove(0);
        }
        if !right.is_empty() {
            let text = right.join("  ");
            let spacer = width.saturating_sub(left_len + text.chars().count() + 1);
            spans.push(Span::raw(" ".repeat(spacer)));
            spans.push(Span::styled(text, Style::default().fg(th.cyan)));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    /// '?': every chord, grouped, over a luminance-dimmed board. Generated
    /// from the same keymap table as the footer.
    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let buf = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                cell.fg = theme::dimmed(cell.fg);
                cell.bg = theme::dimmed(cell.bg);
            }
        }
        let mut lines: Vec<Line> = Vec::new();
        for group in keymap::Group::ALL {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                group.title(),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            )));
            for (keys, label) in keymap::help_rows(group) {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {keys:<22}"), Style::default().fg(th.fg)),
                    Span::styled(label, Style::default().fg(th.muted)),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "ESC/? CLOSES",
            Style::default().fg(th.dim),
        )));
        let w = 40u16.min(area.width.saturating_sub(4));
        let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let panel = Rect {
            x: area.x + (area.width - w) / 2,
            y: area.y + (area.height - h) / 2,
            width: w,
            height: h,
        };
        frame.render_widget(Clear, panel);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th.star))
            .title(Span::styled(
                " KEYS ",
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(th.bg).fg(th.fg)),
            panel,
        );
    }
}

/// "UPD 12s" freshness age for the footer; minutes past 60s.
fn age_label(secs: u64) -> String {
    if secs < 60 {
        format!("UPD {secs}s")
    } else {
        format!("UPD {}m", secs / 60)
    }
}

/// Blank cells between the tail and the wrapped head of a scrolling ticker
/// row — enough of a gap to read as "the reel restarted".
const MARQUEE_GAP: usize = 10;

fn push_cells(row: &mut Vec<(char, Style)>, text: &str, style: Style) {
    row.extend(text.chars().map(|c| (c, style)));
}

/// A `width`-cell window into `cells`, scrolled one cell per render tick with
/// wraparound. Content that fits renders unshifted — no motion. Pure in
/// (cells, width, tick).
fn marquee_spans(cells: &[(char, Style)], width: usize, tick: u64) -> Vec<Span<'static>> {
    if cells.len() <= width {
        return group_spans(cells.iter().copied());
    }
    let total = cells.len() + MARQUEE_GAP;
    let offset = (tick as usize) % total;
    group_spans((0..width).map(|i| {
        let idx = (offset + i) % total;
        cells.get(idx).copied().unwrap_or((' ', Style::default()))
    }))
}

/// Merge runs of identically-styled cells back into spans.
fn group_spans(cells: impl Iterator<Item = (char, Style)>) -> Vec<Span<'static>> {
    let mut out: Vec<(String, Style)> = Vec::new();
    for (ch, style) in cells {
        match out.last_mut() {
            Some((text, last)) if *last == style => text.push(ch),
            _ => out.push((ch.to_string(), style)),
        }
    }
    out.into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Favorite, Pin};
    use crate::domain::*;
    use crate::tiles::packer::LayoutPref;
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

    fn g(id: &str, away: &str, home: &str, live: bool) -> Game {
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
            start_time: None,
            broadcast: None,
            odds: None,
        }
    }

    fn app_with(games: Vec<Game>, pins: Vec<Pin>) -> App {
        let dir = std::env::temp_dir().join(format!("gd-app-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), pins, dir);
        app.apply_boards(League::Nfl, games, false);
        app
    }

    #[test]
    fn home_shows_only_pinned() {
        let app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
            vec![Pin {
                game_id: "1".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        let ids: Vec<_> = app.visible_games().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["1"]);
    }

    #[test]
    fn nfl_tab_shows_all_league_games() {
        let mut app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", false)],
            vec![],
        );
        app.tab = Tab::League(League::Nfl);
        assert_eq!(app.visible_games().len(), 2);
        assert_eq!(app.live_games().len(), 1);
        assert_eq!(app.slate_games().len(), 1);
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
        assert_eq!(app.live_games().len(), 1);
        // Esc in Normal mode clears the committed filter.
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.filter, None);
        assert_eq!(app.live_games().len(), 2);
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
            scoring: false,
        }];
        let mut app = app_with(vec![game], vec![]);
        // MLB/NBA summaries have no drives => zero mapped plays; don't blank the tile.
        app.merge_summary("1", crate::domain::Summary::default());
        let board = &app.boards[&League::Nfl];
        assert_eq!(board[0].last_plays[0].text, "from scoreboard");
        // A real summary still replaces them.
        let mut s = crate::domain::Summary::default();
        s.last_plays = vec![crate::domain::Play {
            clock: "0:55".into(),
            team: "TB".into(),
            text: "from summary".into(),
            scoring: false,
        }];
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
    fn enter_zooms_and_esc_pops() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.view,
            View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview }
        );
        assert_eq!(app.effective_layout(), LayoutPref::One);
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        assert_eq!(app.effective_layout(), LayoutPref::Auto);
        // 'z' aliases Enter, and 'z' inside the zoom restores the board.
        app.on_key(KeyCode::Char('z'), KeyModifiers::NONE);
        assert!(matches!(app.view, View::Zoom { .. }));
        app.on_key(KeyCode::Char('z'), KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn tab_pops_zoom_and_home_hides_unpinned() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.view, View::Zoom { .. }));
        app.on_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home);
        assert_eq!(app.view, View::Board);
        let ids: Vec<_> = app.visible_games().into_iter().map(|g| g.id).collect();
        assert!(!ids.iter().any(|id| id == "1"));
    }

    #[test]
    fn zoom_tabs_cycle_with_hl_and_brackets_and_jk_clamp() {
        let mut game = g("1", "KC", "TB", true);
        game.last_plays = vec![
            Play { clock: "1:00".into(), team: "KC".into(), text: "a".into(), scoring: false },
            Play { clock: "2:00".into(), team: "TB".into(), text: "b".into(), scoring: false },
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
        use crate::theme::{self, ThemeName};
        theme::set_current(ThemeName::Broadcast);
        // Own dir: app_with's shared dir is also written by other tests' saves.
        let dir = std::env::temp_dir().join(format!("gd-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir);
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), ThemeName::Ceefax);
        assert_eq!(app.config.theme, "ceefax");
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.theme, "ceefax");
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), ThemeName::Broadcast);
        assert_eq!(app.config.theme, "broadcast");
    }

    #[test]
    fn layout_keys() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(app.config.layout, LayoutPref::Two);
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(app.config.layout, LayoutPref::Sidebar);
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
            layout: LayoutPref::Auto,
            favorites: vec![],
            theme: "broadcast".into(),
            score_style: Default::default(),
        };
        let app = App::new(cfg, vec![], dir);
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

    fn cells(s: &str) -> Vec<(char, Style)> {
        s.chars().map(|c| (c, Style::default())).collect()
    }

    fn window_text(cells: &[(char, Style)], width: usize, tick: u64) -> String {
        marquee_spans(cells, width, tick)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn marquee_is_static_when_content_fits() {
        let c = cells("SHORT");
        assert_eq!(window_text(&c, 10, 0), "SHORT");
        assert_eq!(window_text(&c, 10, 7), "SHORT", "no motion when it fits");
    }

    #[test]
    fn marquee_scrolls_one_cell_per_tick_and_wraps() {
        let c = cells("ABCDEFGHIJ"); // 10 cells, window 6, cycle 10+GAP=20
        assert_eq!(window_text(&c, 6, 0), "ABCDEF");
        assert_eq!(window_text(&c, 6, 1), "BCDEFG");
        assert_eq!(window_text(&c, 6, 4), "EFGHIJ", "tail scrolls into view");
        assert_eq!(window_text(&c, 6, 15), "     A", "gap, then the head wraps");
        assert_eq!(window_text(&c, 6, 20), "ABCDEF", "full cycle");
    }

    #[test]
    fn ticker_includes_every_scoring_event() {
        // 8 scoring plays: more than the old take(6) cap — all must be present
        // in the ticker content so the marquee can bring each into view.
        let mut game = g("1", "KC", "TB", true);
        game.last_plays = (0..8)
            .map(|i| Play {
                clock: format!("{i}:00"),
                team: "KC".into(),
                text: format!("score number {i}"),
                scoring: true,
            })
            .collect();
        let app = app_with(vec![game], vec![]);
        let rows = app.ticker_rows();
        let all: String = rows
            .iter()
            .flat_map(|r| r.iter().map(|(c, _)| *c))
            .collect();
        for i in 0..8 {
            let needle = format!("score number {i}");
            assert!(all.contains(&needle), "missing {needle:?} in ticker");
        }
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

    fn six_live() -> Vec<Game> {
        (0..6)
            .map(|i| g(&format!("g{i}"), "KC", "TB", true))
            .collect()
    }

    #[test]
    fn paging_wraps_instead_of_blanking() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        // Auto over 6 games resolves to Four => 2 pages.
        assert_eq!(app.page_count(), 2);
        app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.page, 1);
        assert_eq!(app.selected, 4, "selection lands on the page's first tile");
        app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.page, 0, "past the end wraps to page 0, never blank");
        app.on_key(KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(app.page, 1, "p from page 0 wraps to the last page");
        // PgUp/PgDn alias p/n.
        app.on_key(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(app.page, 0);
        app.on_key(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(app.page, 1);
    }

    #[test]
    fn moving_selection_pulls_the_page_along() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        for _ in 0..4 {
            app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        }
        assert_eq!(app.selected, 4);
        assert_eq!(app.page, 1, "page follows the selection");
        app.on_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.page, 0);
    }

    #[test]
    fn shrinking_board_clamps_the_page() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.page, 1);
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        assert_eq!(app.page, 0, "page index must never point past the end");
    }

    #[test]
    fn sidebar_paging_reaches_every_game_and_tracks_the_selection() {
        // Regression: pack()'s narrow branch used a height-based page size
        // while App paged by page_size() — games past page_count*8 were
        // unreachable and j/k could select an off-screen game.
        use crate::tiles::packer::pack;
        use ratatui::layout::Rect;
        let games: Vec<Game> = (0..16).map(|i| g(&format!("g{i}"), "KC", "TB", true)).collect();
        let mut app = app_with(games.clone(), vec![]);
        app.tab = Tab::League(League::Nfl);
        app.config.layout = LayoutPref::Sidebar;
        assert_eq!(app.page_count(), 2, "16 games / 8 per sidebar page");
        app.on_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!((app.page, app.selected), (1, 8));
        // pack agrees: page 1 exists and starts at the game App selected.
        let area = Rect::new(0, 0, 50, 30); // narrow branch (width < 60)
        let tiles = pack(&games, area, LayoutPref::Sidebar, app.page);
        assert!(!tiles.is_empty(), "page 1 must render tiles");
        assert_eq!(tiles[0].game.id, "g8", "pack's page 1 starts where App thinks it does");
        // j from the last tile of page 0 pulls the page to where the
        // selection actually renders.
        app.on_key(KeyCode::Char('p'), KeyModifiers::NONE);
        for _ in 0..8 {
            app.on_key(KeyCode::Char('j'), KeyModifiers::NONE);
        }
        assert_eq!(app.selected, 8);
        assert_eq!(app.page, 1, "page follows selection under Sidebar layout");
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
    fn r_requests_refresh_and_upd_age_formats() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(app.refresh_now);
        assert_eq!(age_label(0), "UPD 0s");
        assert_eq!(age_label(12), "UPD 12s");
        assert_eq!(age_label(59), "UPD 59s");
        assert_eq!(age_label(60), "UPD 1m");
        assert_eq!(age_label(150), "UPD 2m");
    }

    #[test]
    fn apply_boards_stamps_last_update() {
        let mut app = app_with(vec![], vec![]);
        assert!(app.last_update.is_some(), "app_with applies a board");
        app.last_update = None;
        app.apply_boards(League::Nba, vec![], false);
        assert!(app.last_update.is_some());
    }

    #[test]
    fn cfb_board_is_separate_from_nfl() {
        let dir = std::env::temp_dir().join(format!("gd-cfb2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(
            Config {
                enabled_tabs: vec![League::Nfl, League::Cfb],
                layout: LayoutPref::Auto,
                favorites: vec![],
                theme: "broadcast".into(),
            score_style: Default::default(),
            },
            vec![],
            dir,
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
