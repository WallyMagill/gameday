//! Themes as identities: a theme is a palette plus a *discipline* (what the
//! chrome is allowed to color). Every draw call reads roles (bg/fg/live/...)
//! off the current `Theme`, never raw colors, and asks the discipline helpers
//! (`chip`, `section_label`, `team_text`, `clock`, `sidebar_header`) before
//! spending an accent.
//!
//! One TOML format serves the built-ins (compiled in from `assets/themes/`)
//! and user files in `<config_dir>/themes/*.toml`; a user file wins on a name
//! clash, a broken one is skipped with a stderr line naming the file, the key
//! and the expected form. `current()` is thread-local so draw code
//! (single-threaded) sees the theme the app set, and each test thread can set
//! its own deterministically.

use crate::domain::League;
use ratatui::style::Color;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Built-in theme names in picker/cycle order. `broadcast` is the default
/// identity; the community palettes are opt-in.
pub const BUILTIN_NAMES: [&str; 11] = [
    "broadcast",
    "studio",
    "ceefax",
    "phosphor",
    "gruvbox",
    "tokyo-night",
    "nord",
    "catppuccin-mocha",
    "rose-pine",
    "everforest",
    "dracula",
];

/// The compiled-in theme sources, parallel to [`BUILTIN_NAMES`].
const BUILTIN_TOML: [&str; 11] = [
    include_str!("../assets/themes/broadcast.toml"),
    include_str!("../assets/themes/studio.toml"),
    include_str!("../assets/themes/ceefax.toml"),
    include_str!("../assets/themes/phosphor.toml"),
    include_str!("../assets/themes/gruvbox.toml"),
    include_str!("../assets/themes/tokyo-night.toml"),
    include_str!("../assets/themes/nord.toml"),
    include_str!("../assets/themes/catppuccin-mocha.toml"),
    include_str!("../assets/themes/rose-pine.toml"),
    include_str!("../assets/themes/everforest.toml"),
    include_str!("../assets/themes/dracula.toml"),
];

/// How the three sidebar headers (⚑ GLOBAL ALERTS / TOP PLAYS / RECORDS) are
/// colored: each its own hue, one shared accent (`star`), or gray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarHeaders {
    Multi,
    #[default]
    Single,
    Muted,
}

impl SidebarHeaders {
    pub const VALID: &'static str = "multi|single|muted";
}

/// What the chrome is allowed to color. The identity floor — scores, logos,
/// LIVE, scoring words — is always colored and has no knob here.
///
/// `sidebar_headers` owns the sidebar's three headers outright (it is the
/// more specific knob); `section_labels` owns every other section caption
/// (LAST PLAYS, meter labels, LEADERS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Discipline {
    /// League accent on the `[NFL]` chips (false: chips in `fg`).
    pub chips: bool,
    /// Accent on section captions — LAST PLAYS, LEAD METER, LEADERS (false: `muted`).
    pub section_labels: bool,
    /// Team/league color on play-row text — play abbrs, ticker abbrs, alert
    /// abbrs, TOP PLAYS lines, the RECORDS rail names (false: `fg`).
    pub play_abbrs: bool,
    /// `cyan` clocks (false: `muted`).
    pub clocks: bool,
    pub sidebar_headers: SidebarHeaders,
}

impl Default for Discipline {
    /// The spec example's values: chips on, everything else calm.
    fn default() -> Self {
        Self {
            chips: true,
            section_labels: false,
            play_abbrs: false,
            clocks: false,
            sidebar_headers: SidebarHeaders::Single,
        }
    }
}

/// Which sidebar header a draw call is coloring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarHeader {
    Alerts,
    TopPlays,
    Records,
}

/// Semantic palette + discipline. Field names are roles, not hues —
/// `green`/`cyan`/`magenta` keep their broadcast names even where a palette
/// (phosphor) remaps them to warm steps, because draw code means
/// "positive"/"clock"/"records header".
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
    /// One accent per league, indexed like `League::ALL`; slugs a theme file
    /// leaves out are filled with `star` at parse time.
    league: [Color; 9],
    pub discipline: Discipline,
}

