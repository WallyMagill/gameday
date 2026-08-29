//! Semantic theme: every draw call reads roles (bg/fg/live/...) off the
//! current `Theme`, never raw colors. Three palettes ship as constructors;
//! `current()` is thread-local so draw code (single-threaded) sees the theme
//! the app set, and each test thread can set its own deterministically.

use crate::domain::League;
use ratatui::style::Color;
use std::cell::Cell;

/// Semantic palette. Field names are roles, not hues — `green`/`cyan`/`magenta`
/// keep their broadcast names even where a palette (phosphor) remaps them to
/// warm steps, because draw code means "positive"/"clock"/"records header".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub bright: Color,
    pub muted: Color,
    pub dim: Color,
    pub border: Color,
    pub live: Color,
    pub green: Color,
    pub cyan: Color,
    pub magenta: Color,
    pub star: Color,
    accent_nfl: Color,
    accent_cfb: Color,
    accent_nba: Color,
    accent_wnba: Color,
    accent_cbb: Color,
    accent_mlb: Color,
    accent_nhl: Color,
    accent_epl: Color,
    accent_mls: Color,
}

impl Theme {
    /// Default identity: RedZone board. True-black ground, warm live red,
    /// cool structure.
    pub const fn broadcast() -> Self {
        Self {
            bg: Color::Rgb(0, 0, 0),
            fg: Color::Rgb(200, 200, 200),
            bright: Color::Rgb(235, 235, 235),
            muted: Color::Rgb(120, 120, 120),
            dim: Color::Rgb(60, 60, 60),
            border: Color::Rgb(80, 80, 80),
            live: Color::Rgb(255, 60, 60),
            green: Color::Rgb(80, 210, 110),
            cyan: Color::Rgb(70, 200, 220),
            magenta: Color::Rgb(220, 100, 220),
            star: Color::Rgb(240, 200, 70),
            accent_nfl: Color::Rgb(255, 70, 70),
            accent_cfb: Color::Rgb(255, 150, 60),
            accent_nba: Color::Rgb(80, 140, 255),
            accent_wnba: Color::Rgb(110, 180, 255), // near NBA blue, lighter
            accent_cbb: Color::Rgb(120, 120, 255),
            accent_mlb: Color::Rgb(230, 200, 60),
            accent_nhl: Color::Rgb(70, 200, 220),
            accent_epl: Color::Rgb(90, 210, 130), // pitch green
            accent_mls: Color::Rgb(60, 200, 180), // teal, apart from EPL
        }
    }

    /// Teletext: dark blue ground, the seven-ish teletext hues. Accent reuse
    /// across leagues (WNBA red, EPL green, MLS cyan) is authentic teletext —
    /// the hue set is small.
    pub const fn ceefax() -> Self {
        Self {
            bg: Color::Rgb(10, 10, 46),
            fg: Color::Rgb(216, 216, 240),
            bright: Color::Rgb(240, 240, 255),
            muted: Color::Rgb(106, 106, 154),
            dim: Color::Rgb(51, 51, 92),
            border: Color::Rgb(74, 74, 122),
            live: Color::Rgb(255, 68, 68),
            green: Color::Rgb(60, 220, 90),
            cyan: Color::Rgb(0, 224, 224),
            magenta: Color::Rgb(220, 100, 220),
            star: Color::Rgb(255, 210, 0),
            accent_nfl: Color::Rgb(0, 224, 224),
            accent_cfb: Color::Rgb(255, 210, 0),
            accent_nba: Color::Rgb(255, 140, 0),
            accent_wnba: Color::Rgb(255, 68, 68),
            accent_cbb: Color::Rgb(220, 100, 220),
            accent_mlb: Color::Rgb(60, 220, 90),
            accent_nhl: Color::Rgb(90, 140, 255),
            accent_epl: Color::Rgb(60, 220, 90),
            accent_mls: Color::Rgb(0, 224, 224),
        }
    }

    /// Amber CRT lamp, deliberately near-monochrome: green/cyan/magenta roles
    /// remap to warm amber steps. NHL keeps the single cold accent for ice.
    pub const fn phosphor() -> Self {
        Self {
            bg: Color::Rgb(8, 6, 0),
            fg: Color::Rgb(255, 176, 0),
            bright: Color::Rgb(255, 200, 80),
            muted: Color::Rgb(150, 100, 20),
            dim: Color::Rgb(70, 45, 10),
            border: Color::Rgb(110, 75, 20),
            live: Color::Rgb(255, 240, 190),
            green: Color::Rgb(255, 220, 140),
            cyan: Color::Rgb(200, 140, 40),
            magenta: Color::Rgb(230, 170, 60),
            star: Color::Rgb(255, 235, 180),
            accent_nfl: Color::Rgb(255, 180, 40),
            accent_cfb: Color::Rgb(255, 140, 20),
            accent_nba: Color::Rgb(255, 210, 120),
            accent_wnba: Color::Rgb(255, 195, 80), // amber step between NBA and NFL
            accent_cbb: Color::Rgb(200, 130, 30),
            accent_mlb: Color::Rgb(220, 150, 10),
            accent_nhl: Color::Rgb(200, 220, 220), // the single cold accent for ice
            accent_epl: Color::Rgb(235, 165, 45), // amber steps
            accent_mls: Color::Rgb(180, 120, 25),
        }
    }

