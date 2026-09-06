//! Keys inside the zoomed game view: tab cycling, the Plays/Stats
//! highlight, and the two ways in (`z`/Enter on the board, the band's jump).

use crate::app::App;
use crate::views::{View, ZoomTab};
use crossterm::event::KeyCode;

impl App {
    /// Keys inside the zoomed view: h/l and [/] cycle the tab, j/k move the
    /// Plays highlight, Esc/q/z pop back to the board (q quits ONLY from the
    /// board), Tab still switches league tabs (which pops the zoom).
    pub(super) fn on_key_zoom(&mut self, code: KeyCode) {
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
    pub(super) fn zoom_selected(&mut self) {
        if let Some(game) = self.selected_game() {
            self.zoom_game_id(&game.id);
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
    pub(in crate::app) fn move_zoom_scroll(&mut self, delta: isize) {
        let len = match &self.view {
            View::Zoom {
                game_id,
                tab: ZoomTab::Stats,
            } => self.stats.get(game_id).map(|s| s.rows.len()).unwrap_or(0),
            _ => self.zoomed_game().map(|g| g.last_plays.len()).unwrap_or(0),
        };
        if len == 0 {
            self.zoom_scroll = 0;
            return;
        }
        let next = self.zoom_scroll as isize + delta;
        self.zoom_scroll = next.clamp(0, len as isize - 1) as usize;
    }

    /// Zoom a game by id, opening on Overview. The selection is not the only
    /// way in any more: the band's `enter` jump (spec v3.3 §3) names the game
    /// that just scored, which is rarely the one under the cursor.
    pub(crate) fn zoom_game_id(&mut self, game_id: &str) {
        self.view = View::Zoom {
            game_id: game_id.to_string(),
            tab: ZoomTab::Overview,
        };
        self.zoom_scroll = 0;
    }
}