impl Theme {
    /// Accent color for a league (chips, LAST PLAYS, meter labels — each
    /// gated by its discipline helper below).
    pub fn league_accent(&self, league: League) -> Color {
        self.league[league_index(league)]
    }

    /// `[NFL]` chip color: the league accent, or `fg` when chips are off.
    pub fn chip(&self, league: League) -> Color {
        if self.discipline.chips {
            self.league_accent(league)
        } else {
            self.fg
        }
    }

    /// A section caption that would like to be `accent`: granted, or `muted`.
    pub fn section_label(&self, accent: Color) -> Color {
        if self.discipline.section_labels {
            accent
        } else {
            self.muted
        }
    }

    /// Team color on play-row text: granted, or `fg`.
    pub fn team_text(&self, team_color: [u8; 3]) -> Color {
        if self.discipline.play_abbrs {
            rgb(team_color)
        } else {
            self.fg
        }
    }

    /// Art (logo) color on this theme's ground: unchanged when it reads,
    /// blended toward `fg` just far enough to read when it sinks. Luma
    /// contrast can't make this call — Oilers navy is 1.29:1 on broadcast
    /// black (reads fine, hue carries it) and 1.30:1 on nord (vanishes) —
    /// so the knob is redmean color distance. See ART_FLOOR.
    pub fn art_color(&self, c: [u8; 3]) -> Color {
        let Color::Rgb(br, bg_, bb) = self.bg else { return rgb(c) };
        let bg = [br, bg_, bb];
        if redmean(c, bg) >= ART_FLOOR {
            return rgb(c);
        }
        let Color::Rgb(fr, fg_, fb) = self.fg else { return rgb(c) };
        let toward = [fr, fg_, fb];
        for step in 1..=20u32 {
            let t = step as f64 / 20.0;
            let mixed = [
                blend(c[0], toward[0], t),
                blend(c[1], toward[1], t),
                blend(c[2], toward[2], t),
            ];
            if redmean(mixed, bg) >= ART_FLOOR {
                return rgb(mixed);
            }
        }
        self.fg
    }

    /// Team color for a drawn mark (the abbr fallback when a logo is
    /// missing): primary if it reads on this ground, else the team's
    /// alternate (the brand-correct dark-ground swap), else primary lifted.
    pub fn team_mark_color(&self, primary: [u8; 3], alt: [u8; 3]) -> Color {
        if let Color::Rgb(br, bg_, bb) = self.bg {
            let bg = [br, bg_, bb];
            if redmean(primary, bg) >= ART_FLOOR {
                return rgb(primary);
            }
            if redmean(alt, bg) >= ART_FLOOR {
                return rgb(alt);
            }
        }
        self.art_color(primary)
    }

    /// League accent on play-row text (the sidebar's TOP PLAYS lines): the
    /// same knob as team color on abbrs — both are "color on play text".
    pub fn league_text(&self, league: League) -> Color {
        if self.discipline.play_abbrs {
            self.league_accent(league)
        } else {
            self.fg
        }
    }

    /// Clock digits: `cyan`, or `muted`.
    pub fn clock(&self) -> Color {
        if self.discipline.clocks {
            self.cyan
        } else {
            self.muted
        }
    }

    pub fn sidebar_header(&self, which: SidebarHeader) -> Color {
        match self.discipline.sidebar_headers {
            SidebarHeaders::Multi => match which {
                SidebarHeader::Alerts => self.live,
                SidebarHeader::TopPlays => self.star,
                SidebarHeader::Records => self.magenta,
            },
            SidebarHeaders::Single => self.star,
            SidebarHeaders::Muted => self.muted,
        }
    }
}

fn league_index(league: League) -> usize {
    League::ALL
        .iter()
        .position(|l| *l == league)
        .expect("League::ALL lists every league")
}

// ------------------------------------------------------------------ TOML

