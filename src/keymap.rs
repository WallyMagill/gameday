//! Single source of truth for every key binding. This one table generates
//! BOTH the footer chord list and the help overlay, so they cannot drift:
//! adding a binding here is the only way it becomes visible anywhere.
//!
//! Mouse support lives here too: views register `(Rect, Hit)` zones while
//! they draw, and [`on_mouse`] resolves a click against them (wheel events
//! scroll the current view without needing a zone).

use crate::app::{App, Tab};
use crate::views::ZoomTab;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

/// What a mouse gesture resolved to. Clicks come from the hit zones the last
/// draw registered; wheel events map straight to Scroll and let the current
/// view decide what scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Select mosaic tile at this selection-list index.
    Tile(usize),
    /// Switch to this header tab.
    TabChip(Tab),
    /// Select slate row `i` (selection index = live tiles + i).
    SlateRow(usize),
    /// Switch the zoomed view to this tab.
    ZoomTab(ZoomTab),
    ScrollUp,
    ScrollDown,
}

/// Route one mouse event: left click hit-tests the zones the last draw
/// registered; the wheel scrolls whatever the current view scrolls with j/k.
/// Inert while the help overlay is open — help is modal for clicks too.
pub fn on_mouse(app: &mut App, ev: MouseEvent) {
    if app.help_open {
        return;
    }
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let pos = Position {
                x: ev.column,
                y: ev.row,
            };
            if let Some(hit) = app.hit_at(pos) {
                app.on_hit(hit);
            }
        }
        MouseEventKind::ScrollUp => app.on_hit(Hit::ScrollUp),
        MouseEventKind::ScrollDown => app.on_hit(Hit::ScrollDown),
        _ => {}
    }
}

/// Help-overlay section a binding belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Navigation,
    Selection,
    View,
    App,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Navigation, Group::Selection, Group::View, Group::App];

    pub fn title(self) -> &'static str {
        match self {
            Group::Navigation => "NAVIGATION",
            Group::Selection => "SELECTION",
            Group::View => "VIEW",
            Group::App => "APP",
        }
    }
}

/// Which surface the footer is describing — Board, the Config editor, or any
/// other full-screen view (zoom, plays feed, standings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterCtx {
    Board,
    Zoomed,
    Config,
}

/// When a binding appears in the footer. The footer shows only the top
/// chords; the help overlay always shows the whole table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterSlot {
    Never,
    Always,
    /// Only on the Board view ([Z] ZOOM, [Q] QUIT).
    Board,
    /// Only inside the zoomed view (tab cycling).
    Zoomed,
    /// Only in the Config editor (toggle/edit/cycle).
    Config,
    /// Every full-screen view that pops back to the board ([ESC] BACK).
    NotBoard,
}

pub struct Binding {
    /// Every chord for the action; `keys[0]` is the one the footer shows.
    pub keys: &'static [&'static str],
    pub label: &'static str,
    pub group: Group,
    pub footer: FooterSlot,
}

pub const KEYMAP: &[Binding] = &[
    Binding {
        keys: &["TAB", "S-TAB", "L/H", "→/←"],
        label: "LEAGUE",
        group: Group::Navigation,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["N/P", "PGDN/PGUP"],
        label: "PAGE",
        group: Group::Navigation,
        footer: FooterSlot::Board,
    },
    Binding {
        keys: &["J/K", "↓/↑"],
        label: "MOVE",
        group: Group::Selection,
        footer: FooterSlot::Board,
    },
    Binding {
        keys: &["SPC"],
        label: "PIN",
        group: Group::Selection,
        footer: FooterSlot::Board,
    },
    Binding {
        keys: &["T"],
        label: "FAVORITE",
        group: Group::Selection,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &["Z", "ENTER"],
        label: "ZOOM",
        group: Group::Selection,
        footer: FooterSlot::Board,
    },
    Binding {
        keys: &["ESC", "Q"],
        label: "BACK",
        group: Group::Selection,
        footer: FooterSlot::NotBoard,
    },
    Binding {
        // Zoom tabs: OVERVIEW | PLAYS | STATS.
        keys: &["H/L", "[/]"],
        label: "TABS",
        group: Group::View,
        footer: FooterSlot::Zoomed,
    },
    Binding {
        // Config rows: enable/disable a league tab, remove a favorite.
        keys: &["SPC"],
        label: "TOGGLE",
        group: Group::View,
        footer: FooterSlot::Config,
    },
    Binding {
        // Config: ENTER activates the row (add/remove favorite, toggle).
        keys: &["ENTER"],
        label: "EDIT",
        group: Group::View,
        footer: FooterSlot::Config,
    },
    Binding {
        // Config display rows: THEME / SCORE / LAYOUT values.
        keys: &["H/L"],
        label: "CYCLE",
        group: Group::View,
        footer: FooterSlot::Config,
    },
    Binding {
        keys: &["1/2/4/S"],
        label: "LAYOUT",
        group: Group::View,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &["C"],
        label: "THEME",
        group: Group::View,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &[":"],
        label: "CMD",
        group: Group::App,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["/"],
        label: "FILTER",
        group: Group::View,
        footer: FooterSlot::Always,
    },
    Binding {
        // Footer real estate went to [:] and [/]; refresh stays in help.
        keys: &["R"],
        label: "REFRESH",
        group: Group::App,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &["?"],
        label: "HELP",
        group: Group::App,
        footer: FooterSlot::Always,
    },
    Binding {
        // 'q' quits only from the Board (it pops other views); the footer
        // advertises it only where it's true. Ctrl+C quits from anywhere.
        keys: &["Q", "CTRL-C"],
        label: "QUIT",
        group: Group::App,
        footer: FooterSlot::Board,
    },
];

