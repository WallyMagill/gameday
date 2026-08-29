use gameday::config::{load_pins, prune_pins, save_pins, Config, Favorite, Pin};
use gameday::domain::League;
use gameday::tiles::packer::LayoutPref;
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
fn pins_roundtrip() {
    let dir = tmp("pins");
    let pins = vec![Pin { game_id: "9".into(), league: League::Nfl, final_at: None }];
    save_pins(&dir, &pins).unwrap();
    let loaded = load_pins(&dir).unwrap();
    assert_eq!(loaded[0].game_id, "9");
    fs::remove_dir_all(&dir).ok();
}