/// On-disk shape. Colors stay strings here so a bad one can be reported by
/// key ("palette.live") instead of as an anonymous serde error. Unknown
/// keys are errors at every level (`deny_unknown_fields`): a typo'd
/// discipline knob that silently took its default was the one theme-file
/// mistake the author could never see.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    palette: PaletteFile,
    #[serde(default)]
    discipline: Discipline,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteFile {
    bg: String,
    fg: String,
    bright: String,
    muted: String,
    dim: String,
    border: String,
    live: String,
    green: String,
    cyan: String,
    magenta: String,
    star: String,
    #[serde(default)]
    league: BTreeMap<String, String>,
}

/// `"#rrggbb"` → Color. The error names the key, the value and the form.
fn parse_hex(key: &str, value: &str) -> Result<Color, String> {
    let hex = value.strip_prefix('#').filter(|h| h.len() == 6);
    let parsed = hex.and_then(|h| u32::from_str_radix(h, 16).ok());
    match parsed {
        Some(v) => Ok(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        None => Err(format!("{key} = {value:?} is not a color, expected \"#rrggbb\"")),
    }
}

fn hex_of(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        // Palettes are truecolor by construction; anything else is a bug
        // upstream, but serialize something re-parseable rather than panic.
        _ => "#000000".to_string(),
    }
}

/// Parse one theme file's text. Errors name the key and the expected form;
/// unknown enum values for `sidebar_headers` list the valid set.
pub fn parse_theme(text: &str) -> Result<(String, Theme), String> {
    // toml's full Display carries the offending line (key and value) and,
    // for enums, the expected set — e.g. `sidebar_headers = "rainbow"` /
    // "unknown variant `rainbow`, expected one of `multi`, `single`, `muted`".
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.to_string().trim().to_string())?;
    let name = file.name.trim().to_string();
    if name.is_empty() {
        return Err("name = \"\" is empty, expected a short identifier like \"gruvbox\"".into());
    }
    let p = &file.palette;
    let star = parse_hex("palette.star", &p.star)?;
    let mut league = [star; 9];
    for (slug, value) in &p.league {
        let Some(l) = League::from_slug(slug) else {
            return Err(format!(
                "palette.league.{slug} is not a league, expected one of {}",
                League::ALL.map(League::slug).join("|")
            ));
        };
        league[league_index(l)] = parse_hex(&format!("palette.league.{slug}"), value)?;
    }
    Ok((
        name,
        Theme {
            bg: parse_hex("palette.bg", &p.bg)?,
            fg: parse_hex("palette.fg", &p.fg)?,
            bright: parse_hex("palette.bright", &p.bright)?,
            muted: parse_hex("palette.muted", &p.muted)?,
            dim: parse_hex("palette.dim", &p.dim)?,
            border: parse_hex("palette.border", &p.border)?,
            live: parse_hex("palette.live", &p.live)?,
            green: parse_hex("palette.green", &p.green)?,
            cyan: parse_hex("palette.cyan", &p.cyan)?,
            magenta: parse_hex("palette.magenta", &p.magenta)?,
            star,
            league,
            discipline: file.discipline,
        },
    ))
}

/// Serialize a theme in the same format `parse_theme` reads (every league
/// slug written out, so a round trip is exact).
pub fn to_toml(name: &str, th: &Theme) -> String {
    let file = ThemeFile {
        name: name.to_string(),
        palette: PaletteFile {
            bg: hex_of(th.bg),
            fg: hex_of(th.fg),
            bright: hex_of(th.bright),
            muted: hex_of(th.muted),
            dim: hex_of(th.dim),
            border: hex_of(th.border),
            live: hex_of(th.live),
            green: hex_of(th.green),
            cyan: hex_of(th.cyan),
            magenta: hex_of(th.magenta),
            star: hex_of(th.star),
            league: League::ALL
                .iter()
                .map(|l| (l.slug().to_string(), hex_of(th.league_accent(*l))))
                .collect(),
        },
        discipline: th.discipline,
    };
    toml::to_string_pretty(&file).expect("theme file shape always serializes")
}

// -------------------------------------------------------------- registry

/// One loaded theme: its canonical name, the palette, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub theme: Theme,
    /// Loaded from `<config_dir>/themes/` (or installed by a test) rather
    /// than compiled in.
    pub user: bool,
}

