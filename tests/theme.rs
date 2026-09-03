//! Themes as identities: every built-in is a TOML file that round-trips, user
//! files override built-ins by name, a bad user file is skipped with an error
//! naming the file/key/expected form, and the discipline knobs visibly change
//! specific cells of a rendered tile.

use gameday::domain::{Game, League, Meter, Play, Situation, Status, Team};
use gameday::theme::{self, Discipline, Entry, SidebarHeaders, Theme};
use gameday::app::App;
use gameday::config::Config;
use gameday::views::{View, ZoomTab};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use std::fs;

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gameday-theme-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("themes")).unwrap();
    dir
}

#[test]
fn the_three_builtins_ship_in_the_decided_order() {
    assert_eq!(theme::BUILTIN_NAMES, ["broadcast", "studio", "gruvbox"]);
    assert_eq!(theme::names(), theme::BUILTIN_NAMES.map(String::from).to_vec());
}

#[test]
fn every_builtin_round_trips_through_toml() {
    for name in theme::BUILTIN_NAMES {
        let entry = theme::lookup(name).unwrap_or_else(|| panic!("builtin {name} missing"));
        assert_eq!(entry.name, name);
        let text = theme::to_toml(&entry.name, &entry.theme);
        let (back_name, back) = theme::parse_theme(&text)
            .unwrap_or_else(|e| panic!("{name} does not re-parse from its own TOML: {e}\n{text}"));
        assert_eq!(back_name, name);
        assert_eq!(back, entry.theme, "{name} palette/discipline changed across a round trip");
        // Truecolor only — no ANSI-16 leaks into any palette role or accent.
        for league in League::ALL {
            assert!(matches!(entry.theme.league_accent(league), Color::Rgb(..)), "{name}/{league:?}");
        }
    }
}

#[test]
fn broadcast_is_loud_and_studio_is_the_same_palette_disciplined() {
    let b = theme::builtin("broadcast");
    let s = theme::builtin("studio");
    assert_eq!(b.bg, Color::Rgb(0, 0, 0));
    assert_eq!(b.live, Color::Rgb(255, 60, 60));
    assert_eq!(
        b.discipline,
        Discipline {
            chips: true,
            section_labels: true,
            play_abbrs: true,
            clocks: true,
            sidebar_headers: SidebarHeaders::Multi,
        }
    );
    assert_eq!(
        s.discipline,
        Discipline {
            chips: true,
            section_labels: false,
            play_abbrs: false,
            clocks: false,
            sidebar_headers: SidebarHeaders::Muted,
        }
    );
    // Same palette, different discipline.
    let mut s_as_b = s;
    s_as_b.discipline = b.discipline;
    assert_eq!(s_as_b, b, "studio must be broadcast's palette");
}

#[test]
fn missing_league_slug_falls_back_to_star() {
    let text = r##"
name = "minimal"
[palette]
bg = "#000000"
fg = "#cccccc"
bright = "#ffffff"
muted = "#777777"
dim = "#333333"
border = "#555555"
live = "#ff0000"
green = "#00ff00"
cyan = "#00ffff"
magenta = "#ff00ff"
star = "#ffcc00"
[palette.league]
nfl = "#ff4444"
"##;
    let (name, th) = theme::parse_theme(text).unwrap();
    assert_eq!(name, "minimal");
    assert_eq!(th.league_accent(League::Nfl), Color::Rgb(255, 68, 68));
    assert_eq!(th.league_accent(League::Nhl), th.star, "missing slug -> star");
    // No [discipline] table: the calm defaults from the spec example.
    assert_eq!(
        th.discipline,
        Discipline {
            chips: true,
            section_labels: false,
            play_abbrs: false,
            clocks: false,
            sidebar_headers: SidebarHeaders::Single,
        }
    );
}

