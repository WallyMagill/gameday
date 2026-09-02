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
    // DAL is pre-game, so only the favorite's live game lands on Home here.
    let boards = vec![game("9", "KC", Status::Live), game("8", "DAL", Status::Pre)];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&[], &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, "9");
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
    // "a" is pre-game so the live band stays empty: this asserts the pin/fav
    // bands only.
    let boards = vec![
        game("a", "DAL", Status::Pre),
        game("b", "KC", Status::Live),
        game("c", "PHI", Status::Live),
    ];
    let pins = [Pin { game_id: "c".into(), league: League::Nfl, final_at: None }];
    let favs = [Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let out = home_games(&pins, &favs, &boards, OffsetDateTime::now_utc());
    assert_eq!(out.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["c", "b"]);
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
fn home_shows_every_live_game_when_nothing_is_pinned() {
    let boards = vec![
        game("1", "SEA", Status::Live),
        game("2", "NYY", Status::Pre),
        game("3", "SD", Status::Live),
        game("4", "SF", Status::Final),
    ];
    let out = home_games(&[], &[], &boards, OffsetDateTime::now_utc());
    let ids: Vec<&str> = out.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["1", "3"],
        "live games in board order; pre/final stay off Home unless pinned/favorited"
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
