use gameday::domain::{League, Meter, Status};
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
    // Possessing team KC on the opponent's 3 => red zone.
    assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 3 }));
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

#[test]
fn maps_wnba_scoreboard_with_total_records() {
    let json = include_str!("../fixtures/wnba_scoreboard.json");
    let games = map_scoreboard(League::Wnba, json).unwrap();
    let g = games.iter().find(|g| g.id == "401857184").unwrap();
    assert_eq!(g.status, Status::Final);
    assert_eq!(g.home.abbr, "NY");
    assert_eq!(g.away.abbr, "CHI");
    assert_eq!(g.home_score, 85);
    assert_eq!(g.away_score, 66);
    // Records selected by type == "total", not positionally.
    assert_eq!(g.home.record, "24-16");
    assert_eq!(g.away.record, "15-25");
    assert_eq!(g.period, "Q4");
    // Meters are live-only; a final carries none.
    assert_eq!(g.meter, None);
}

#[test]
fn maps_live_mlb_inning_count_and_diamond() {
    let json = include_str!("../fixtures/mlb_scoreboard.json");
    let games = map_scoreboard(League::Mlb, json).unwrap();
    let g = games.iter().find(|g| g.id == "401816718").unwrap();
    assert_eq!(g.status, Status::Live);
    // ESPN's shortDetail ("Bot 7th") beats the bare period number, and the
    // meaningless "0:00" baseball clock is dropped.
    assert_eq!(g.period, "BOT 7TH");
    assert_eq!(g.clock, "");
    let sit = g.situation.as_ref().unwrap();
    assert_eq!(sit.balls, Some(4));
    assert_eq!(sit.strikes, Some(2));
    assert_eq!(sit.outs, Some(2));
    assert_eq!(sit.on_base, Some([true, false, true]));
    assert_eq!(sit.down_distance, "2 OUTS  4-2");
    assert_eq!(g.meter, Some(Meter::Diamond { occupied: [true, false, true] }));
    // Play attributed to the team on the payload (id 27 = COL), not possession.
    assert_eq!(g.last_plays[0].team, "COL");
    assert_eq!(g.home.record, "80-55");
    assert_eq!(g.away.record, "52-83");

    // Pre-game inning label stays empty (shortDetail is a date string there).
    let pre = games.iter().find(|g| g.id == "401816717").unwrap();
    assert_eq!(pre.status, Status::Pre);
    assert_eq!(pre.period, "");
}

#[test]
fn maps_epl_full_time_scoreboard() {
    let json = include_str!("../fixtures/epl_scoreboard.json");
    let games = map_scoreboard(League::Epl, json).unwrap();
    let g = games.iter().find(|g| g.id == "401879314").unwrap();
    assert_eq!(g.status, Status::Final);
    assert_eq!(g.home.abbr, "LIV");
    assert_eq!(g.away.abbr, "NFO");
    assert_eq!(g.home_score, 2);
    assert_eq!(g.away_score, 2);
    assert_eq!(g.period, "FT");
    assert_eq!(g.clock, "");
    // Soccer W-D-L record, selected by type == "total".
    assert_eq!(g.home.record, "0-2-0");
    assert_eq!(g.meter, None, "soccer has no meter");
}

fn one_game(state: &str, period: i64, clock: &str, away: u16, home: u16) -> String {
    format!(
        r#"{{"events":[{{"id":"1","competitions":[{{"status":{{"displayClock":"{clock}","period":{period},"type":{{"state":"{state}"}}}},"competitors":[
      {{"homeAway":"away","score":"{away}","team":{{"id":"1","abbreviation":"AAA","displayName":"Aaa"}}}},
      {{"homeAway":"home","score":"{home}","team":{{"id":"2","abbreviation":"HHH","displayName":"Hhh"}}}}
    ]}}]}}]}}"#
    )
}

#[test]
fn live_soccer_period_is_the_match_minute() {
    let json = one_game("in", 2, "90'+3'", 1, 1);
    let g = &map_scoreboard(League::Mls, &json).unwrap()[0];
    assert_eq!(g.period, "90'+3'");
    assert_eq!(g.clock, "");
}