#[test]
fn parse_errors_name_the_key_and_the_expected_form() {
    let bad_color = r##"
name = "oops"
[palette]
bg = "#000000"
fg = "#cccccc"
bright = "#ffffff"
muted = "#777777"
dim = "#333333"
border = "#555555"
live = "red"
green = "#00ff00"
cyan = "#00ffff"
magenta = "#ff00ff"
star = "#ffcc00"
"##;
    let err = theme::parse_theme(bad_color).unwrap_err();
    assert!(err.contains("palette.live"), "must name the key: {err}");
    assert!(err.contains("\"red\""), "must name the value: {err}");
    assert!(err.contains("#rrggbb"), "must name the expected form: {err}");

    let bad_mode = r##"
name = "oops"
[palette]
bg = "#000000"
fg = "#cccccc"
bright = "#ffffff"
muted = "#777777"
dim = "#333333"
border = "#555555"
live = "#ff0000"
green = "#00ff00"
cyan = "#00ffff"
magenta = "#ff00ff"
star = "#ffcc00"
[discipline]
sidebar_headers = "rainbow"
"##;
    let err = theme::parse_theme(bad_mode).unwrap_err();
    assert!(err.contains("sidebar_headers") && err.contains("rainbow"), "{err}");
    assert!(err.contains("multi") && err.contains("single") && err.contains("muted"), "{err}");

    let missing = "name = \"x\"\n[palette]\nbg = \"#000000\"\n";
    let err = theme::parse_theme(missing).unwrap_err();
    assert!(err.contains("fg") || err.contains("missing field"), "{err}");
}

