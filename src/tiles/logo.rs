//! Tile-era logo API. The art and the blit moved to [`crate::board::logo`];
//! these shims keep the tile grammar compiling until the board replaces it.

use crate::domain::Team;
use ratatui::layout::Rect;
use ratatui::Frame;

pub use crate::board::logo::{draw_abbr_mark, draw_hero_mark, hero_mark, ArtCell, HeroMark};

/// Old name for [`hero_mark`].
pub fn load_logo(key: &str) -> Option<&'static HeroMark> {
    hero_mark(key)
}

/// Old tile blit: the team's mark, or its abbreviation when art is missing.
pub fn draw_logo(frame: &mut Frame, area: Rect, team: &Team) {
    match hero_mark(&team.logo_key) {
        Some(mark) => draw_hero_mark(frame, area, mark),
        None => draw_abbr_mark(frame, area, team),
    }
}
