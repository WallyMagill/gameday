//! View dispatch: `App.view` names the full-screen surface the body renders;
//! each surface draws from its own module. Header/ticker/footer stay in
//! `app/chrome.rs` — they are shared chrome, identical across views.

pub mod board;
pub mod config_view;
pub mod plays_feed;
pub mod standings;
pub mod theme_picker;
pub mod zoom;

use crate::app::App;
use crate::domain::League;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Which surface fills the body.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Board,
    Zoom {
        game_id: String,
        tab: ZoomTab,
    },
    PlaysFeed,
    Standings(League),
    ConfigView,
    /// `:theme` with no argument: the board stays underneath as the live
    /// preview; the panel lists every loaded theme.
    ThemePicker,
}

/// Tabs inside the zoomed single-game view, cycled with h/l and [/].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoomTab {
    #[default]
    Overview,
    Plays,
    Stats,
}

impl ZoomTab {
    pub const ALL: [ZoomTab; 3] = [ZoomTab::Overview, ZoomTab::Plays, ZoomTab::Stats];

    pub fn label(self) -> &'static str {
        match self {
            ZoomTab::Overview => "OVERVIEW",
            ZoomTab::Plays => "PLAYS",
            ZoomTab::Stats => "STATS",
        }
    }

    /// Neighbor `delta` steps away, wrapping — powers h/l and [/].
    pub fn cycled(self, delta: isize) -> ZoomTab {
        let n = Self::ALL.len() as isize;
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        Self::ALL[(i + delta).rem_euclid(n) as usize]
    }
}

/// Render the current view's body into `area`. `app` is mutable so views can
/// register their mouse hit zones while they draw (state itself is read-only
/// here — drawing must never change what is drawn).
pub fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    // Cloned so the borrow of `app.view` doesn't pin `app` across the call.
    match app.view.clone() {
        View::Board => board::draw(app, frame, area),
        View::Zoom { game_id, tab } => zoom::draw(app, frame, area, &game_id, tab),
        View::PlaysFeed => plays_feed::draw(app, frame, area),
        View::Standings(league) => standings::draw(app, frame, area, league),
        View::ConfigView => config_view::draw(app, frame, area),
        View::ThemePicker => {
            board::draw(app, frame, area);
            theme_picker::draw(app, frame, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_tabs_cycle_and_wrap_both_ways() {
        assert_eq!(ZoomTab::Overview.cycled(1), ZoomTab::Plays);
        assert_eq!(ZoomTab::Plays.cycled(1), ZoomTab::Stats);
        assert_eq!(ZoomTab::Stats.cycled(1), ZoomTab::Overview, "wraps forward");
        assert_eq!(ZoomTab::Overview.cycled(-1), ZoomTab::Stats, "wraps backward");
    }
}
