//! Key handling, one file per view. `on_key` is the dispatcher `input.rs`
//! calls; each view's handler and its cursor helpers live beside it.

mod board;
mod config;
mod feed;
mod standings;
mod theme;
mod tv;
mod zoom;

use super::{App, PAGE_ROWS_FALLBACK};
use crate::views::{View, ZoomTab};
use crossterm::event::{KeyCode, KeyModifiers};

/// Far enough that every mover clamps to its end, small enough that
/// `scroll as isize + FAR` cannot overflow.
const FAR: isize = isize::MAX / 4;

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
        // Paging is one gesture in every scrolling view: half of what the
        // last frame showed, never a wrap; the ends are the ends. Ctrl-d/u
        // are the vim spellings. The config editor is not a list (and its
        // favorite prompt types g's), TV and the picker have nothing to
        // page. Zoom's OVERVIEW is a fixed layout, not a list either — only
        // its PLAYS/STATS tabs scroll, so OVERVIEW is excluded even though
        // the view itself is `View::Zoom`.
        let pageable = match self.view {
            View::Board | View::PlaysFeed | View::Standings(_) => true,
            View::Zoom { tab, .. } => tab != ZoomTab::Overview,
            View::ConfigView | View::ThemePicker | View::Tv => false,
        };
        if pageable {
            let ctrl = mods.contains(KeyModifiers::CONTROL);
            // `.max(2)`: `page_rows` can be `Some(0)` for an empty pane (zoom
            // PLAYS/STATS with nothing to list yet) — half of nothing must
            // still be one row, not zero.
            let half = (self.page_rows.unwrap_or(PAGE_ROWS_FALLBACK).max(2) / 2) as isize;
            let delta = match (code, ctrl) {
                (KeyCode::PageDown, _) | (KeyCode::Char('d'), true) => Some(half),
                (KeyCode::PageUp, _) | (KeyCode::Char('u'), true) => Some(-half),
                (KeyCode::Home, _) | (KeyCode::Char('g'), false) => Some(-FAR),
                (KeyCode::End, _) | (KeyCode::Char('G'), false) => Some(FAR),
                _ => None,
            };
            if let Some(delta) = delta {
                match self.view {
                    View::Board => self.page_selected(delta),
                    View::Zoom { .. } => self.move_zoom_scroll(delta),
                    View::PlaysFeed => self.move_feed_scroll(delta),
                    View::Standings(_) => self.move_standings_scroll(delta),
                    _ => {}
                }
                return;
            }
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