#[test]
fn live_basketball_lead_meter_signs_per_side() {
    // Home up 80-74 => +6.
    let json = one_game("in", 3, "4:20", 74, 80);
    let g = &map_scoreboard(League::Wnba, &json).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::Lead { plus_minus: 6 }));
    // Away up 90-81 => -9.
    let json = one_game("in", 4, "1:00", 90, 81);
    let g = &map_scoreboard(League::Nba, &json).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::Lead { plus_minus: -9 }));
}

#[test]
fn period_labels_per_league_from_period_number() {
    for (league, period, want) in [
        (League::Cbb, 1, "1ST HALF"),
        (League::Cbb, 2, "2ND HALF"),
        (League::Cbb, 3, "OT"),
        (League::Nhl, 1, "1ST"),
        (League::Nhl, 3, "3RD"),
        (League::Nhl, 4, "OT"),
        (League::Nhl, 5, "SO"),
        (League::Cfb, 2, "Q2"),
        (League::Wnba, 5, "OT"),
    ] {
        let json = one_game("in", period, "5:00", 0, 0);
        let g = &map_scoreboard(league, &json).unwrap()[0];
        assert_eq!(g.period, want, "league {:?} period {period}", league);
    }
}

#[test]
fn record_prefers_type_total_over_first_entry() {
    let json = r#"{"events":[{"id":"1","competitions":[{"status":{"displayClock":"0:00","period":0,"type":{"state":"pre"}},"competitors":[
      {"homeAway":"away","score":"0","records":[{"type":"home","summary":"9-9"},{"type":"total","summary":"20-11"}],"team":{"id":"1","abbreviation":"AAA","displayName":"Aaa"}},
      {"homeAway":"home","score":"0","records":[{"type":"road","summary":"5-5"}],"team":{"id":"2","abbreviation":"HHH","displayName":"Hhh"}}
    ]}]}]}"#;
    let g = &map_scoreboard(League::Cbb, json).unwrap()[0];
    assert_eq!(g.away.record, "20-11", "type=total wins over first entry");
    assert_eq!(g.home.record, "5-5", "falls back to first when no total");
}

#[test]
fn maps_soccer_summary_key_events_with_header_team_abbrs() {
    // Live EPL/MLS summaries carry no drives/scoringPlays; goals, cards and
    // subs live under keyEvents, credited by team id (fixture trimmed from
    // the real 2026-08-29 NFO@LIV payload).
    let json = include_str!("../fixtures/epl_summary.json");
    let s = map_summary(json).unwrap();
    assert!(!s.last_plays.is_empty(), "keyEvents must map to plays");
    // Newest first: the last keyEvent (End Regular Time) leads.
    assert!(s.last_plays[0].text.starts_with("Second Half ends"), "{:?}", s.last_plays[0]);
    // Goals are flagged scoring and credited via the header id->abbr map.
    let goal = s
        .last_plays
        .iter()
        .find(|p| p.scoring)
        .expect("a goal within the last 8 events");
    assert_eq!(goal.team, "LIV");
    assert!(goal.clock.ends_with('\''), "match minute clock: {:?}", goal.clock);
    // Empty-text markers (Start Delay) are dropped, not mapped as blanks.
    assert!(s.last_plays.iter().all(|p| !p.text.is_empty()));
    // scoringPlays absent => derived from keyEvents, newest goal first.
    assert_eq!(s.scoring_plays.len(), 4);
    assert!(s.scoring_plays.iter().all(|p| p.scoring));
    assert_eq!(s.scoring_plays[0].clock, "82'");
    assert_eq!(s.scoring_plays[0].team, "LIV");
    assert_eq!(s.scoring_plays.last().unwrap().team, "NFO");
}

#[test]
fn maps_basketball_summary_flat_plays_array() {
    // Non-football, non-soccer summaries carry a flat `plays` array
    // (fixture: tail of the real 2026-08-29 CHI@NY WNBA payload).
    let json = include_str!("../fixtures/wnba_summary.json");
    let s = map_summary(json).unwrap();
    assert_eq!(s.last_plays.len(), 8, "capped at 8, like drives");
    // Newest first.
    assert_eq!(s.last_plays[0].text, "End of Game");
    // Team ids resolve to abbrs through the header (9 = NY, 19 = CHI).
    assert!(s.last_plays.iter().any(|p| p.team == "NY"), "{:?}", s.last_plays);
    assert!(s.last_plays.iter().any(|p| p.team == "CHI"), "{:?}", s.last_plays);
    let bucket = s.last_plays.iter().find(|p| p.scoring).expect("a made shot");
    assert_eq!(bucket.team, "NY");
    assert!(bucket.text.contains("makes free throw"));
}

