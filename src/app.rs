use crate::config::{prune_pins, save_pins, Config, Favorite, Pin};
use crate::domain::{Game, GameStats, League, StandingsTable, Status, Summary};
use crate::home::home_games;
use crate::input::{CompletionState, InputMode};
use crate::keymap;
use crate::net::{NetChip, NetStatus};
use crate::theme;
use crate::ticker;
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
    /// The local UTC offset, read once on the main thread at startup
    /// (`text::startup_offset`). Every clock the app renders goes through
    /// [`App::now`] so nothing calls `now_local()` off the main thread.
    pub offset: time::UtcOffset,
    /// Frozen clock: when set, [`App::now`] returns this instead of reading
    /// the wall clock. Dumps and draw tests set it so a capture of the same
    /// tick is the same pixels every run; the real app leaves it None.
    pub now_override: Option<OffsetDateTime>,
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
            page: 0,
            selected: 0,
            pins,
            config,
            boards: HashMap::new(),
            stats: HashMap::new(),
            standings: HashMap::new(),
            net: NetStatus::default(),
            should_quit: false,
            refresh_now: false,
            view: View::Board,
            zoom_scroll: 0,
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
            tick: 0,
            last_scores: HashMap::new(),
            flashes: HashMap::new(),
            alerts: crate::alerts::AlertState::default(),
            active_alert: None,
            bell_pending: false,
            hit_zones: Vec::new(),
            offset,
            now_override: None,
        }
    }

    /// Now, in the user's local offset — or the frozen clock when one is set.
    /// The one clock the app reads.
    pub fn now(&self) -> OffsetDateTime {
        self.now_override
            .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(self.offset))
    }

    /// The soonest scheduled start still ahead of us across the enabled
    /// boards — what empty Home names when nothing is live.
    pub fn next_start(&self) -> Option<Game> {
        let now = self.now();
        self.concat_boards()
            .into_iter()
            .filter(|g| g.status == Status::Pre && g.start.is_some_and(|s| s > now))
            .min_by_key(|g| g.start.expect("filtered to Some above"))
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
                home_games(&self.pins, &self.config.favorites, &concat, self.now())
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
            View::ConfigView => self.on_key_config(code),
            View::ThemePicker => self.on_key_theme_picker(code),
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
                let _ = self.config.save_to(&self.config_dir);
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
                    ConfigRow::Theme | ConfigRow::Score | ConfigRow::Layout => {
                        self.config_cycle(1)
                    }
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

    /// Space on a TABS row: toggle the league in `enabled_tabs` (appended at
    /// the end when re-enabled — the list's order is the tab order). If the
    /// current tab was just disabled, fall back to Home.
    fn config_toggle_tab(&mut self, league: League) {
        if let Some(i) = self.config.enabled_tabs.iter().position(|l| *l == league) {
            self.config.enabled_tabs.remove(i);
            if self.tab == Tab::League(league) {
                self.tab = Tab::Home;
            }
        } else {
            self.config.enabled_tabs.push(league);
        }
        let _ = self.config.save_to(&self.config_dir);
    }

    fn config_remove_favorite(&mut self, i: usize) {
        if i < self.config.favorites.len() {
            self.config.favorites.remove(i);
            let _ = self.config.save_to(&self.config_dir);
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
        let _ = self.config.save_to(&self.config_dir);
    }

    /// h/l on a display row: cycle its value and persist. No-op on rows that
    /// don't cycle.
    fn config_cycle(&mut self, delta: isize) {
        use crate::views::config_view::{rows, ConfigRow, LAYOUTS};
        let rows = rows(self);
        match rows[self.config_cursor.min(rows.len() - 1)] {
            ConfigRow::Theme => {
                let next = theme::next_name(&theme::current_name(), delta);
                let _ = theme::set_current(&next);
                self.config.theme = next;
            }
            ConfigRow::Score => {
                self.config.score_style = match self.config.score_style {
                    crate::tiles::ScoreStyle::Big => crate::tiles::ScoreStyle::Compact,
                    crate::tiles::ScoreStyle::Compact => crate::tiles::ScoreStyle::Big,
                };
            }
            ConfigRow::Layout => {
                let i = LAYOUTS
                    .iter()
                    .position(|l| *l == self.config.layout)
                    .unwrap_or(0) as isize;
                self.config.layout =
                    LAYOUTS[(i + delta).rem_euclid(LAYOUTS.len() as isize) as usize];
            }
            ConfigRow::Tab(_) | ConfigRow::Favorite(_) | ConfigRow::AddFavorite => return,
        }
        let _ = self.config.save_to(&self.config_dir);
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
                            g.scoring_plays.push(p);
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
        let _ = save_pins(&self.config_dir, &self.pins);
        self.clamp_selected();
    }

    pub fn merge_summary(&mut self, game_id: &str, summary: Summary) {
        // Non-football summaries carry no "drives", so they can map to zero
        // plays; keep the scoreboard's lastPlay instead of blanking the tile.
        if summary.last_plays.is_empty() && summary.scoring_plays.is_empty() {
            return;
        }
        for board in self.boards.values_mut() {
            if let Some(game) = board.iter_mut().find(|g| g.id == game_id) {
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
                return;
            }
        }
    }

    /// The zoomed game's (league, id) — the stats poll's only target. None
    /// unless the Zoom view is open and its game is still on a board.
    pub fn stats_target(&self) -> Option<(League, String)> {
        self.zoomed_game().map(|g| (g.league, g.id))
    }

    /// Latest box score for `game_id`, from the stats poll (or a fixture in
    /// tests/dump). Replaces wholesale — rows are a snapshot, not a delta.
    pub fn merge_stats(&mut self, game_id: &str, stats: GameStats) {
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
        let _ = self.config.save_to(&self.config_dir);
        self.clamp_selected();
    }

    fn set_layout(&mut self, layout: LayoutPref) {
        self.config.layout = layout;
        let _ = self.config.save_to(&self.config_dir);
    }

    /// 'c': step to the next loaded theme in picker order (built-ins, then
    /// user files), wrapping; persisted like layout.
    fn cycle_theme(&mut self) {
        let next = theme::next_name(&theme::current_name(), 1);
        let _ = theme::set_current(&next);
        self.status_line = Some(format!("theme {next}"));
        self.config.theme = next;
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
        let ticker_h = if area.height >= 24 { ticker::HEIGHT } else { 0 };
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
        let prefix_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let tabs: Vec<(Span, Tab)> = self
            .tab_list()
            .into_iter()
            .map(|tab| {
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
                (chip, tab)
            })
            .collect();
        let alert_len = self
            .active_alert
            .as_ref()
            .map(|a| a.text.chars().count() + 2)
            .unwrap_or(0);
        // The connection chip is its own cell, padded on both sides so it can
        // never read as a suffix of the date ("OFFLINEMON SEP 1"). It is the
        // one thing in the header that must never be chopped mid-word: it
        // degrades — padded label, then the bare state word, then no padding
        // — and only if none of those fit do trailing league chips give way
        // (never past the selected tab; Task 13 makes shedding orderly).
        let net = self.net.chip(Instant::now());
        let chip_forms: Vec<String> = match (net.label(), net.short_label()) {
            (Some(full), Some(bare)) => vec![format!("  {full}  "), format!(" {bare} "), bare],
            _ => Vec::new(),
        };
        let tab_cells = |n: usize| -> usize {
            tabs.iter()
                .take(n)
                .map(|(s, _)| s.content.chars().count() + 1)
                .sum()
        };
        let selected_idx = tabs.iter().position(|(_, t)| *t == self.tab).unwrap_or(0);
        let width = area.width as usize;
        let mut kept = tabs.len();
        let mut chip_text: Option<String> = None;
        while !chip_forms.is_empty() {
            let used = prefix_len + tab_cells(kept) + alert_len;
            chip_text = chip_forms
                .iter()
                .find(|f| used + f.chars().count() <= width)
                .cloned();
            if chip_text.is_some() {
                break;
            }
            if kept <= selected_idx + 1 {
                // Nothing fits even with the bar cut back to the selected
                // tab: keep the tabs, drop the chip. Never a half word.
                kept = tabs.len();
                break;
            }
            kept -= 1;
        }
        for (chip, tab) in tabs.into_iter().take(kept) {
            // Register the chip as a click zone at its rendered columns (the
            // header is all single-width chars, so chars == cells).
            let x: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            let w = chip.content.chars().count();
            if x + w <= width {
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
        // Favorite-score banner: earned red — the live role, spec's color
        // discipline — for its short lifetime, then advance_tick drops it.
        if let Some(alert) = &self.active_alert {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                alert.text.clone(),
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            ));
        }
        let now = self.now();
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
        let clock = crate::text::fmt_clock12(now);
        let chip_span = chip_text.map(|text| {
            let color = match net {
                NetChip::NoDataYet => th.muted,
                NetChip::Stale { .. } => th.star,
                // Offline is the one failure the board can have; it earns the
                // live role's red for as long as it lasts.
                NetChip::Offline { .. } => th.live,
                NetChip::Live => th.muted,
            };
            Span::styled(text, Style::default().fg(color).add_modifier(Modifier::BOLD))
        });
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let chip_len = chip_span
            .as_ref()
            .map(|s| s.content.chars().count())
            .unwrap_or(0);
        // A full tab bar can leave no room for all three. The date is what
        // goes: an outage chip and the clock both say something the rest of
        // the screen doesn't.
        let date_len = date.chars().count() + 2;
        let clock_len = clock.len() + 1;
        // Dropped in priority order rather than truncated mid-word: the chip
        // outranks the date (a traveled date is a claim about what you are
        // looking at), which outranks the clock.
        let show_clock = left_len + chip_len + date_len + clock_len <= width;
        let show_date = left_len + chip_len + date_len <= width;
        let right_len = chip_len
            + if show_date { date_len } else { 0 }
            + if show_clock { clock_len } else { 0 };
        let spacer = width.saturating_sub(left_len + right_len);
        spans.push(Span::raw(" ".repeat(spacer)));
        if let Some(s) = chip_span {
            spans.push(s);
        }
        if show_date {
            spans.push(Span::styled(date, date_style));
            spans.push(Span::raw("  "));
        }
        if show_clock {
            spans.push(Span::styled(
                clock,
                Style::default().fg(th.clock()).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" "));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    /// Scoring plays across every enabled board, newest first per game,
    /// games in board order. Finals keep theirs until they leave the board.
    pub(crate) fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let mut out = Vec::new();
        for game in self.concat_boards() {
            for play in game.scoring_plays.iter().rev() {
                out.push((game.clone(), play.clone()));
            }
        }
        out
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
            pinned: self.pins.iter().any(|p| p.game_id == game.id),
            favorite: self.config.favorites.iter().any(|f| {
                f.league == game.league
                    && (f.team_abbr.eq_ignore_ascii_case(&game.away.abbr)
                        || f.team_abbr.eq_ignore_ascii_case(&game.home.abbr))
            }),
            now: self.now(),
        }
    }

    /// Every live game across the enabled boards, league order — the ticker
    /// covers what the visible tab (or a traveled date) does not. A typed
    /// filter is explicit intent, so it narrows the ticker too.
    fn ticker_live(&self) -> Vec<Game> {
        let needle = self.active_filter();
        self.concat_boards()
            .into_iter()
            .filter(|g| g.status == Status::Live)
            .filter(|g| needle.is_none_or(|n| game_matches(g, n)))
            .collect()
    }

    /// Scoring plays of the ticker's live games, board order.
    fn ticker_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let live = self.ticker_live();
        self.scoring_events()
            .into_iter()
            .filter(|(g, _)| live.iter().any(|l| l.id == g.id))
            .collect()
    }

    fn draw_ticker(&self, frame: &mut Frame, area: Rect) {
        ticker::draw(frame, area, &self.ticker_live(), &self.ticker_events(), self.tick);
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
        // every other view advertises the way back (zoom its tab cycle, the
        // config editor its toggle/edit/cycle verbs, the feeds just the
        // shared chords — they have no tabs to cycle).
        let zoomed = self.view != View::Board;
        let ctx = match self.view {
            View::Board => keymap::FooterCtx::Board,
            View::ConfigView => keymap::FooterCtx::Config,
            View::Zoom { .. } => keymap::FooterCtx::Zoomed,
            View::PlaysFeed | View::Standings(_) | View::ThemePicker => keymap::FooterCtx::Feed,
        };
        // Narrow terminals can't hold every chord: shed the low-value ones in
        // keymap's declared order so HELP and QUIT are never the ones clipped.
        let mut chords = keymap::footer_chords(ctx);
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
        // The UPD age freezes and dims the moment the data stops arriving —
        // `net` marks the frozen label with a trailing "·" so a stale number
        // can't pass for a live one.
        if let Some(upd) = self.net.upd_label(Instant::now()) {
            right.push(upd);
        }
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let width = area.width as usize;
        while !right.is_empty() && left_len + right.join("  ").chars().count() + 2 > width {
            right.remove(0);
        }
        if !right.is_empty() {
            let text_len = right.join("  ").chars().count();
            let spacer = width.saturating_sub(left_len + text_len + 1);
            spans.push(Span::raw(" ".repeat(spacer)));
            for (i, part) in right.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                // GAME/PAGE/UPD is status, clock-shaped: it takes the clocks
                // discipline (cyan on broadcast, muted on studio), never raw
                // cyan — except a frozen UPD, which drops to dim.
                let color = if part.ends_with('·') { th.dim } else { th.clock() };
                spans.push(Span::styled(part.clone(), Style::default().fg(color)));
            }
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
            ..Game::default()
        }
    }

    fn app_with(games: Vec<Game>, pins: Vec<Pin>) -> App {
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
            ..Default::default()
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
            ..Default::default()
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
        assert!(matches!(app.net.chip(now), crate::net::NetChip::Live));
        assert!(app.net.upd_label(now).is_some(), "app_with applies a board");
        app.apply_boards(League::Nba, vec![], true);
        assert!(
            matches!(app.net.chip(now), crate::net::NetChip::Stale { .. }),
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
                layout: LayoutPref::Auto,
                favorites: vec![],
                theme: "broadcast".into(),
            score_style: Default::default(),
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
