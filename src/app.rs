use crate::config::{prune_pins, save_pins, Config, Favorite, Pin};
use crate::domain::{Game, League, Status, Summary};
use crate::home::home_games;
use crate::theme;
use crate::tiles::packer::{pack, LayoutPref};
use crate::tiles::{render_tile, TileFx};
use crossterm::event::KeyCode;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::HashMap;
use std::path::PathBuf;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    League(League),
}

/// Render ticks a score flash stays lit: 10 ticks ≈ 1s at the live cadence
/// (10 render ticks per second while anything is live).
pub const FLASH_TICKS: u64 = 10;

/// LIVE chip pulse phase, pure in the tick: ~1s bright then ~1s dim at the
/// 10 ticks/s live cadence. A luminance step, never a hue change.
pub fn live_pulse_bright(tick: u64) -> bool {
    (tick / 10) % 2 == 0
}

pub struct App {
    pub tab: Tab,
    pub page: usize,
    pub selected: usize,
    pub pins: Vec<Pin>,
    pub config: Config,
    pub boards: HashMap<League, Vec<Game>>,
    pub stale: bool,
    pub should_quit: bool,
    pub refresh_now: bool,
    pub focused_id: Option<String>,
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
            stale: false,
            should_quit: false,
            refresh_now: false,
            focused_id: None,
            config_dir,
            tick: 0,
            last_scores: HashMap::new(),
            flashes: HashMap::new(),
        }
    }

    /// One render tick. Expired flashes are dropped here, so a settled score
    /// never re-flashes (one-shot).
    pub fn advance_tick(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        self.flashes
            .retain(|_, start| tick.saturating_sub(*start) < FLASH_TICKS);
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
        match self.tab {
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
            Tab::League(league) => self.boards.get(&league).cloned().unwrap_or_default(),
        }
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

    pub fn on_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Tab | KeyCode::Char('l') => self.cycle_tab(1),
            KeyCode::Char('h') => self.cycle_tab(-1),
            KeyCode::Char('j') => self.move_selected(1),
            KeyCode::Char('k') => self.move_selected(-1),
            KeyCode::Char(' ') => self.toggle_pin(),
            KeyCode::Enter => {
                if let Some(game) = self.selected_game() {
                    self.focused_id = Some(game.id);
                }
            }
            KeyCode::Esc => self.focused_id = None,
            KeyCode::Char('t') => self.toggle_favorite(),
            KeyCode::Char('n') => {
                self.page = self.page.saturating_add(1);
                self.selected = 0;
            }
            KeyCode::Char('p') => {
                self.page = self.page.saturating_sub(1);
                self.selected = 0;
            }
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
        self.stale = stale;
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

    pub fn poll_plan(&self) -> crate::poll::PollPlan {
        crate::poll::plan(&self.visible_for_poll(), &self.config.enabled_tabs)
    }

    pub fn effective_layout(&self) -> LayoutPref {
        if self.focused_id.is_some() {
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

    fn selection_list(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let live = self.live_games();
                if live.is_empty() {
                    self.slate_games()
                } else {
                    live
                }
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
    }

    fn cycle_tab(&mut self, delta: isize) {
        let tabs = self.tab_list();
        if tabs.is_empty() {
            return;
        }
        let n = tabs.len() as isize;
        let next = (self.current_tab_index() as isize + delta).rem_euclid(n) as usize;
        self.tab = tabs[next];
        self.page = 0;
        self.selected = 0;
        self.focused_id = None;
    }

    fn move_selected(&mut self, delta: isize) {
        let n = self.selection_list().len();
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
        self.draw_body(frame, chunks[1]);
        if ticker_h > 0 {
            self.draw_ticker(frame, chunks[2]);
        }
        self.draw_footer(frame, chunks[3]);
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
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
            if tab == self.tab {
                spans.push(Span::styled(
                    format!("[{label}]"),
                    Style::default()
                        .fg(th.bg)
                        .bg(th.star)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    format!("[ {label} ]"),
                    Style::default().fg(th.muted),
                ));
            }
            spans.push(Span::raw(" "));
        }
        if self.stale {
            spans.push(Span::styled(" STALE", Style::default().fg(th.star)));
        }
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let date = format!(
            "{} {} {} {}",
            &format!("{:?}", now.weekday()).to_uppercase()[..3],
            &format!("{:?}", now.month()).to_uppercase()[..3],
            now.day(),
            now.year()
        );
        let (h12, ampm) = match now.hour() {
            0 => (12, "AM"),
            h if h < 12 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        let clock = format!("{}:{:02}:{:02} {}", h12, now.minute(), now.second(), ampm);
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let right_len = date.len() + 2 + clock.len() + 1;
        let spacer = (area.width as usize).saturating_sub(left_len + right_len);
        spans.push(Span::raw(" ".repeat(spacer)));
        spans.push(Span::styled(date, Style::default().fg(th.green).add_modifier(Modifier::BOLD)));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(clock, Style::default().fg(th.cyan).add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    fn draw_body(&self, frame: &mut Frame, area: Rect) {
        let main = if area.width >= 100 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(60), Constraint::Length(22)])
                .split(area);
            self.draw_sidebar(frame, cols[1]);
            cols[0]
        } else {
            area
        };
        let show_slate = matches!(self.tab, Tab::League(_)) && main.height >= 24;
        let mosaic = if show_slate {
            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(7)])
                .split(main);
            self.draw_slate(frame, parts[1]);
            parts[0]
        } else {
            main
        };
        self.draw_mosaic(frame, mosaic, show_slate);
    }

    /// Scoring plays across every visible board, newest-ish first: (game, play).
    fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
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

    fn team_color(game: &Game, abbr: &str) -> ratatui::style::Color {
        let th = theme::current();
        if game.away.abbr.eq_ignore_ascii_case(abbr) {
            theme::rgb(game.away.color)
        } else if game.home.abbr.eq_ignore_ascii_case(abbr) {
            theme::rgb(game.home.color)
        } else {
            th.fg
        }
    }

    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let events = self.scoring_events();
        let w = inner.width as usize;
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            "⚑ GLOBAL ALERTS",
            Style::default().fg(th.live).add_modifier(Modifier::BOLD),
        )));
        if events.is_empty() {
            lines.push(Line::from(Span::styled("no alerts", Style::default().fg(th.dim))));
        }
        for (game, play) in events.iter().take(4) {
            let word = theme::scoring_word(game.league);
            let label = format!("{:<4}{:<11}", play.team, word);
            let clock = &play.clock;
            let pad = w.saturating_sub(label.chars().count() + clock.len());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<4}", play.team),
                    Style::default().fg(Self::team_color(game, &play.team)).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{word:<11}"), Style::default().fg(th.live)),
                Span::raw(" ".repeat(pad)),
                Span::styled(clock.clone(), Style::default().fg(th.cyan)),
            ]));
        }
        lines.push(rule(w));

        lines.push(Line::from(Span::styled(
            "TOP PLAYS",
            Style::default().fg(th.star).add_modifier(Modifier::BOLD),
        )));
        for (game, play) in events.iter().take(5) {
            let text: String = play.text.chars().take(w.saturating_sub(2)).collect();
            lines.push(Line::from(vec![
                Span::styled("★ ", Style::default().fg(th.star)),
                Span::styled(text, Style::default().fg(th.league_accent(game.league))),
            ]));
        }
        if events.is_empty() {
            lines.push(Line::from(Span::styled("no scoring yet", Style::default().fg(th.dim))));
        }
        lines.push(rule(w));

        lines.push(Line::from(Span::styled(
            "RECORDS",
            Style::default().fg(th.magenta).add_modifier(Modifier::BOLD),
        )));
        let mut teams: Vec<&crate::domain::Team> = Vec::new();
        let games = self.visible_games();
        for g in &games {
            teams.push(&g.away);
            teams.push(&g.home);
        }
        let mut rows: Vec<(&crate::domain::Team, u32, u32)> = teams
            .into_iter()
            .filter_map(|t| {
                let mut parts = t.record.split('-');
                let win: u32 = parts.next()?.trim().parse().ok()?;
                let loss: u32 = parts.next()?.trim().parse().ok()?;
                Some((t, win, loss))
            })
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        lines.push(Line::from(Span::styled(
            format!("{:<12}{:>3}{:>3}", "TEAM", "W", "L"),
            Style::default().fg(th.muted),
        )));
        for (i, (team, win, loss)) in rows.iter().take(6).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(th.muted)),
                Span::styled(
                    format!("{:<9}", team.name.chars().take(9).collect::<String>()),
                    Style::default().fg(theme::rgb(team.color)),
                ),
                Span::styled(format!("{win:>3}{loss:>3}"), Style::default().fg(th.fg)),
            ]));
        }
        lines.truncate(inner.height as usize);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn mosaic_games(&self, show_slate: bool) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let live = self.live_games();
                if !live.is_empty() {
                    live
                } else if show_slate {
                    Vec::new()
                } else {
                    self.slate_games()
                }
            }
        }
    }

    fn focused_game(&self) -> Option<Game> {
        let id = self.focused_id.as_ref()?;
        match self.tab {
            Tab::Home => self.visible_games().into_iter().find(|g| g.id == *id),
            Tab::League(league) => self
                .boards
                .get(&league)
                .into_iter()
                .flatten()
                .find(|g| g.id == *id)
                .cloned(),
        }
    }

    /// Per-tile animation state: pure in (tick, flash table) so a dump at a
    /// fixed tick always renders the same frame.
    fn tile_fx(&self, game: &Game) -> TileFx {
        TileFx {
            flash: self.flash_active(&game.id),
            live_bright: live_pulse_bright(self.tick),
        }
    }

    fn draw_mosaic(&self, frame: &mut Frame, area: Rect, show_slate: bool) {
        let th = theme::current();
        if let Some(game) = self.focused_game() {
            let fx = self.tile_fx(&game);
            let one = [game];
            for tile in pack(&one, area, LayoutPref::One, 0) {
                render_tile(frame, tile.area, tile.game, tile.density, true, fx);
            }
            return;
        }

        let games = self.mosaic_games(show_slate);
        match self.tab {
            Tab::Home if games.is_empty() => {
                frame.render_widget(
                    Paragraph::new("pin a game from nfl (space) · t fav home")
                        .style(Style::default().fg(th.muted).bg(th.bg))
                        .alignment(Alignment::Center),
                    area,
                );
                return;
            }
            Tab::League(_) if self.visible_games().is_empty() => {
                frame.render_widget(
                    Paragraph::new("next kickoff")
                        .style(Style::default().fg(th.muted).bg(th.bg))
                        .alignment(Alignment::Center),
                    area,
                );
                return;
            }
            _ => {}
        }

        let packed = pack(&games, area, self.effective_layout(), self.page);
        let start = packed
            .first()
            .and_then(|tile| games.iter().position(|g| g.id == tile.game.id))
            .unwrap_or(0);
        for (i, tile) in packed.iter().enumerate() {
            let selected = start + i == self.selected || i == self.selected;
            render_tile(
                frame,
                tile.area,
                tile.game,
                tile.density,
                selected,
                self.tile_fx(tile.game),
            );
        }
    }

    fn draw_slate(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th.dim))
            .title(Span::styled(
                " SLATE ",
                Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines: Vec<Line> = self
            .slate_games()
            .iter()
            .map(|g| {
                Line::from(Span::styled(
                    slate_line(g),
                    Style::default().fg(th.muted),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(th.bg).fg(th.muted)),
            inner,
        );
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

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        // "SPC" and single-space separators keep the full list within 120 cols.
        let chords: &[(&str, &str)] = &[
            ("TAB", "LEAGUE"),
            ("J/K", "MOVE"),
            ("SPC", "PIN"),
            ("ENTER", "FOCUS"),
            ("N/P", "PAGE"),
            ("T", "FAV"),
            ("1/2/4/S", "LAYOUT"),
            ("C", "THEME"),
            ("R", "REFRESH"),
            ("Q", "QUIT"),
        ];
        let mut spans = vec![Span::styled(
            " NAV:",
            Style::default().fg(th.fg).add_modifier(Modifier::BOLD),
        )];
        for (key, action) in chords {
            spans.push(Span::styled(format!(" [{key}]"), Style::default().fg(th.fg)));
            spans.push(Span::styled(format!(" {action}"), Style::default().fg(th.muted)));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }
}

fn rule(width: usize) -> Line<'static> {
    let th = theme::current();
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(th.dim),
    ))
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

fn slate_line(game: &Game) -> String {
    match game.status {
        Status::Pre => {
            let mut line = format!("{} @ {}", game.away.abbr, game.home.abbr);
            if let Some(broadcast) = &game.broadcast {
                line.push_str("  ");
                line.push_str(broadcast);
            }
            if let Some(start) = &game.start_time {
                line.push_str("  ");
                line.push_str(start);
            }
            line
        }
        Status::Final => format!(
            "{} {}  {} {}  F",
            game.away.abbr, game.away_score, game.home.abbr, game.home_score
        ),
        Status::Live => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Favorite, Pin};
    use crate::domain::*;
    use crate::tiles::packer::LayoutPref;
    use crossterm::event::KeyCode;

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
    fn space_toggles_pin() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char(' '));
        assert_eq!(app.pins.len(), 1);
        assert_eq!(app.pins[0].game_id, "1");
        app.on_key(KeyCode::Char(' '));
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
        app.on_key(KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn tab_cycles_home_then_nfl() {
        let mut app = app_with(vec![], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        assert_eq!(app.tab, Tab::Home);
        app.on_key(KeyCode::Tab);
        assert_eq!(app.tab, Tab::League(League::Nfl));
        app.on_key(KeyCode::Tab);
        assert_eq!(app.tab, Tab::Home);
    }

    #[test]
    fn t_favorites_home_team() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char('t'));
        assert_eq!(
            app.config.favorites,
            vec![Favorite {
                league: League::Nfl,
                team_abbr: "TB".into()
            }]
        );
        app.on_key(KeyCode::Char('t'));
        assert!(app.config.favorites.is_empty());
    }

    #[test]
    fn enter_focuses_and_esc_clears() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter);
        assert_eq!(app.focused_id.as_deref(), Some("1"));
        assert_eq!(app.effective_layout(), LayoutPref::One);
        app.on_key(KeyCode::Esc);
        assert!(app.focused_id.is_none());
        assert_eq!(app.effective_layout(), LayoutPref::Auto);
    }

    #[test]
    fn tab_clears_focus_and_home_hides_unpinned() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.config.enabled_tabs = vec![League::Nfl];
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Enter);
        assert_eq!(app.focused_id.as_deref(), Some("1"));
        app.on_key(KeyCode::Tab);
        assert_eq!(app.tab, Tab::Home);
        assert!(app.focused_id.is_none());
        let ids: Vec<_> = app.visible_games().into_iter().map(|g| g.id).collect();
        assert!(!ids.iter().any(|id| id == "1"));
    }

    #[test]
    fn c_cycles_theme_and_persists() {
        use crate::theme::{self, ThemeName};
        theme::set_current(ThemeName::Broadcast);
        // Own dir: app_with's shared dir is also written by other tests' saves.
        let dir = std::env::temp_dir().join(format!("gd-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir);
        app.on_key(KeyCode::Char('c'));
        assert_eq!(theme::current_name(), ThemeName::Ceefax);
        assert_eq!(app.config.theme, "ceefax");
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.theme, "ceefax");
        app.on_key(KeyCode::Char('c'));
        app.on_key(KeyCode::Char('c'));
        assert_eq!(theme::current_name(), ThemeName::Broadcast);
        assert_eq!(app.config.theme, "broadcast");
    }

    #[test]
    fn layout_keys() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('2'));
        assert_eq!(app.config.layout, LayoutPref::Two);
        app.on_key(KeyCode::Char('s'));
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
            app.on_key(key);
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
    fn cfb_board_is_separate_from_nfl() {
        let dir = std::env::temp_dir().join(format!("gd-cfb2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(
            Config {
                enabled_tabs: vec![League::Nfl, League::Cfb],
                layout: LayoutPref::Auto,
                favorites: vec![],
                theme: "broadcast".into(),
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
