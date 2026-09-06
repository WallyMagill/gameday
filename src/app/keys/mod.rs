//! Key handling, one file per view. `on_key` is the dispatcher `input.rs`
//! calls; each view's handler and its cursor helpers live beside it.

mod board;
mod config;
mod feed;
mod standings;
mod theme;
mod tv;
mod zoom;

use super::App;
use crate::views::View;
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    pub fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // Raw mode swallows SIGINT, so Ctrl+C must be an explicit quit.
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // Help is modal: Esc, '?' and 'q' all close it — 'q' inside help
        // dismisses the overlay, never the app (quitting from behind a
        // modal you opened by accident is the wrong surprise). Ctrl+C above
        // is still the escape hatch. Everything else is inert while open.
        if self.help_open {
            match code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.help_open = false,
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
            View::Tv => self.on_key_tv(code),
        }
    }
}
