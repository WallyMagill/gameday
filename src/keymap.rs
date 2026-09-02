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

/// Which surface the footer is describing — Board, the zoomed game (with its
/// OVERVIEW/PLAYS/STATS tabs), the Config editor, or a scrolling feed (plays
/// feed, standings) that has no tabs of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterCtx {
    Board,
    Zoomed,
    Config,
    Feed,
}

impl FooterCtx {
    pub const ALL: [FooterCtx; 4] = [
        FooterCtx::Board,
        FooterCtx::Zoomed,
        FooterCtx::Config,
        FooterCtx::Feed,
    ];
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
        // Slate time travel: step the viewed date back/forward a day. Board
        // only, and never in the footer — the header's ‹ date › says when
        // it is in use.
        keys: &["[/]"],
        label: "DATE",
        group: Group::Navigation,
        footer: FooterSlot::Never,
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

    /// Every key the Board handler reacts to must be advertised somewhere.
    /// Drives a fresh App with one live game and diffs observable state.
    #[test]
    fn every_handled_board_key_is_a_binding() {
        use crate::app::App;
        use crate::config::Config;
        use crate::domain::*;
        use crossterm::event::{KeyCode, KeyModifiers};
        let mk = || {
            let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir().join(format!("gd-km-{}", std::process::id())), time::UtcOffset::UTC);
            let g = |id: &str| Game { id: id.into(), status: Status::Live, away: Team { abbr: "KC".into(), ..Default::default() }, home: Team { abbr: "TB".into(), ..Default::default() }, ..Default::default() };
            app.apply_boards(League::Nfl, vec![g("1"), g("2"), g("3"), g("4"), g("5")], false);
            app.tab = crate::app::Tab::League(League::Nfl);
            app
        };
        let snapshot = |a: &App| format!("{:?}|{}|{}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{:?}|{}|{:?}",
            a.tab, a.page, a.selected, a.pins.len(), a.view, a.help_open, a.should_quit, a.refresh_now,
            a.filter, a.config.layout, a.config.theme, a.config.favorites.len(), a.viewed_date_offset);
        let chord_for = |c: char| -> String { match c {
            ' ' => "SPC".into(), '[' | ']' => "[/]".into(), '?' => "?".into(), ':' => ":".into(), '/' => "/".into(),
            '1' | '2' | '4' => "1/2/4/S".into(), 's' => "1/2/4/S".into(),
            other => other.to_ascii_uppercase().to_string(),
        }};
        let mut unadvertised = vec![];
        for c in (b' '..=b'~').map(char::from) {
            let mut app = mk();
            let before = snapshot(&app);
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
            if snapshot(&app) == before { continue; }
            let chord = chord_for(c);
            let advertised = KEYMAP.iter().any(|b| b.keys.iter().any(|k| k.split('/').any(|part| part == chord) || *k == chord));
            if !advertised { unadvertised.push(c); }
        }
        assert!(unadvertised.is_empty(), "keys that change state but appear in no Binding: {unadvertised:?}");
    }

    #[test]
    fn feed_footer_drops_the_zoom_tab_cycle_but_keeps_league_and_back() {
        let feed = footer_chords(FooterCtx::Feed);
        // h/l cycle nothing in the plays feed / standings — never advertised.
        assert!(!feed.iter().any(|(_, l)| *l == "TABS"), "{feed:?}");
        for label in ["LEAGUE", "BACK", "HELP", "CMD"] {
            assert!(feed.iter().any(|(_, l)| *l == label), "feed footer missing {label}: {feed:?}");
        }
        assert!(!feed.iter().any(|(_, l)| *l == "QUIT" || *l == "ZOOM" || *l == "TOGGLE"));
    }

    #[test]
    fn footer_is_a_subset_of_the_keymap() {
        for ctx in FooterCtx::ALL {
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
        for ctx in [FooterCtx::Board, FooterCtx::Zoomed, FooterCtx::Feed] {
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
