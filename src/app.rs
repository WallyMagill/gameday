use crate::config::{prune_pins, save_pins, Config, Favorite, Pin};
use crate::domain::{Game, League, Status, Summary};
use crate::home::home_games;
use crate::tiles::packer::LayoutPref;
use crossterm::event::KeyCode;
use std::collections::HashMap;
use std::path::PathBuf;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    League(League),
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
        }
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
            KeyCode::Char('r') => self.refresh_now = true,
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    pub fn apply_boards(&mut self, league: League, games: Vec<Game>, stale: bool) {
        let now = OffsetDateTime::now_utc();
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
        if let Some(idx) = self.config.favorites.iter().position(|f| {
            f.league == game.league && f.team_abbr.eq_ignore_ascii_case(&abbr)
        }) {
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
            id: abbr.into(), abbr: abbr.into(), name: abbr.into(),
            color: [1, 2, 3], alt_color: [0, 0, 0],
            logo_key: format!("nfl/{}", abbr.to_lowercase()),
        }
    }

    fn g(id: &str, away: &str, home: &str, live: bool) -> Game {
        Game {
            id: id.into(), league: League::Nfl,
            away: team(away), home: team(home),
            away_score: 7, home_score: 3,
            status: if live { Status::Live } else { Status::Pre },
            period: "Q2".into(), clock: "5:00".into(),
            situation: None, last_plays: vec![], meter: None,
            start_time: None, broadcast: None,
        }
    }

    fn app_with(games: Vec<Game>, pins: Vec<Pin>) -> App {
        let dir = std::env::temp_dir().join(format!("gd-app-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_nfl(), pins, dir);
        app.apply_boards(League::Nfl, games, false);
        app
    }

    #[test]
    fn home_shows_only_pinned() {
        let app = app_with(
            vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
            vec![Pin { game_id: "1".into(), league: League::Nfl, final_at: None }],
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
    fn q_quits() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn tab_cycles_home_then_nfl() {
        let mut app = app_with(vec![], vec![]);
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
        assert_eq!(app.config.favorites, vec![Favorite { league: League::Nfl, team_abbr: "TB".into() }]);
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
    fn layout_keys() {
        let mut app = app_with(vec![], vec![]);
        app.on_key(KeyCode::Char('2'));
        assert_eq!(app.config.layout, LayoutPref::Two);
        app.on_key(KeyCode::Char('s'));
        assert_eq!(app.config.layout, LayoutPref::Sidebar);
    }

    #[test]
    fn apply_boards_stamps_final_at() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![
            Pin { game_id: "1".into(), league: League::Nfl, final_at: None },
        ]);
        let mut done = g("1", "KC", "TB", false);
        done.status = Status::Final;
        app.apply_boards(League::Nfl, vec![done], false);
        assert!(app.pins[0].final_at.is_some());
    }
}
