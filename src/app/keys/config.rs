//! Keys in the Config view: the row cursor, the tab and favorite toggles,
//! the favorite-abbr editor, and the h/l cyclers.

use crate::app::{App, Tab};
use crate::config::Favorite;
use crate::domain::League;
use crate::theme;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
    /// Keys in the Config view: j/k move the row cursor, space/enter activate
    /// (toggle a tab, remove a favorite, open the abbr editor), h/l cycle the
    /// display rows, Esc/q pop to the board. An open abbr editor captures
    /// everything first (Enter commits, Esc cancels).
    pub(super) fn on_key_config(&mut self, code: KeyCode) {
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

    pub(in crate::app) fn move_config_cursor(&mut self, delta: isize) {
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
            self.mark_favorites();
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
        self.mark_favorites();
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
}