fn builtins() -> &'static [Entry] {
    static BUILTINS: OnceLock<Vec<Entry>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        BUILTIN_NAMES
            .iter()
            .zip(BUILTIN_TOML)
            .map(|(expected, text)| {
                let (name, theme) = parse_theme(text)
                    .unwrap_or_else(|e| panic!("built-in theme {expected} fails to parse: {e}"));
                assert_eq!(&name, expected, "assets/themes/{expected}.toml names itself {name:?}");
                Entry { name, theme, user: false }
            })
            .collect()
    })
}

thread_local! {
    // Thread-local, not process-global: draw code is single-threaded, and
    // parallel test threads each get their own user set and current theme.
    static USER: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static CURRENT: RefCell<Option<Entry>> = const { RefCell::new(None) };
}

/// Every loaded theme in picker/cycle order: the built-ins (a user file with
/// the same name replaces the built-in in its slot), then user-only themes.
pub fn entries() -> Vec<Entry> {
    let user = USER.with(|u| u.borrow().clone());
    let mut out: Vec<Entry> = builtins()
        .iter()
        .map(|b| {
            user.iter()
                .find(|u| u.name.eq_ignore_ascii_case(&b.name))
                .cloned()
                .unwrap_or_else(|| b.clone())
        })
        .collect();
    for u in user {
        if !out.iter().any(|e| e.name.eq_ignore_ascii_case(&u.name)) {
            out.push(u);
        }
    }
    out
}

pub fn names() -> Vec<String> {
    entries().into_iter().map(|e| e.name).collect()
}

/// Case-insensitive lookup by name.
pub fn lookup(name: &str) -> Option<Entry> {
    let name = name.trim();
    entries().into_iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// A built-in by name (panics on a typo — it's a programmer's constant).
pub fn builtin(name: &str) -> Theme {
    builtins()
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no built-in theme {name:?}, valid: {}", BUILTIN_NAMES.join("|")))
        .theme
}

/// Install (or replace) a user theme on this thread.
pub fn install(entry: Entry) {
    USER.with(|u| {
        let mut u = u.borrow_mut();
        u.retain(|e| !e.name.eq_ignore_ascii_case(&entry.name));
        u.push(Entry { user: true, ..entry });
    });
}

/// Read `<dir>/themes/*.toml` (sorted by file name). Returns the themes that
/// parsed and one error line per file that didn't, each naming the file.
pub fn load_user_themes(dir: &Path) -> (Vec<Entry>, Vec<String>) {
    let themes_dir = dir.join("themes");
    let Ok(read) = std::fs::read_dir(&themes_dir) else {
        return (vec![], vec![]);
    };
    let mut paths: Vec<_> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse_theme(&text));
        match result {
            Ok((name, theme)) => entries.push(Entry { name, theme, user: true }),
            Err(e) => errors.push(format!("theme file {} skipped: {e}", path.display())),
        }
    }
    (entries, errors)
}

/// Load the user's theme files and install them, reporting broken ones on
/// stderr. Never fails startup.
pub fn install_user_themes(dir: &Path) {
    let (entries, errors) = load_user_themes(dir);
    for e in errors {
        eprintln!("gameday: {e}");
    }
    for entry in entries {
        install(entry);
    }
}

fn valid_names() -> String {
    names().join("|")
}

/// Make `name` the current theme. Unknown names leave the theme alone and
/// return an error naming the value and the valid set.
pub fn set_current(name: &str) -> Result<String, String> {
    match lookup(name) {
        Some(entry) => {
            let canonical = entry.name.clone();
            CURRENT.with(|c| *c.borrow_mut() = Some(entry));
            Ok(canonical)
        }
        None => Err(format!("unknown theme {name:?}, valid: {}", valid_names())),
    }
}

/// Lenient select for config/env values: unknown names fall back to
/// broadcast with a stderr note naming the bad value and the valid set.
/// Returns the name that is now current.
pub fn select_or_default(name: &str) -> String {
    set_current(name).unwrap_or_else(|err| {
        eprintln!("gameday: {err}; using broadcast");
        set_current("broadcast").expect("broadcast is always loaded")
    })
}