#[test]
fn maps_nfl_boxscore_stats_and_leaders() {
    // Fixture: real 2026-08-23 SEA@TEN summary (event 401873297), trimmed to
    // boxscore.teams + leaders. Values asserted are from that capture.
    let json = include_str!("../fixtures/nfl_boxscore.json");
    let s = gameday::provider::map::map_stats(json).unwrap();
    assert!(s.rows.len() >= 5, "rows={}", s.rows.len());
    let ty = s
        .rows
        .iter()
        .find(|r| r.label == "Total Yards")
        .expect("a Total Yards row");
    assert_eq!(ty.away, "251", "SEA is the away team");
    assert_eq!(ty.home, "277", "TEN is the home team");
    // Every row carries both sides — no half-mapped rows.
    assert!(s.rows.iter().all(|r| !r.away.is_empty() && !r.home.is_empty()));
    let lock = s
        .leaders
        .iter()
        .find(|l| l.text.contains("D. Lock"))
        .expect("SEA passing leader");
    assert_eq!(lock.team, "SEA");
    assert_eq!(lock.label, "Passing Yards");
    assert!(lock.text.contains("12/14, 103 YDS, 1 TD"), "{}", lock.text);
    let ward = s
        .leaders
        .iter()
        .find(|l| l.text.contains("C. Ward"))
        .expect("TEN passing leader");
    assert_eq!(ward.team, "TEN");
}

#[test]
fn maps_nfl_standings_conferences_and_rows() {
    // Fixture: real 2026-08-30 NFL standings capture (preseason, 3 games in),
    // trimmed to children[].standings.entries[].{team,stats}. Values asserted
    // are from that capture.
    let json = include_str!("../fixtures/nfl_standings.json");
    let t = gameday::provider::map::map_standings(gameday::domain::League::Nfl, json).unwrap();
    assert_eq!(t.league, gameday::domain::League::Nfl);
    assert!(t.groups.len() >= 2, "groups={}", t.groups.len());
    assert_eq!(t.groups[0].name, "American Football Conference");
    assert_eq!(t.groups[1].name, "National Football Conference");
    for g in &t.groups {
        assert!(g.rows.len() >= 4, "group {:?} rows={}", g.name, g.rows.len());
    }
    let buf = t.groups[0]
        .rows
        .iter()
        .find(|r| r.abbr == "BUF")
        .expect("BUF in the AFC group");
    assert_eq!(buf.name, "Bills");
    assert_eq!((buf.wins, buf.losses), (3, 0));
    assert_eq!(buf.third, Some(0), "NFL carries ties");
    assert_eq!(buf.third_label, "T");
    let lar = t.groups[1]
        .rows
        .iter()
        .find(|r| r.abbr == "LAR")
        .expect("LAR in the NFC group");
    assert_eq!((lar.wins, lar.losses), (3, 0));
}

#[test]
fn maps_dated_scoreboard_odds() {
    // Fixture: real dated capture `?dates=20260913` (2026-08-30; the plan's
    // "yesterday" had only finals, which ESPN strips odds from — a future
    // slate is the honest capture that actually carries odds). Values
    // asserted are exact strings from that payload.
    let json = include_str!("../fixtures/nfl_scoreboard_dated.json");
    let games = map_scoreboard(League::Nfl, json).unwrap();
    assert_eq!(games.len(), 3);
    let cin = games
        .iter()
        .find(|g| g.away.abbr == "CIN" || g.home.abbr == "CIN")
        .expect("CIN game");
    assert_eq!(cin.odds.as_deref(), Some("CIN -3.5  O/U 51.5"));
    let det = games
        .iter()
        .find(|g| g.away.abbr == "DET" || g.home.abbr == "DET")
        .expect("DET game");
    assert_eq!(det.odds.as_deref(), Some("DET -7  O/U 49.5"));
    // The live scoreboard fixture carries no odds objects: mapped as None,
    // never an empty string.
    let live = map_scoreboard(League::Nfl, include_str!("../fixtures/nfl_scoreboard.json")).unwrap();
    assert!(!live.is_empty());
    assert!(live.iter().all(|g| g.odds.is_none()));
}
