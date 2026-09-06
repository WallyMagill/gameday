//! Keys in the Standings view: the table scroll and the pops.

use super::feed::FEED_PAGE_JUMP;
use crate::app::App;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
    /// Keys in the Standings view: j/k scroll the table one line, PgUp/PgDn
    /// jump, Tab switches league (landing on the board, as in Zoom), Esc/q
    /// pop back to the board (q quits ONLY there).
    pub(super) fn on_key_standings(&mut self, code: KeyCode) {
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
    /// lands on the last pane row (`standings_max_scroll`, recorded by the
    /// last draw) — the stored value never runs past what is shown, so `k`
    /// after the bottom moves the table on the first press. Before any draw
    /// the pane is unknown and the clamp falls back to the line count.
    pub(in crate::app) fn move_standings_scroll(&mut self, delta: isize) {
        let lines = self
            .standings_target()
            .and_then(|l| self.standings.get(&l))
            .map(crate::views::standings::line_count)
            .unwrap_or(0);
        if lines == 0 {
            self.standings_scroll = 0;
            return;
        }
        let max = self.standings_max_scroll.unwrap_or(lines - 1);
        let next = self.standings_scroll as isize + delta;
        self.standings_scroll = next.clamp(0, max as isize) as usize;
    }
}
