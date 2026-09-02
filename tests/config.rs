use gameday::config::{
    load_config, load_pins, load_pins_outcome, prune_pins, resolve_dir, save_pins, Config,
    Favorite, Pin,
};
use gameday::domain::League;
use gameday::config::LayoutPref;
use gameday::tiles::ScoreStyle;
use std::fs;
use time::{Duration, OffsetDateTime};

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gameday-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn roundtrip_config() {
    let dir = tmp("cfg");
    let c = Config {
        enabled_tabs: vec![League::Nfl, League::Cfb],
        layout: LayoutPref::Two,
        favorites: vec![Favorite { league: League::Nfl, team_abbr: "KC".into() }],
        theme: "ceefax".into(),
        score_style: gameday::tiles::ScoreStyle::Compact,
        sort: Default::default(),
    };
    c.save_to(&dir).unwrap();
    let loaded = Config::load_from(&dir).unwrap();
    assert_eq!(loaded.favorites[0].team_abbr, "KC");
    assert_eq!(loaded.layout, LayoutPref::Two);
    assert_eq!(loaded.enabled_tabs, vec![League::Nfl, League::Cfb]);
    assert_eq!(loaded.theme, "ceefax");
    assert_eq!(loaded.score_style, ScoreStyle::Compact);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn config_without_score_style_key_defaults_to_big() {
    // Configs written before score_style existed must keep loading, and the
    // TOML value is the lowercase word from the spec ("big"/"compact").
    let dir = tmp("cfg-no-score-style");
    fs::write(
        dir.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nlayout = \"Auto\"\nfavorites = []\n",
    )
    .unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.score_style, ScoreStyle::Big);
    c.save_to(&dir).unwrap();
    let text = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(text.contains("score_style = \"big\""), "{text}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn config_without_theme_key_defaults_to_broadcast() {
    // Configs written before the theme field existed must keep loading.
    let dir = tmp("cfg-no-theme");
    fs::write(
        dir.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nlayout = \"Auto\"\nfavorites = []\n",
    )
    .unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.theme, "broadcast");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_config_enables_every_league() {
    let dir = tmp("missing");
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.enabled_tabs, League::ALL.to_vec());
    assert_eq!(c.layout, LayoutPref::Auto);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn new_league_variants_roundtrip_in_toml() {
    let dir = tmp("cfg-new-leagues");
    let c = Config {
        enabled_tabs: vec![League::Wnba, League::Epl, League::Mls],
        layout: LayoutPref::Auto,
        favorites: vec![],
        theme: "broadcast".into(),
        score_style: Default::default(),
        sort: Default::default(),
    };
    c.save_to(&dir).unwrap();
    let loaded = Config::load_from(&dir).unwrap();
    assert_eq!(loaded.enabled_tabs, vec![League::Wnba, League::Epl, League::Mls]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prune_drops_final_after_six_hours() {
    let now = OffsetDateTime::now_utc();
    let old = Pin { game_id: "1".into(), league: League::Nfl, final_at: Some(now - Duration::hours(7)) };
    let keep = Pin { game_id: "2".into(), league: League::Nfl, final_at: Some(now - Duration::hours(1)) };
    let live = Pin { game_id: "3".into(), league: League::Nfl, final_at: None };
    let out = prune_pins(vec![old, keep, live], now);
    let ids: Vec<_> = out.iter().map(|p| p.game_id.as_str()).collect();
    assert_eq!(ids, vec!["2", "3"]);
}

#[test]
fn xdg_wins_then_dot_config_then_legacy_is_read_only() {
    let home = tmp("home");
    let legacy = home.join("Library/Application Support/gameday");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nlayout = \"Auto\"\nfavorites = []\n",
    )
    .unwrap();
    // No ~/.config/gameday yet: dir is the new path, reads fall back to legacy.
    let r = resolve_dir(None, &home, None, Some(&legacy));
    assert_eq!(r.dir, home.join(".config/gameday"));
    assert_eq!(r.legacy_read_from.as_deref(), Some(legacy.as_path()));
    let out = load_config(&r);
    assert_eq!(out.value.enabled_tabs, vec![League::Nfl]);
    assert!(out.error.is_none());
    // XDG_CONFIG_HOME set: it wins outright.
    let xdg = home.join("xdg");
    let r = resolve_dir(None, &home, Some(&xdg), Some(&legacy));
    assert_eq!(r.dir, xdg.join("gameday"));
    // --config-dir beats everything and never consults legacy.
    let r = resolve_dir(Some(home.join("custom")), &home, Some(&xdg), Some(&legacy));
    assert_eq!(r.dir, home.join("custom"));
    assert!(r.legacy_read_from.is_none());
    fs::remove_dir_all(&home).ok();
}

/// On Linux `dirs::config_dir()` IS `~/.config`, so the "legacy" path handed
/// in equals the resolved dir. A dir is never its own legacy — otherwise every
/// Linux user would be told to move their folder into itself.
#[test]
fn a_dir_is_never_its_own_legacy() {
    let home = tmp("self-legacy");
    let xdg = home.join(".config");
    let legacy = xdg.join("gameday");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nlayout = \"Auto\"\nfavorites = []\n",
    )
    .unwrap();
    let r = resolve_dir(None, &home, Some(&xdg), Some(&legacy));
    assert_eq!(r.dir, legacy);
    assert!(r.legacy_read_from.is_none(), "{:?}", r.legacy_read_from);
    fs::remove_dir_all(&home).ok();
}

/// pins.json gets the same treatment as config.toml: unreadable means empty in
/// memory, an error naming the file, and no write that would eat the pins.
#[test]
fn broken_pins_report_the_file_and_block_the_next_pin_save() {
    let dir = tmp("broken-pins");
    fs::write(dir.join("pins.json"), "[{\"game_id\": ").unwrap();
    let r = resolve_dir(Some(dir.clone()), &dir, None, None);
    let out = load_pins_outcome(&r);
    assert!(out.value.is_empty());
    let err = out.error.clone().expect("error reported");
    assert!(err.contains("pins.json"), "{err}");
    let before = fs::read_to_string(dir.join("pins.json")).unwrap();
    let mut app = gameday::app::App::new(
        Config::default_all(),
        vec![],
        dir.clone(),
        time::UtcOffset::UTC,
    );
    app.set_config_error(out.error);
    app.pins.push(Pin {
        game_id: "1".into(),
        league: League::Nfl,
        final_at: None,
    });
    app.persist_pins();
    assert_eq!(
        fs::read_to_string(dir.join("pins.json")).unwrap(),
        before,
        "file untouched"
    );
    assert!(
        app.status_line
            .as_deref()
            .unwrap_or("")
            .contains("not saving"),
        "{:?}",
        app.status_line
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_broken_config_loads_defaults_reports_the_line_and_is_never_overwritten() {
    let dir = tmp("broken");
    fs::write(
        dir.join("config.toml"),
        "enabled_tabs = [\"NFLL\"]\nlayout = \"Auto\"\nfavorites = []\n",
    )
    .unwrap();
    let r = resolve_dir(Some(dir.clone()), &dir, None, None);
    let out = load_config(&r);
    assert_eq!(out.value, Config::default_all(), "defaults in memory");
    let err = out.error.clone().expect("error reported");
    assert!(
        err.contains("config.toml") && err.contains("NFLL") && err.contains("nfl|cfb"),
        "{err}"
    );
    let before = fs::read_to_string(dir.join("config.toml")).unwrap();
    let mut app = gameday::app::App::new(out.value, vec![], dir.clone(), time::UtcOffset::UTC);
    app.config_error = out.error;
    // '2' sets the two-up layout, which would persist the config.
    app.on_key(
        crossterm::event::KeyCode::Char('2'),
        crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(
        fs::read_to_string(dir.join("config.toml")).unwrap(),
        before,
        "file untouched"
    );
    assert!(
        app.status_line
            .as_deref()
            .unwrap_or("")
            .contains("not saving"),
        "{:?}",
        app.status_line
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn pins_roundtrip() {
    let dir = tmp("pins");
    let pins = vec![Pin { game_id: "9".into(), league: League::Nfl, final_at: None }];
    save_pins(&dir, &pins).unwrap();
    let loaded = load_pins(&dir).unwrap();
    assert_eq!(loaded[0].game_id, "9");
    fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore = "fields removed in the keys task"]
fn sort_key_round_trips_and_old_layout_keys_are_ignored() {
    let dir = tmp("sortkey");
    fs::write(dir.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nfavorites = []\nsort = \"time\"\nlayout = \"Auto\"\nscore_style = \"big\"\n").unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.sort, gameday::rank::SortKey::Time);
    // layout/score_style are gone from the struct; unknown keys parse fine.
    c.save_to(&dir).unwrap();
    let text = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(text.contains("sort = \"time\""));
    assert!(!text.contains("layout"), "removed key is not re-written");
    fs::remove_dir_all(&dir).ok();
}
