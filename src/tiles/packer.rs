use super::Density;
use crate::domain::Game;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum LayoutPref {
    #[default]
    Auto,
    One,
    Two,
    Four,
    Sidebar,
}

pub struct PackedTile<'a> {
    pub game: &'a Game,
    pub area: Rect,
    pub density: Density,
}

/// Tiles per page for `pref` over `n` games. Public so `App` can clamp/wrap
/// its page index with the exact numbers `pack` will use — the narrow
/// (<60 col / Sidebar) branch slices by this same size, so the two can
/// never disagree about which games a page holds.
pub fn page_size(pref: LayoutPref, n: usize) -> usize {
    match pref {
        LayoutPref::One => 1,
        LayoutPref::Two => 2,
        LayoutPref::Four => 4,
        LayoutPref::Sidebar => 8,
        LayoutPref::Auto => match n {
            0 | 1 => 1,
            2 => 2,
            _ => 4,
        },
    }
}

fn resolved(pref: LayoutPref, n: usize) -> LayoutPref {
    match pref {
        LayoutPref::Auto => match n {
            0 | 1 => LayoutPref::One,
            2 => LayoutPref::Two,
            _ => LayoutPref::Four,
        },
        other => other,
    }
}

pub fn pack<'a>(games: &'a [Game], area: Rect, pref: LayoutPref, page: usize) -> Vec<PackedTile<'a>> {
    if games.is_empty() || area.width == 0 || area.height == 0 {
        return vec![];
    }
    let narrow = pref == LayoutPref::Sidebar || area.width < 60;
    if narrow {
        // Same page size as page_size(): App's n/p paging and j/k selection
        // math are computed from it, so a height-based size here would strand
        // games on unreachable pages.
        let ps = page_size(pref, games.len());
        let start = page.saturating_mul(ps);
        if start >= games.len() {
            return vec![];
        }
        let slice = &games[start..games.len().min(start + ps)];
        let h = (area.height / slice.len() as u16).max(1);
        return slice
            .iter()
            .enumerate()
            .map(|(i, game)| PackedTile {
                game,
                density: Density::Compact,
                area: Rect {
                    x: area.x,
                    y: area.y + i as u16 * h,
                    width: area.width,
                    height: h.min(area.height.saturating_sub(i as u16 * h)),
                },
            })
            .collect();
    }
    let pref = resolved(pref, games.len());
    let ps = page_size(pref, games.len());
    let start = page.saturating_mul(ps);
    if start >= games.len() {
        return vec![];
    }
    let slice = &games[start..games.len().min(start + ps)];
    let density = match pref {
        LayoutPref::One => Density::Full,
        _ => Density::Standard,
    };
    let rects = split_areas(area, pref, slice.len());
    slice
        .iter()
        .zip(rects)
        .map(|(game, r)| PackedTile {
            game,
            area: r,
            density,
        })
        .collect()
}

fn split_areas(area: Rect, pref: LayoutPref, n: usize) -> Vec<Rect> {
    match (pref, n) {
        (LayoutPref::One, _) | (_, 1) if pref != LayoutPref::Four && pref != LayoutPref::Two => {
            vec![area]
        }
        (LayoutPref::Two, _) | (_, 2) if pref != LayoutPref::Four => {
            let split = if area.height < 18 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area)
                    .to_vec()
            } else {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area)
                    .to_vec()
            };
            split.into_iter().take(n).collect()
        }
        _ => {
            // LayoutPref::Four (including n=1): always 2×2; take top-left cell(s).
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let mut out = Vec::new();
            for row in rows.iter() {
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(*row);
                out.extend_from_slice(&cols);
            }
            out.truncate(n);
            out
        }
    }
}
