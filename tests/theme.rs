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

/// A scratch config dir for one test. `name` MUST be unique per test: the
/// dir is keyed by name + pid only, and `tmp` wipes it on entry, so two tests
/// sharing a name race under the default (parallel) harness — one test's
/// `tmp` deletes the other's theme files mid-run.
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
fn broadcast_is_loud_and_studio_is_the_press_box() {
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
    // v3.3 §6: studio stopped being broadcast's palette. It is the press box —
    // its own grayscale set, so every one of the eleven colors moves except
    // the red, and every league accent goes gray.
    for (label, a, c) in [
        ("bg", b.bg, s.bg),
        ("fg", b.fg, s.fg),
        ("bright", b.bright, s.bright),
        ("muted", b.muted, s.muted),
        ("dim", b.dim, s.dim),
        ("border", b.border, s.border),
        ("green", b.green, s.green),
        ("cyan", b.cyan, s.cyan),
        ("magenta", b.magenta, s.magenta),
        ("star", b.star, s.star),
    ] {
        assert_ne!(a, c, "studio is no longer broadcast's palette: {label}");
    }
    for league in League::ALL {
        assert_ne!(b.league_accent(league), s.league_accent(league), "{league:?} accent");
    }
    // Different at the ROLE layer too: studio's scores are white where
    // broadcast's are amber. (Before v3.2 the two themes had identical
    // `[roles]`, which made "studio" a discipline flag rather than a look.)
    assert_ne!(b.roles(), s.roles(), "studio must differ as a role mapping");
    assert_eq!(b.roles().digits, b.star, "broadcast scores are amber");
    assert_eq!(s.roles().digits, s.bright, "studio scores are white");
    assert_eq!(s.roles().cool, s.border, "studio's structure is gray, not colored");
    // The identity floor holds in both: red is red. The ground does not —
    // press-box studio sits a step off true black.
    assert_eq!(b.roles().hot, s.roles().hot, "one red, shared");
    assert_eq!(s.live, Color::Rgb(255, 60, 60), "and it is broadcast's red");
    assert_ne!(b.roles().ground, s.roles().ground);
    // The one team-color scope studio keeps: `hero`. `never` would be a claim
    // the board cannot honor — the drawn difference between `hero` and `never`
    // is nothing (only `hero+marks` is read, in `board::rows`), and hero team
    // color is the documented identity floor, not chrome.
    assert_eq!(s.roles().team, theme::TeamColorScope::Hero);
}

/// v3.2 §6 cut the built-ins from eleven to three, and the README promises
/// the other eight still work — as user files. That promise is only true if
/// the retired TOMLs still *parse and select* under the v3.2 theme format
/// (they carry `[discipline]` tables, which `deny_unknown_fields` would
/// reject the moment those keys were deleted). One retired name is enough to
/// pin it: they all ship from the same directory in the same shape.
#[test]
fn a_retired_builtin_still_loads_and_selects_as_a_user_theme() {
    let dir = tmp("retired-one");
    fs::write(
        dir.join("themes/nord.toml"),
        include_str!("../assets/themes/nord.toml"),
    )
    .unwrap();
    assert!(theme::lookup("nord").is_none(), "nord is not a built-in any more");
    theme::install_user_themes(&dir);
    let entry = theme::lookup("nord").expect("nord loads as a user file");
    assert!(entry.user, "and is reported as a user theme");
    assert_eq!(theme::set_current("nord").unwrap(), "nord");
    assert_eq!(theme::current(), entry.theme);
    assert!(theme::names().iter().any(|n| n == "nord"), "it is offered in the picker");
    theme::set_current("broadcast").unwrap();
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
    let dir = tmp("retired-eight");
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
    assert!(!err.contains("never"), "`never` is retired and must not be offered: {err}");
}

/// v3.3 deleted `TeamColorScope::Never`: nothing read it, so `never` and
/// `hero` drew the same pixels and the config value was a lie. A user theme
/// on disk that still says it must keep loading — as the value it always
/// behaved as, not as an error and not as a different board.
#[test]
fn a_theme_file_still_saying_team_never_loads_as_hero() {
    let base = theme::to_toml("mine", &theme::builtin("broadcast"));
    let retired = base.replace("team = \"hero\"", "team = \"never\"");
    assert!(retired.contains("team = \"never\""), "the fixture must actually say never");
    let (name, th) = theme::parse_theme(&retired).expect("a retired scope must not fail the load");
    assert_eq!(name, "mine");
    assert_eq!(
        th.roles().team,
        theme::TeamColorScope::Hero,
        "`never` maps to the scope it always drew as"
    );
    // And round-trips as the honest spelling, so re-saving the file heals it.
    assert!(theme::to_toml("mine", &th).contains("team = \"hero\""));
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
fn clock_knob_maps_to_a_palette_color() {
    // v3.2 §7: the sidebar is deleted, and its `sidebar_header`/`league_text`
    // knobs went with it. `clocks` is one of the four discipline knobs the
    // surviving chrome still spends.
    let mut th = theme::builtin("broadcast");
    th.discipline.clocks = true;
    assert_eq!(th.clock(), th.cyan);
    th.discipline.clocks = false;
    assert_eq!(th.clock(), th.muted);
}
