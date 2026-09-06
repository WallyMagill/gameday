//! Keys in the theme picker, plus `c`'s one-press theme cycle. Both live
//! here because both are the same preview-then-persist dance.

use crate::app::App;
use crate::theme;
use crate::views::View;
use crossterm::event::KeyCode;

impl App {
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
    pub(super) fn on_key_theme_picker(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.move_theme_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_theme_cursor(-1),
            KeyCode::Enter => {
                self.config.theme = theme::current_name();
                self.persist_config();
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

    pub(in crate::app) fn revert_theme_preview(&mut self) {
        // The prior theme is always a loaded name (it was current); if a
        // user file vanished mid-session, broadcast is the honest fallback.
        if theme::set_current(&self.theme_prior).is_err() {
            let _ = theme::set_current("broadcast");
        }
    }

    /// Move the picker cursor `delta` rows (wrapping) and preview that theme.
    pub(in crate::app) fn move_theme_cursor(&mut self, delta: isize) {
        let names = theme::names();
        let n = names.len() as isize;
        self.theme_cursor = (self.theme_cursor as isize + delta).rem_euclid(n) as usize;
        let _ = theme::set_current(&names[self.theme_cursor]);
    }

    /// 'c': step to the next loaded theme in picker order (built-ins, then
    /// user files), wrapping; persisted like layout.
    pub(super) fn cycle_theme(&mut self) {
        let next = theme::next_name(&theme::current_name(), 1);
        let _ = theme::set_current(&next);
        self.config.theme = next.clone();
        // One keypress, one line. `persist_config` writes its own refusal
        // toast, and the old order let "theme X" overwrite it — so the toast
        // is composed here, after the save, and says both halves.
        self.status_line = None;
        self.persist_config();
        let save_error = self.status_line.take();
        self.status_line = Some(match (&self.config_error, save_error) {
            (Some(_), _) => format!("theme {next} · not saving (config error)"),
            (None, Some(err)) => err,
            (None, None) => format!("theme {next}"),
        });
    }
}