fn current_entry() -> Entry {
    CURRENT.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| builtins()[0].clone())
            .clone()
    })
}

pub fn current() -> Theme {
    current_entry().theme
}

pub fn current_name() -> String {
    current_entry().name
}

/// The loaded name `delta` steps from `name` in picker order, wrapping. An
/// unknown `name` counts as "before the first" so +1 lands on broadcast.
pub fn next_name(name: &str, delta: isize) -> String {
    let all = names();
    let n = all.len() as isize;
    let i = all
        .iter()
        .position(|x| x.eq_ignore_ascii_case(name))
        .map(|i| i as isize)
        .unwrap_or(-1);
    let next = if i < 0 && delta >= 0 { (delta - 1).rem_euclid(n) } else { (i + delta).rem_euclid(n) };
    all[next as usize].clone()
}

// ---------------------------------------------------------------- helpers

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

/// Floor for `art_color`, in redmean distance. Measured on the demo slate:
/// the pairs that vanish (Yankees navy 0,43,109 on nord 103 / dracula 110 /
/// gruvbox 131; Oilers navy on the same, 92-109) sit below it, the approved
/// broadcast pairs (same navies on black, 165-207) above it.
const ART_FLOOR: f64 = 150.0;

/// Redmean color distance — cheap perceptual distance that keeps hue and
/// chroma in play where WCAG luma contrast sees nothing.
fn redmean(a: [u8; 3], b: [u8; 3]) -> f64 {
    let rm = (a[0] as f64 + b[0] as f64) / 2.0;
    let (dr, dg, db) = (
        a[0] as f64 - b[0] as f64,
        a[1] as f64 - b[1] as f64,
        a[2] as f64 - b[2] as f64,
    );
    ((2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db).sqrt()
}

fn blend(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_carry_their_identity() {
        assert_eq!(builtin("broadcast").bg, Color::Rgb(0, 0, 0));
        assert_eq!(builtin("broadcast").live, Color::Rgb(255, 60, 60));
        assert_ne!(builtin("ceefax").bg, Color::Rgb(0, 0, 0), "teletext ground is tinted");
        assert_eq!(builtin("ceefax").star, Color::Rgb(255, 238, 0));
        assert_eq!(builtin("phosphor").fg, Color::Rgb(255, 176, 0));
        // NHL keeps the single cold accent in the amber palette.
        assert_eq!(builtin("phosphor").league_accent(League::Nhl), Color::Rgb(200, 220, 220));
        assert_eq!(builtin("gruvbox").bg, Color::Rgb(0x28, 0x28, 0x28));
        assert_eq!(builtin("nord").bg, Color::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(builtin("dracula").bg, Color::Rgb(0x28, 0x2a, 0x36));
    }

    #[test]
    fn current_is_settable_per_thread() {
        assert_eq!(current_name(), "broadcast");
        set_current("phosphor").unwrap();
        assert_eq!(current(), builtin("phosphor"));
        // Another thread still sees the default.
        std::thread::spawn(|| assert_eq!(current_name(), "broadcast"))
            .join()
            .unwrap();
        set_current("broadcast").unwrap();
    }

    #[test]
    fn hex_parse_rejects_short_and_non_hex_values() {
        assert_eq!(parse_hex("k", "#0a0B0c").unwrap(), Color::Rgb(10, 11, 12));
        for bad in ["#fff", "ffffff", "#gggggg", "", "#12345678"] {
            let err = parse_hex("palette.k", bad).unwrap_err();
            assert!(err.contains("palette.k") && err.contains("#rrggbb"), "{err}");
        }
    }

    #[test]
    fn unknown_keys_are_errors_that_name_the_key() {
        // A typo'd knob must not silently take its default.
        let base = to_toml("x", &builtin("nord"));
        for (typo, at) in [
            ("play_abbr = true", "[discipline]"),
            ("section_label = false", "[discipline]"),
            ("brite = \"#ffffff\"", "[palette]"),
            ("nmae = \"x\"", ""),
        ] {
            let text = if at.is_empty() {
                format!("{typo}\n{base}")
            } else {
                base.replacen(at, &format!("{at}\n{typo}"), 1)
            };
            let key = typo.split(' ').next().unwrap();
            let err = parse_theme(&text).unwrap_err();
            assert!(err.contains(key), "error for {typo:?} must name {key:?}: {err}");
        }
        // The valid file still parses, so the check is not just "fails".
        assert!(parse_theme(&base).is_ok());
    }

    #[test]
    fn every_builtin_live_role_is_red() {
        // The identity floor: LIVE, the scoring words, the ticker frame and
        // the RED ZONE gauge all render in `live`, and every theme must read
        // them as red. "Red" = the red channel leads green and blue by at
        // least 64/255 — a guess wide enough for nord's rose and rose-pine's
        // pink, tight enough to reject phosphor's old cream (#fff0be).
        const LEAD: i32 = 64;
        for name in BUILTIN_NAMES {
            let Color::Rgb(r, g, b) = builtin(name).live else {
                panic!("{name}: live is not truecolor");
            };
            let (r, g, b) = (r as i32, g as i32, b as i32);
            assert!(
                r - g >= LEAD && r - b >= LEAD,
                "{name}: live #{r:02x}{g:02x}{b:02x} is not red (red must lead g and b by >= {LEAD})"
            );
        }
    }

    #[test]
    fn art_color_lifts_only_what_sinks_into_the_ground() {
        // The measured pairs behind ART_FLOOR: Yankees art navy (0,43,109)
        // reads on broadcast black (redmean 207) but vanishes on nord
        // (103) / dracula (110) / gruvbox (131).
        set_current("nord").unwrap();
        let navy = [0u8, 43, 109];
        let lifted = current().art_color(navy);
        assert_ne!(lifted, rgb(navy), "sunk color must be remapped");
        let white = current().art_color([237, 237, 237]);
        assert_eq!(white, rgb([237, 237, 237]), "contrasting art is untouched");
        set_current("broadcast").unwrap();
        assert_eq!(current().art_color(navy), rgb(navy), "navy reads on black");
    }

    #[test]
    fn team_mark_color_cascades_primary_alt_lift() {
        set_current("nord").unwrap();
        let th = current();
        // Primary clears the floor: used as-is.
        assert_eq!(th.team_mark_color([237, 237, 237], [0, 43, 109]), rgb([237, 237, 237]));
        // Primary sinks, alt clears: brand-correct fallback.
        assert_eq!(th.team_mark_color([0, 43, 109], [237, 237, 237]), rgb([237, 237, 237]));
        // Both sink: lift rather than vanish.
        let both = th.team_mark_color([0, 43, 109], [10, 50, 100]);
        assert_ne!(both, rgb([0, 43, 109]));
        assert_ne!(both, rgb([10, 50, 100]));
        set_current("broadcast").unwrap();
    }

    /// WCAG 2 relative luminance of a truecolor.
    fn rel_luma(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else { panic!("not truecolor") };
        let lin = |v: u8| {
            let s = v as f64 / 255.0;
            if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (rel_luma(a), rel_luma(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn every_builtin_muted_text_clears_3_to_1_on_its_ground() {
        // `muted` carries data (play timestamps, the nav bar, table headers),
        // not decoration, so it must clear the WCAG large-text floor of 3:1
        // against `bg`. tokyo-night's upstream comment gray (#565f89) sat at
        // 2.76 and was stepped up one shade.
        for name in BUILTIN_NAMES {
            let th = builtin(name);
            let ratio = contrast(th.muted, th.bg);
            assert!(
                ratio >= 3.0,
                "{name}: muted {} on bg {} is {ratio:.2}:1, expected >= 3.0:1",
                hex_of(th.muted),
                hex_of(th.bg)
            );
        }
    }

    #[test]
    fn league_slugs_in_a_theme_file_are_validated() {
        let text = to_toml("x", &builtin("nord")).replace("nfl = ", "xfl = ");
        let err = parse_theme(&text).unwrap_err();
        assert!(err.contains("palette.league.xfl") && err.contains("nfl|cfb"), "{err}");
    }
}
