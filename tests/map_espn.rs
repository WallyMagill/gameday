use gameday::domain::{League, Status};
use gameday::provider::map::{map_scoreboard, map_summary};

#[test]
fn maps_live_nfl_scoreboard() {
    let json = include_str!("../fixtures/nfl_scoreboard.json");
    let games = map_scoreboard(League::Nfl, json).unwrap();
    assert_eq!(games.len(), 1);
    let g = &games[0];
    assert_eq!(g.id, "401873001");
    assert_eq!(g.away.abbr, "KC");
    assert_eq!(g.home.abbr, "TB");
    assert_eq!(g.away_score, 27);
    assert_eq!(g.home_score, 24);
    assert_eq!(g.status, Status::Live);
    assert_eq!(g.clock, "1:27");
    assert_eq!(g.period, "Q4");
    let sit = g.situation.as_ref().unwrap();
    assert_eq!(sit.down_distance, "1st & Goal");
    assert_eq!(sit.possession.as_deref(), Some("KC"));
    assert_eq!(sit.ball_on.as_deref(), Some("TB 3"));
    assert_eq!(g.away.logo_key, "nfl/kc");
    assert_eq!(g.away.color, [0xe3, 0x18, 0x37]);
    assert_eq!(g.broadcast.as_deref(), Some("CBS"));
    assert_eq!(g.last_plays[0].text, "Mahomes pass to Kelce for 3 yards");
}

#[test]
fn maps_pre_and_final_states() {
    let pre = r#"{"events":[{"id":"1","competitions":[{"status":{"displayClock":"0:00","period":0,"type":{"state":"pre","completed":false}},"competitors":[
      {"homeAway":"away","score":"0","team":{"id":"1","abbreviation":"NE","displayName":"Patriots","color":"002244","alternateColor":"c60c30"}},
      {"homeAway":"home","score":"0","team":{"id":"2","abbreviation":"SEA","displayName":"Seahawks","color":"002244","alternateColor":"69be28"}}
    ],"broadcasts":[{"names":["NBC"]}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, pre).unwrap()[0];
    assert_eq!(g.status, Status::Pre);
    assert_eq!(g.broadcast.as_deref(), Some("NBC"));

    let post = r#"{"events":[{"id":"2","competitions":[{"status":{"displayClock":"0:00","period":4,"type":{"state":"post","completed":true}},"competitors":[
      {"homeAway":"away","score":"21","team":{"id":"1","abbreviation":"NE","displayName":"Patriots","color":"002244","alternateColor":"c60c30"}},
      {"homeAway":"home","score":"17","team":{"id":"2","abbreviation":"SEA","displayName":"Seahawks","color":"002244","alternateColor":"69be28"}}
    ]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, post).unwrap()[0];
    assert_eq!(g.status, Status::Final);
    assert_eq!(g.away_score, 21);
}

#[test]
fn maps_summary_scoring_plays_newest_first() {
    let json = include_str!("../fixtures/nfl_summary.json");
    let s = map_summary(json).unwrap();
    assert!(s.scoring_plays.iter().all(|p| p.scoring));
    assert_eq!(s.scoring_plays[0].text, "Tyler Bass 33 Yd Field Goal");
    assert_eq!(s.last_plays[0].text, "Kelce left for 2 yards");
    assert_eq!(s.last_plays[1].text, "Mahomes pass to Kelce for 3 yards");
}