    /// Accent color for a league's chip, LAST PLAYS label, and meter.
    pub fn league_accent(&self, league: League) -> Color {
        match league {
            League::Nfl => self.accent_nfl,
            League::Cfb => self.accent_cfb,
            League::Nba => self.accent_nba,
            League::Wnba => self.accent_wnba,
            League::Cbb => self.accent_cbb,
            League::Mlb => self.accent_mlb,
            League::Nhl => self.accent_nhl,
            League::Epl => self.accent_epl,
            League::Mls => self.accent_mls,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeName {
    Broadcast,
    Ceefax,
    Phosphor,
}

impl ThemeName {
    pub const ALL: [ThemeName; 3] = [ThemeName::Broadcast, ThemeName::Ceefax, ThemeName::Phosphor];

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeName::Broadcast => "broadcast",
            ThemeName::Ceefax => "ceefax",
            ThemeName::Phosphor => "phosphor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "broadcast" => Some(ThemeName::Broadcast),
            "ceefax" => Some(ThemeName::Ceefax),
            "phosphor" => Some(ThemeName::Phosphor),
            _ => None,
        }
    }

    /// 'c' key order: broadcast -> ceefax -> phosphor -> broadcast.
    pub fn next(self) -> Self {
        match self {
            ThemeName::Broadcast => ThemeName::Ceefax,
            ThemeName::Ceefax => ThemeName::Phosphor,
            ThemeName::Phosphor => ThemeName::Broadcast,
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeName::Broadcast => Theme::broadcast(),
            ThemeName::Ceefax => Theme::ceefax(),
            ThemeName::Phosphor => Theme::phosphor(),
        }
    }
}

/// Lenient parse for config/env values: unknown names fall back to broadcast
/// with a stderr note naming the bad value and the valid set.
pub fn parse_or_default(s: &str) -> ThemeName {
    ThemeName::parse(s).unwrap_or_else(|| {
        eprintln!(
            "gameday: unknown theme {s:?}, valid: {}; using broadcast",
            ThemeName::ALL.map(|t| t.as_str()).join("|")
        );
        ThemeName::Broadcast
    })
}

thread_local! {
    // Thread-local, not process-global: draw code is single-threaded, and
    // parallel test threads each get their own current theme.
    static CURRENT: Cell<ThemeName> = const { Cell::new(ThemeName::Broadcast) };
}

pub fn set_current(name: ThemeName) {
    CURRENT.set(name);
}

pub fn current_name() -> ThemeName {
    CURRENT.get()
}

pub fn current() -> Theme {
    CURRENT.get().theme()
}

pub fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

/// Luminance step for pulse effects: same hue at ~55% brightness — visible
/// but subtle (55% picked by eye against the broadcast live red).
pub fn dimmed(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as u16 * 11 / 20) as u8,
            (g as u16 * 11 / 20) as u8,
            (b as u16 * 11 / 20) as u8,
        ),
        other => other,
    }
}

/// Word the ticker/alerts use for a scoring play in this league.
pub fn scoring_word(league: League) -> &'static str {
    match league {
        League::Nfl | League::Cfb => "TOUCHDOWN!",
        League::Nba | League::Wnba | League::Cbb => "BUCKET!",
        League::Mlb => "HOME RUN!",
        League::Nhl | League::Epl | League::Mls => "GOAL!",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_carry_their_identity() {
        assert_eq!(Theme::broadcast().bg, Color::Rgb(0, 0, 0));
        assert_eq!(Theme::broadcast().live, Color::Rgb(255, 60, 60));
        assert_eq!(Theme::ceefax().bg, Color::Rgb(10, 10, 46));
        assert_eq!(Theme::ceefax().star, Color::Rgb(255, 210, 0));
        assert_eq!(Theme::phosphor().fg, Color::Rgb(255, 176, 0));
        // NHL keeps the single cold accent in the amber palette.
        assert_eq!(
            Theme::phosphor().league_accent(League::Nhl),
            Color::Rgb(200, 220, 220)
        );
    }

    #[test]
    fn every_league_has_an_accent_in_every_palette() {
        for name in ThemeName::ALL {
            let theme = name.theme();
            for league in League::ALL {
                // Rgb only — no ANSI-16 leaks into any palette.
                assert!(
                    matches!(theme.league_accent(league), Color::Rgb(..)),
                    "{name:?}/{league:?} accent is not truecolor"
                );
            }
        }
    }

    #[test]
    fn parse_roundtrips_and_is_lenient() {
        for name in ThemeName::ALL {
            assert_eq!(ThemeName::parse(name.as_str()), Some(name));
        }
        assert_eq!(ThemeName::parse("CEEFAX"), Some(ThemeName::Ceefax));
        assert_eq!(ThemeName::parse("solarized"), None);
        assert_eq!(parse_or_default("solarized"), ThemeName::Broadcast);
    }

    #[test]
    fn next_cycles_all_three() {
        assert_eq!(ThemeName::Broadcast.next(), ThemeName::Ceefax);
        assert_eq!(ThemeName::Ceefax.next(), ThemeName::Phosphor);
        assert_eq!(ThemeName::Phosphor.next(), ThemeName::Broadcast);
    }

    #[test]
    fn current_is_settable_per_thread() {
        assert_eq!(current_name(), ThemeName::Broadcast);
        set_current(ThemeName::Phosphor);
        assert_eq!(current(), Theme::phosphor());
        // Another thread still sees the default.
        std::thread::spawn(|| assert_eq!(current_name(), ThemeName::Broadcast))
            .join()
            .unwrap();
        set_current(ThemeName::Broadcast);
    }
}