/// Labels shed from the footer first when the terminal is too narrow for the
/// whole chord list, least valuable first. HELP and QUIT are deliberately
/// absent: whatever gets clipped, the way out and the way to the full keymap
/// stay visible.
pub const FOOTER_DROP_ORDER: &[&str] = &["MOVE", "PAGE", "PIN", "LEAGUE", "FILTER", "CMD"];

/// The footer chord list for the current view: (key, label) pairs in table
/// order.
pub fn footer_chords(ctx: FooterCtx) -> Vec<(&'static str, &'static str)> {
    KEYMAP
        .iter()
        .filter(|b| match b.footer {
            FooterSlot::Always => true,
            FooterSlot::Never => false,
            FooterSlot::Board => ctx == FooterCtx::Board,
            FooterSlot::Zoomed => ctx == FooterCtx::Zoomed,
            FooterSlot::Config => ctx == FooterCtx::Config,
            FooterSlot::NotBoard => ctx != FooterCtx::Board,
        })
        .map(|b| (b.keys[0], b.label))
        .collect()
}

/// Help-overlay rows for one group: (all chords joined, label).
pub fn help_rows(group: Group) -> Vec<(String, &'static str)> {
    KEYMAP
        .iter()
        .filter(|b| b.group == group)
        .map(|b| (b.keys.join("  "), b.label))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_reaches_the_help_overlay() {
        let shown: usize = Group::ALL.iter().map(|g| help_rows(*g).len()).sum();
        assert_eq!(shown, KEYMAP.len(), "a binding is missing from help");
        // Every individual chord string appears in some help row.
        for b in KEYMAP {
            let rows = help_rows(b.group);
            for key in b.keys {
                assert!(
                    rows.iter().any(|(keys, _)| keys.contains(key)),
                    "chord {key} for {} missing from help",
                    b.label
                );
            }
        }
    }

    #[test]
    fn footer_is_a_subset_of_the_keymap() {
        for ctx in [FooterCtx::Board, FooterCtx::Zoomed, FooterCtx::Config] {
            for (key, label) in footer_chords(ctx) {
                assert!(
                    KEYMAP.iter().any(|b| b.keys[0] == key && b.label == label),
                    "footer chord [{key}] {label} not in KEYMAP"
                );
            }
        }
    }

    #[test]
    fn config_footer_shows_its_own_chords_and_the_way_back() {
        let config = footer_chords(FooterCtx::Config);
        for label in ["TOGGLE", "EDIT", "CYCLE", "BACK", "HELP"] {
            assert!(
                config.iter().any(|(_, l)| *l == label),
                "config footer missing {label}: {config:?}"
            );
        }
        // Zoom's tab cycling and the board's quit don't apply there.
        assert!(!config.iter().any(|(_, l)| *l == "TABS"));
        assert!(!config.iter().any(|(_, l)| *l == "QUIT"));
        // And the config chords leak into no other view's footer.
        for ctx in [FooterCtx::Board, FooterCtx::Zoomed] {
            assert!(!footer_chords(ctx).iter().any(|(_, l)| *l == "TOGGLE"));
        }
    }

    #[test]
    fn footer_swaps_zoom_for_back_per_view() {
        let board = footer_chords(FooterCtx::Board);
        let zoomed = footer_chords(FooterCtx::Zoomed);
        assert!(board.iter().any(|(k, l)| *k == "Z" && *l == "ZOOM"));
        assert!(!board.iter().any(|(_, l)| *l == "BACK"));
        // 'q' quits only from the Board, so QUIT is advertised only there…
        assert!(board.iter().any(|(_, l)| *l == "QUIT"));
        assert!(!zoomed.iter().any(|(_, l)| *l == "QUIT"));
        // …and the zoomed footer shows the way back plus the tab cycle.
        assert!(zoomed.iter().any(|(k, l)| *k == "ESC" && *l == "BACK"));
        assert!(zoomed.iter().any(|(_, l)| *l == "TABS"));
        assert!(!zoomed.iter().any(|(_, l)| *l == "ZOOM"));
        // The '?' hint is always advertised.
        for chords in [&board, &zoomed] {
            assert!(chords.iter().any(|(k, _)| *k == "?"));
        }
    }
}
