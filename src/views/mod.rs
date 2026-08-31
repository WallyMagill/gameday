//! View dispatch: `App.view` names the full-screen surface the body renders;
//! each surface draws from its own module. Header/ticker/footer stay in
//! `app.rs` — they are shared chrome, identical across views.

pub mod board;
pub mod plays_feed;
pub mod standings;
pub mod zoom;

use crate::app::App;
use crate::domain::League;
use crate::theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Which surface fills the body. ConfigView renders a placeholder until its
/// task in this plan lands — it exists now so `command::Cmd` routing compiles
/// and `:config` navigates somewhere honest instead of erroring.
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

/// Render the current view's body into `area`.
pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    match &app.view {
        View::Board => board::draw(app, frame, area),
        View::Zoom { game_id, tab } => zoom::draw(app, frame, area, game_id, *tab),
        View::PlaysFeed => plays_feed::draw(app, frame, area),
        View::Standings(league) => standings::draw(app, frame, area, *league),
        View::ConfigView => placeholder(frame, area, "CONFIG"),
    }
}

/// Honest stub for views whose tasks land later in this plan.
fn placeholder(frame: &mut Frame, area: Rect, label: &str) {
    let th = theme::current();
    frame.render_widget(
        Paragraph::new(format!("{label} — coming in this plan · esc back"))
            .style(Style::default().fg(th.muted).bg(th.bg))
            .alignment(Alignment::Center),
        area,
    );
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
