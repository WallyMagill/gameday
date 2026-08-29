use gameday::domain::*;
use gameday::tiles::packer::{pack, LayoutPref};
use gameday::tiles::Density;
use ratatui::layout::Rect;

fn g(id: &str, live: bool) -> Game {
    let team = |abbr: &str| Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color: [255, 255, 255],
        alt_color: [0, 0, 0],
        logo_key: format!("nfl/{}", abbr.to_lowercase()),
    };
    Game {
        id: id.into(),
        league: League::Nfl,
        away: team("A"),
        home: team("B"),
        away_score: 1,
        home_score: 0,
        status: if live { Status::Live } else { Status::Pre },
        period: "Q1".into(),
        clock: "15:00".into(),
        situation: None,
        last_plays: vec![],
        meter: None,
        start_time: None,
        broadcast: None,
    }
}

#[test]
fn auto_one_game_is_full() {
    let games = [g("1", true)];
    let out = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Auto, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].density, Density::Full);
    assert_eq!(out[0].area.width, 120);
}

#[test]
fn auto_two_splits_standard() {
    let games = [g("1", true), g("2", true)];
    let out = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Auto, 0);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|t| t.density == Density::Standard));
    assert_eq!(out[0].area.width, 60);
    assert_eq!(out[1].area.x, 60);
}

#[test]
fn auto_four_is_grid() {
    let games: Vec<_> = (0..4).map(|i| g(&i.to_string(), true)).collect();
    let out = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Auto, 0);
    assert_eq!(out.len(), 4);
    assert!(out.iter().all(|t| t.density == Density::Standard));
}

#[test]
fn five_games_page_zero_shows_four() {
    let games: Vec<_> = (0..5).map(|i| g(&i.to_string(), true)).collect();
    let p0 = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Auto, 0);
    assert_eq!(p0.len(), 4);
    assert_eq!(p0[0].game.id, "0");
    let p1 = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Auto, 1);
    assert_eq!(p1.len(), 1);
    assert_eq!(p1[0].game.id, "4");
}

#[test]
fn sidebar_or_narrow_is_compact_stack() {
    let games = [g("1", true), g("2", true)];
    let out = pack(&games, Rect::new(0, 0, 50, 30), LayoutPref::Auto, 0);
    assert!(out.iter().all(|t| t.density == Density::Compact));
    assert_eq!(out[0].area.width, 50);
    assert_eq!(out[1].area.y, out[0].area.y + out[0].area.height);

    let forced = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Sidebar, 0);
    assert!(forced.iter().all(|t| t.density == Density::Compact));
}

#[test]
fn force_four_on_one_game() {
    let games = [g("1", true)];
    let out = pack(&games, Rect::new(0, 0, 120, 30), LayoutPref::Four, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].density, Density::Standard);
}

#[test]
fn two_pane_stacks_when_short() {
    let games = [g("1", true), g("2", true)];
    let out = pack(&games, Rect::new(0, 0, 120, 12), LayoutPref::Two, 0);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].area.width, 120);
    assert!(out[1].area.y > out[0].area.y);
}
