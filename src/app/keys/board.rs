//! Keys on the board: tab and selection movement, pin/favorite toggles,
//! the sort cycle, and the `[`/`]` date walk.

use crate::app::{App, Tab, DATE_TRAVEL_MAX_DAYS};
use crate::config::{Favorite, Pin};
use crate::domain::League;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
    pub(super) fn on_key_board(&mut self, code: KeyCode) {
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
            // n/p paging is deleted — the board is one scrolling
            // list, so PgDn/PgUp have nothing to page and n/p are free again.
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('v') => self.open_tv(),
            KeyCode::Char('c') => self.open_theme_picker(),
            KeyCode::Char('r') => self.refresh_now = true,
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    /// j/k: walk `Derived::selection` — one list, hero included, wrapping at
    /// both ends. The board scrolls itself to keep the selection visible
    /// (`board::first_visible`), so nothing here has a window to move.
    pub(in crate::app) fn move_selected(&mut self, delta: isize) {
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
            self.toast(format!("unpinned {matchup}"));
        } else {
            self.pins.push(Pin {
                game_id: game.id,
                league: game.league,
                final_at: None,
            });
            self.toast(format!("pinned {matchup}"));
        }
        self.persist_pins();
        // Membership in MY GAMES changed, so the game just entered or left
        // the ranked list: re-rank now. Without this the released game
        // re-enters `live_all` and `OrderState::ordered`'s append-unseen
        // fallback parks it *last* in IN PLAY until the next fresh apply
        // changes a fingerprint — the board reading as if it punished the
        // unpin. A user keystroke is an event under the same gate as
        // `s` or `:sort`, so the frozen-order rule is untouched.
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
            self.toast(format!(
                "unfavorited {} {abbr}",
                game.league.slug().to_uppercase()
            ));
        } else {
            self.toast(format!(
                "favorited {} {abbr}",
                game.league.slug().to_uppercase()
            ));
            self.config.favorites.push(Favorite {
                league: game.league,
                team_abbr: abbr,
            });
        }
        self.mark_favorites();
        self.persist_config();
        self.force_reorder(); // membership change — see `toggle_pin`
        self.clamp_selected();
    }

    /// 's': cycle WATCH → TIME → LEAGUE → WATCH and re-derive the order right
    /// away — a sort-key change is an event, not something the next score
    /// tick should gate.
    fn cycle_sort(&mut self) {
        let next = self.config.sort.cycled();
        self.config.sort = next;
        self.force_reorder();
        self.status_line = None;
        self.persist_config();
        self.report_save(format!("sort {}", next.label().to_ascii_lowercase()));
    }

    pub(super) fn cycle_tab(&mut self, delta: isize) {
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

    pub(in crate::app) fn clamp_selected(&mut self) {
        let n = self.selection_len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    /// The date `league`'s tab is viewing, None when it's live today.
    pub fn viewed_date(&self, league: League) -> Option<time::Date> {
        let off = self.viewed_date_offset.get(&league).copied().unwrap_or(0);
        if off == 0 {
            return None;
        }
        self.now()
            .date()
            .checked_add(time::Duration::days(off as i64))
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
}