#[test]
fn user_theme_overrides_builtin_and_bad_files_are_skipped_with_a_note() {
    let dir = tmp("user");
    // A user "gruvbox" with a different ground wins over the built-in.
    let mut gruv = theme::to_toml("gruvbox", &theme::builtin("gruvbox"));
    gruv = gruv.replace("bg = \"#282828\"", "bg = \"#1d2021\"");
    fs::write(dir.join("themes/gruvbox.toml"), gruv).unwrap();
    // A user-only theme.
    fs::write(
        dir.join("themes/mine.toml"),
        theme::to_toml("mine", &theme::builtin("studio")),
    )
    .unwrap();
    // A broken one: skipped, never fatal.
    fs::write(dir.join("themes/broken.toml"), "name = \"broken\"\n[palette]\nbg = \"nope\"\n").unwrap();
    // Not a .toml file: ignored.
    fs::write(dir.join("themes/notes.txt"), "hello").unwrap();

    let (entries, errors) = theme::load_user_themes(&dir);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["gruvbox", "mine"], "sorted by file name, broken skipped");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("broken.toml"), "error names the file: {}", errors[0]);

    theme::install_user_themes(&dir);
    let gruv = theme::lookup("gruvbox").unwrap();
    assert!(gruv.user, "user file wins on a name clash");
    assert_eq!(gruv.theme.bg, Color::Rgb(0x1d, 0x20, 0x21));
    let names = theme::names();
    assert_eq!(names.len(), theme::BUILTIN_NAMES.len() + 1, "{names:?}");
    assert_eq!(names.last().map(String::as_str), Some("mine"), "user-only names follow the built-ins");
    assert_eq!(
        names.iter().position(|n| n == "gruvbox"),
        Some(2),
        "an overriding user theme keeps the built-in's slot"
    );
    theme::set_current("MINE").unwrap();
    assert_eq!(theme::current_name(), "mine", "names are case-insensitive, canonical on read");
    assert_eq!(theme::current(), theme::builtin("studio"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_eight_retired_palettes_still_load_as_user_files() {
    // v3.2 decision A cut the built-in list to three, but the files stayed in
    // assets/themes/ — dropping them out of `include_str!` must not make them
    // unloadable, and every one of them gets the documented default roles.
    let dir = tmp("retired");
    for (name, text) in [
        ("ceefax", include_str!("../assets/themes/ceefax.toml")),
        ("phosphor", include_str!("../assets/themes/phosphor.toml")),
        ("tokyo-night", include_str!("../assets/themes/tokyo-night.toml")),
        ("nord", include_str!("../assets/themes/nord.toml")),
        ("catppuccin-mocha", include_str!("../assets/themes/catppuccin-mocha.toml")),
        ("rose-pine", include_str!("../assets/themes/rose-pine.toml")),
        ("everforest", include_str!("../assets/themes/everforest.toml")),
        ("dracula", include_str!("../assets/themes/dracula.toml")),
    ] {
        fs::write(dir.join(format!("themes/{name}.toml")), text).unwrap();
        let (parsed, th) = theme::parse_theme(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parsed, name);
        let r = th.roles();
        assert_eq!((r.ground, r.ink, r.dim), (th.bg, th.fg, th.muted), "{name} default roles");
        assert_eq!((r.digits, r.hot, r.cool), (th.star, th.live, th.border), "{name} default roles");
    }
    let (entries, errors) = theme::load_user_themes(&dir);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(entries.len(), 8, "every retired palette loads from disk");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_theme_file_can_reassign_roles_and_scope_team_color() {
    let base = theme::to_toml("mine", &theme::builtin("broadcast"));
    let text = base
        .replace("digits = \"star\"", "digits = \"cyan\"")
        .replace("team = \"hero\"", "team = \"hero+marks\"");
    let (_, th) = theme::parse_theme(&text).unwrap();
    assert_eq!(th.roles().digits, th.cyan, "the role follows the palette key it names");
    assert_eq!(th.roles().team, theme::TeamColorScope::HeroMarks);

    let bad_key = base.replace("hot = \"live\"", "hot = \"crimson\"");
    let err = theme::parse_theme(&bad_key).unwrap_err();
    assert!(err.contains("roles.hot") && err.contains("\"crimson\""), "{err}");
    assert!(err.contains("live") && err.contains("star"), "the valid set: {err}");

    let bad_scope = base.replace("team = \"hero\"", "team = \"everywhere\"");
    let err = theme::parse_theme(&bad_scope).unwrap_err();
    assert!(err.contains("roles.team") && err.contains("hero+marks"), "{err}");
}

#[test]
fn unknown_names_fall_back_to_broadcast_and_errors_name_the_valid_set() {
    let err = theme::set_current("solarized").unwrap_err();
    assert!(err.contains("\"solarized\""), "{err}");
    assert!(err.contains("broadcast|studio|gruvbox"), "{err}");
    assert_eq!(theme::current_name(), "broadcast", "a failed set leaves the theme alone");
    theme::set_current("gruvbox").unwrap();
    assert_eq!(theme::select_or_default("nope"), "broadcast");
    assert_eq!(theme::current_name(), "broadcast");
    assert_eq!(theme::select_or_default("GruvBox"), "gruvbox");
    assert_eq!(theme::current(), theme::builtin("gruvbox"));
}

#[test]
fn next_name_cycles_the_loaded_set_both_ways() {
    assert_eq!(theme::next_name("broadcast", 1), "studio");
    assert_eq!(theme::next_name("gruvbox", 1), "broadcast", "wraps forward");
    assert_eq!(theme::next_name("broadcast", -1), "gruvbox", "wraps backward");
    assert_eq!(theme::next_name("unknown", 1), "broadcast", "unknown restarts at the top");
}

// ---- discipline changes specific cells --------------------------------------

fn demo_game() -> Game {
    Game {
        id: "t1".into(),
        league: League::Nfl,
        away: Team {
            id: "12".into(),
            abbr: "KC".into(),
            name: "Chiefs".into(),
            location: "Kansas City".into(),
            record: "11-6".into(),
            color: [227, 24, 55],
            alt_color: [230, 230, 230],
            logo_key: "nfl/kc".into(),
            ..Default::default()
        },
        home: Team {
            id: "27".into(),
            abbr: "TB".into(),
            name: "Buccaneers".into(),
            location: "Tampa Bay".into(),
            record: "11-6".into(),
            color: [213, 10, 10],
            alt_color: [230, 230, 230],
            logo_key: "nfl/tb".into(),
            ..Default::default()
        },
        away_score: 27,
        home_score: 24,
        status: Status::Live,
        period: "Q4".into(),
        clock: "1:27".into(),
        situation: Some(Situation {
            down_distance: "1st & Goal".into(),
            possession: Some("KC".into()),
            ball_on: Some("TB 3".into()),
            ..Default::default()
        }),
        last_plays: vec![Play {
            clock: "1:27".into(),
            team: "KC".into(),
            text: "Mahomes pass to Kelce for 3 yards".into(),
            scoring: false,
            ..Default::default()
        }],
        meter: Some(Meter::Lead { plus_minus: 3 }),
        broadcast: Some("CBS".into()),
        ..Game::default()
    }
}

/// The zoom's Overview tab — the surface that still carries all three of the
/// rendered discipline grants (a section label, a meter label, a play abbr)
/// now that Task 13 deleted the tile grammar those cells used to live in.
fn render(game: &Game) -> Buffer {
    let dir = std::env::temp_dir().join(format!("gameday-theme-render-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
    app.apply_boards(game.league, vec![game.clone()], false);
    app.view = View::Zoom { game_id: game.id.clone(), tab: ZoomTab::Overview };
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    term.backend().buffer().clone()
}

/// fg of the first cell of the first occurrence of `needle` on the buffer.
fn fg_of(buf: &Buffer, needle: &str) -> Color {
    let area = *buf.area();
    for y in 0..area.height {
        let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<Vec<_>>().join("");
        if let Some(pos) = row.find(needle) {
            let x = row[..pos].chars().count() as u16;
            return buf[(x, y)].fg;
        }
    }
    panic!("{needle:?} not rendered");
}

fn install_variant(name: &str, discipline: Discipline) -> Theme {
    let mut th = theme::builtin("broadcast");
    th.discipline = discipline;
    theme::install(Entry { name: name.into(), theme: th, user: true });
    theme::set_current(name).unwrap();
    th
}

#[test]
fn discipline_toggles_recolor_section_label_meter_and_play_abbr_cells() {
    let game = demo_game();
    let loud = install_variant(
        "loud",
        Discipline {
            chips: true,
            section_labels: true,
            play_abbrs: true,
            clocks: true,
            sidebar_headers: SidebarHeaders::Multi,
        },
    );
    let buf = render(&game);
    // v3.2 §7: the `[NFL]` tile chip and the tile's `27 - 24` score row died
    // with the tile grammar (Task 13); the three grants that still render are
    // the section label, the meter label and the play abbr.
    assert_eq!(fg_of(&buf, "LAST PLAYS"), loud.league_accent(League::Nfl), "section label in accent");
    assert_eq!(fg_of(&buf, "LEAD"), loud.league_accent(League::Nfl), "meter label in accent");
    assert_eq!(fg_of(&buf, "KC  Mahomes"), theme::rgb([227, 24, 55]), "play abbr in team color");

    let quiet = install_variant(
        "quiet",
        Discipline {
            chips: false,
            section_labels: false,
            play_abbrs: false,
            clocks: false,
            sidebar_headers: SidebarHeaders::Muted,
        },
    );
    let buf = render(&game);
    assert_eq!(fg_of(&buf, "LAST PLAYS"), quiet.muted, "section_labels=false: label muted");
    assert_eq!(fg_of(&buf, "LEAD"), quiet.muted, "meter label muted");
    assert_eq!(fg_of(&buf, "KC  Mahomes"), quiet.fg, "play_abbrs=false: abbr in fg");
    theme::set_current("broadcast").unwrap();
}

#[test]
fn sidebar_header_modes_and_clock_knob_map_to_roles() {
    let mut th = theme::builtin("broadcast");
    th.discipline.sidebar_headers = SidebarHeaders::Multi;
    assert_eq!(th.sidebar_header(theme::SidebarHeader::Alerts), th.live);
    assert_eq!(th.sidebar_header(theme::SidebarHeader::TopPlays), th.star);
    assert_eq!(th.sidebar_header(theme::SidebarHeader::Records), th.magenta);
    th.discipline.sidebar_headers = SidebarHeaders::Single;
    for h in [theme::SidebarHeader::Alerts, theme::SidebarHeader::TopPlays, theme::SidebarHeader::Records] {
        assert_eq!(th.sidebar_header(h), th.star, "{h:?} single -> star");
    }
    th.discipline.sidebar_headers = SidebarHeaders::Muted;
    assert_eq!(th.sidebar_header(theme::SidebarHeader::Records), th.muted);
    th.discipline.clocks = true;
    assert_eq!(th.clock(), th.cyan);
    th.discipline.clocks = false;
    assert_eq!(th.clock(), th.muted);
}
