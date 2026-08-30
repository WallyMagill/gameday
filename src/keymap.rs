//! Single source of truth for every key binding. This one table generates
//! BOTH the footer chord list and the help overlay, so they cannot drift:
//! adding a binding here is the only way it becomes visible anywhere.

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

/// When a binding appears in the footer. The footer shows only the top
/// chords; the help overlay always shows the whole table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterSlot {
    Never,
    Always,
    /// Only while no game is focused ([ENTER] FOCUS).
    Unfocused,
    /// Only while a game is focused ([ESC] BACK).
    Focused,
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
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["J/K", "↓/↑"],
        label: "MOVE",
        group: Group::Selection,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["SPC"],
        label: "PIN",
        group: Group::Selection,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["T"],
        label: "FAVORITE",
        group: Group::Selection,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &["ENTER"],
        label: "FOCUS",
        group: Group::Selection,
        footer: FooterSlot::Unfocused,
    },
    Binding {
        keys: &["ESC"],
        label: "BACK",
        group: Group::Selection,
        footer: FooterSlot::Focused,
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
        keys: &["R"],
        label: "REFRESH",
        group: Group::App,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["?"],
        label: "HELP",
        group: Group::App,
        footer: FooterSlot::Always,
    },
    Binding {
        keys: &["Q", "CTRL-C"],
        label: "QUIT",
        group: Group::App,
        footer: FooterSlot::Always,
    },
];

/// Labels shed from the footer first when the terminal is too narrow for the
/// whole chord list, least valuable first. HELP and QUIT are deliberately
/// absent: whatever gets clipped, the way out and the way to the full keymap
/// stay visible.
pub const FOOTER_DROP_ORDER: &[&str] = &["MOVE", "PAGE", "PIN", "LEAGUE", "REFRESH"];

/// The footer chord list for the current focus state: (key, label) pairs in
/// table order.
pub fn footer_chords(focused: bool) -> Vec<(&'static str, &'static str)> {
    KEYMAP
        .iter()
        .filter(|b| match b.footer {
            FooterSlot::Always => true,
            FooterSlot::Never => false,
            FooterSlot::Unfocused => !focused,
            FooterSlot::Focused => focused,
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
        for focused in [false, true] {
            for (key, label) in footer_chords(focused) {
                assert!(
                    KEYMAP.iter().any(|b| b.keys[0] == key && b.label == label),
                    "footer chord [{key}] {label} not in KEYMAP"
                );
            }
        }
    }

    #[test]
    fn footer_swaps_focus_for_back() {
        let unfocused = footer_chords(false);
        let focused = footer_chords(true);
        assert!(unfocused.iter().any(|(_, l)| *l == "FOCUS"));
        assert!(!unfocused.iter().any(|(_, l)| *l == "BACK"));
        assert!(focused.iter().any(|(k, l)| *k == "ESC" && *l == "BACK"));
        assert!(!focused.iter().any(|(_, l)| *l == "FOCUS"));
        // The '?' hint is always advertised.
        for chords in [&unfocused, &focused] {
            assert!(chords.iter().any(|(k, _)| *k == "?"));
        }
    }
}
