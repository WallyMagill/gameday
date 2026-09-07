//! Keys in the global PlaysFeed: the highlight walk. PgUp/PgDn/g/G page it
//! (see `keys::mod`'s paging dispatch, which runs before `on_key_plays_feed`
//! ever sees them).

use crate::app::App;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
    /// Keys inside the global PlaysFeed: j/k move the highlight one row,
    /// Tab switches league (landing on the board), Esc/q pop back to the
    /// board (q quits ONLY there).
    pub(super) fn on_key_plays_feed(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_feed_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_feed_scroll(-1),
            KeyCode::Esc | KeyCode::Char('q') => self.view = View::Board,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('r') => self.refresh_now = true,
            _ => {}
        }
    }

    /// j/k move the highlight one row; PgUp/PgDn/g/G (dispatched from
    /// `keys::mod`) page it by a whole delta. Clamped to the current
    /// scoring-event list (the renderer re-clamps if boards shrink between a
    /// keypress and the next draw).
    pub(in crate::app) fn move_feed_scroll(&mut self, delta: isize) {
        let len = self.scoring_events().len();
        if len == 0 {
            self.feed_scroll = 0;
            return;
        }
        let next = self.feed_scroll as isize + delta;
        self.feed_scroll = next.clamp(0, len as isize - 1) as usize;
    }
}
