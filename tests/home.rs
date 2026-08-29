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
        start_time: None,
        broadcast: None,
    }
}

#[test]
fn favorite_pulls_live_team_onto_home() {
    let boards = vec![game("9", "KC", Status::Live), game("8", "DAL", Status::Live)];
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
    let boards = vec![
        game("a", "DAL", Status::Live),
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
