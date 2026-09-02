use gameday::config::{Favorite, Pin};
use gameday::domain::*;
use gameday::home::home_games;
use time::OffsetDateTime;

fn game(id: &str, abbr: &str, status: Status) -> Game {
    let t = |a: &str| Team {
        id: a.into(),
        abbr: a.into(),
        name: a.into(),
        color: [1, 2, 3],
        alt_color: [0, 0, 0],
        logo_key: format!("nfl/{}", a.to_lowercase()),
        ..Default::default()
    };
    Game {
        id: id.into(),
        league: League::Nfl,
        away: t(abbr),
        home: t("OPP"),
        away_score: 0,
        home_score: 0,
        status,
        period: "".into(),
        clock: "".into(),
        situation: None,
        last_plays: vec![],
        meter: None,
        ..Game::default()
    }
}

#[test]
fn favorite_pulls_live_team_onto_home() {
    // v3.2 §1: Home is ONE list of the day, so the pre-game is on it too —
    // what the favorite decides is the ORDER (its game leads).
    let boards = vec![game("9", "KC", Status::Live), game("8", "DAL", Status::Pre)];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&[], &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["9", "8"]);
}

#[test]
fn favorite_matches_home_or_away_case_insensitive() {
    let boards = vec![game("1", "kc", Status::Pre)];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&[], &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out[0].id, "1");
}

#[test]
fn pin_and_favorite_dedupe() {
    let boards = vec![game("9", "KC", Status::Live)];
    let pins = [Pin { game_id: "9".into(), league: League::Nfl, final_at: None }];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&pins, &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out.len(), 1);
}

#[test]
fn pin_order_then_favorites() {
    // The pin leads, then the favorite; "a" is a pre-game and lands after
    // both — v3.2 §1 keeps the whole day on Home, in band order.
    let boards = vec![
        game("a", "DAL", Status::Pre),
        game("b", "KC", Status::Live),
        game("c", "PHI", Status::Live),
    ];
    let pins = [Pin { game_id: "c".into(), league: League::Nfl, final_at: None }];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&pins, &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["c", "b", "a"]);
}

#[test]
fn favorites_follow_board_order_not_favorite_list_order() {
    // Fav list order is KC then DAL; board order is DAL then KC → expect DAL, KC.
    let boards = vec![
        game("dal", "DAL", Status::Live),
        game("kc", "KC", Status::Live),
    ];
    let favs = [
        Favorite { league: League::Nfl, team_abbr: "KC".into() },
        Favorite { league: League::Nfl, team_abbr: "DAL".into() },
    ];
    let out = home_games(&[], &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(
        out.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
        vec!["dal", "kc"]
    );
}

#[test]
fn home_shows_the_whole_day_live_first() {
    let boards = vec![
        game("1", "SEA", Status::Live),
        game("2", "NYY", Status::Pre),
        game("3", "SD", Status::Live),
        game("4", "SF", Status::Final),
    ];
    let out = home_games(&[], &[], &boards, OffsetDateTime::now_utc());
    let ids: Vec<&str> = out.iter().map(|g| g.id.as_str()).collect();
    // v3.2 §1: the board is one list with FINAL and LATER sections, so Home
    // carries every game today — live first, then the rest in board order.
    assert_eq!(
        ids,
        vec!["1", "3", "2", "4"],
        "live games in board order, then the day's other games"
    );
}

#[test]
fn pins_then_favorites_then_the_rest_of_the_live_slate() {
    let boards = vec![
        game("1", "SEA", Status::Live),
        game("2", "NYY", Status::Pre),
        game("3", "SD", Status::Live),
        game("4", "KC", Status::Live),
    ];
    let pins = [Pin { game_id: "3".into(), league: League::Nfl, final_at: None }];
    let favs = [Favorite { league: League::Nfl, team_abbr: "NYY".into() }];
    let out = home_games(&pins, &favs, &boards, OffsetDateTime::now_utc());
    let ids: Vec<&str> = out.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["3", "2", "1", "4"],
        "pin, then favorite (even pre-game), then live in board order"
    );
}
