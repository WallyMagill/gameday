use gameday::domain::{League, Meter, PlayKind, Status};
use gameday::provider::map::{map_scoreboard, map_standings, map_summary};
use time::UtcOffset;

/// Every mapper call in this file pins the same offset so `start` assertions
/// don't depend on the machine's timezone.
fn et() -> UtcOffset {
    UtcOffset::from_hms(-4, 0, 0).unwrap()
}

#[test]
fn maps_live_nfl_scoreboard() {
    let json = include_str!("../fixtures/nfl_scoreboard.json");
    let games = map_scoreboard(League::Nfl, json, et()).unwrap();
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
    // spec v3.4 §3: this hand-built v3.0-era fixture predates the structured
    // situation — it carries `possessionText: "TB 3"` and neither `isRedZone`
    // nor `yardLine`. The red zone used to be re-derived by splitting that
    // string; it isn't any more, so a payload that never says "red zone"
    // gets no meter. The real thing does say it — see
    // `the_red_zone_meter_reads_the_flag_and_the_yard_line` and
    // `live_situation_maps_integers_not_strings`.
    assert_eq!(g.meter, None);
    assert_eq!(sit.is_red_zone, None);
    assert_eq!(sit.yard_line, None);
}

#[test]
fn maps_pre_and_final_states() {
    let pre = r#"{"events":[{"id":"1","competitions":[{"status":{"displayClock":"0:00","period":0,"type":{"state":"pre","completed":false}},"competitors":[
      {"homeAway":"away","score":"0","team":{"id":"1","abbreviation":"NE","displayName":"Patriots","color":"002244","alternateColor":"c60c30"}},
      {"homeAway":"home","score":"0","team":{"id":"2","abbreviation":"SEA","displayName":"Seahawks","color":"002244","alternateColor":"69be28"}}
    ],"broadcasts":[{"names":["NBC"]}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, pre, et()).unwrap()[0];
    assert_eq!(g.status, Status::Pre);
    assert_eq!(g.broadcast.as_deref(), Some("NBC"));

    let post = r#"{"events":[{"id":"2","competitions":[{"status":{"displayClock":"0:00","period":4,"type":{"state":"post","completed":true}},"competitors":[
      {"homeAway":"away","score":"21","team":{"id":"1","abbreviation":"NE","displayName":"Patriots","color":"002244","alternateColor":"c60c30"}},
      {"homeAway":"home","score":"17","team":{"id":"2","abbreviation":"SEA","displayName":"Seahawks","color":"002244","alternateColor":"69be28"}}
    ]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, post, et()).unwrap()[0];
    assert_eq!(g.status, Status::Final);
    assert_eq!(g.away_score, 21);
}

#[test]
fn maps_summary_scoring_plays_newest_first() {
    let json = include_str!("../fixtures/nfl_summary.json");
    let s = map_summary(League::Nfl, json).unwrap();
    assert!(s.scoring_plays.iter().all(|p| p.scoring));
    assert_eq!(s.scoring_plays[0].text, "Tyler Bass 33 Yd Field Goal");
    assert_eq!(s.last_plays[0].text, "Kelce left for 2 yards");
    assert_eq!(s.last_plays[1].text, "Mahomes pass to Kelce for 3 yards");
}

#[test]
fn maps_wnba_scoreboard_with_total_records() {
    let json = include_str!("../fixtures/wnba_scoreboard.json");
    let games = map_scoreboard(League::Wnba, json, et()).unwrap();
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
    // The fixture's final carries ESPN's leftover displayClock "10:00"; a
    // finished game has nothing on the clock.
    assert_eq!(g.clock, "");
    // Meters are live-only; a final carries none.
    assert_eq!(g.meter, None);
}

#[test]
fn maps_live_mlb_inning_count_and_diamond() {
    let json = include_str!("../fixtures/mlb_scoreboard.json");
    let games = map_scoreboard(League::Mlb, json, et()).unwrap();
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
    assert_eq!(sit.down_distance, "2 OUT · 4-2");
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
    let games = map_scoreboard(League::Epl, json, et()).unwrap();
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
    let g = &map_scoreboard(League::Mls, &json, et()).unwrap()[0];
    assert_eq!(g.period, "90'+3'");
    assert_eq!(g.clock, "");
}

#[test]
fn live_basketball_lead_meter_signs_per_side() {
    // Home up 80-74 => +6.
    let json = one_game("in", 3, "4:20", 74, 80);
    let g = &map_scoreboard(League::Wnba, &json, et()).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::Lead { plus_minus: 6 }));
    // Away up 90-81 => -9.
    let json = one_game("in", 4, "1:00", 90, 81);
    let g = &map_scoreboard(League::Nba, &json, et()).unwrap()[0];
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
        let g = &map_scoreboard(league, &json, et()).unwrap()[0];
        assert_eq!(g.period, want, "league {:?} period {period}", league);
    }
}

#[test]
fn record_prefers_type_total_over_first_entry() {
    let json = r#"{"events":[{"id":"1","competitions":[{"status":{"displayClock":"0:00","period":0,"type":{"state":"pre"}},"competitors":[
      {"homeAway":"away","score":"0","records":[{"type":"home","summary":"9-9"},{"type":"total","summary":"20-11"}],"team":{"id":"1","abbreviation":"AAA","displayName":"Aaa"}},
      {"homeAway":"home","score":"0","records":[{"type":"road","summary":"5-5"}],"team":{"id":"2","abbreviation":"HHH","displayName":"Hhh"}}
    ]}]}]}"#;
    let g = &map_scoreboard(League::Cbb, json, et()).unwrap()[0];
    assert_eq!(g.away.record, "20-11", "type=total wins over first entry");
    assert_eq!(g.home.record, "5-5", "falls back to first when no total");
}

#[test]
fn maps_soccer_summary_key_events_with_header_team_abbrs() {
    // Live EPL/MLS summaries carry no drives/scoringPlays; goals, cards and
    // subs live under keyEvents, credited by team id (fixture trimmed from
    // the real 2026-08-29 NFO@LIV payload).
    let json = include_str!("../fixtures/epl_summary.json");
    let s = map_summary(League::Epl, json).unwrap();
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
fn nfl_summary_plays_carry_kinds() {
    // Real NFL summary (drives.previous path): scoring plays resolve through
    // football_kind via scoringType.name; a non-scoring play stays Other.
    let json = include_str!("../fixtures/nfl_summary_full.json");
    let s = map_summary(League::Nfl, json).unwrap();
    let scoring: Vec<_> = s.last_plays.iter().filter(|p| p.scoring).collect();
    assert!(
        scoring.iter().any(|p| p.kind == PlayKind::Touchdown),
        "expected a Touchdown among scoring plays"
    );
    assert!(
        scoring.iter().any(|p| p.kind == PlayKind::FieldGoal),
        "expected a FieldGoal among scoring plays"
    );
    assert!(
        s.last_plays.iter().any(|p| !p.scoring && p.kind == PlayKind::Other),
        "a non-scoring play stays Other"
    );
}

#[test]
fn nhl_goals_and_penalties_are_kinds() {
    // Untruncated live capture: the committed nhl_summary_full.json fixture
    // has no 505 Goal row in its trimmed plays, so this reaches into the
    // full live capture (has both a goal and a penalty).
    let json = include_str!("../fixtures/live/nhl_summary_final_full.json");
    let s = map_summary(League::Nhl, json).unwrap();
    assert!(
        s.last_plays.iter().any(|p| p.kind == PlayKind::Goal),
        "expected a 505 Goal play"
    );
    assert!(
        s.last_plays.iter().any(|p| p.kind == PlayKind::HockeyPenalty),
        "expected a play carrying type.penaltyMinutes"
    );
}

#[test]
fn cbb_uses_its_own_table() {
    // CBB's three-pointer id (558 JumpShot) is not the NBA id (92) — the
    // mapper must pass cbb: true so the play routes through the CBB table.
    let json = include_str!("../fixtures/cbb_summary_full.json");
    let s = map_summary(League::Cbb, json).unwrap();
    assert!(
        s.last_plays.iter().any(|p| p.kind == PlayKind::ThreePointer),
        "expected a made three (558/scoringPlay/scoreValue 3) to map ThreePointer"
    );
    // v3.4 T3 review: fixtures/cbb_summary_full.json carries 7 missed
    // threes (id 558, scoringPlay: false) that CBB's endpoint still stamps
    // scoreValue: 3 on — the gate must be scoringPlay, not shootingPlay, or
    // every miss maps ThreePointer.
    assert!(
        s.last_plays
            .iter()
            .any(|p| !p.scoring && p.score_value == Some(3) && p.kind == PlayKind::Other),
        "expected a missed three (558/scoreValue 3/scoringPlay false) to stay Other"
    );
}

#[test]
fn soccer_summary_events_come_from_type_ids() {
    // epl_summary_full's keyEvents carry null scoringPlay/ownGoal/yellowCard
    // booleans (verified via probe) — only type.id is a trustworthy signal.
    let json = include_str!("../fixtures/epl_summary_full.json");
    let s = map_summary(League::Epl, json).unwrap();
    assert!(
        s.last_plays.iter().any(|p| p.kind == PlayKind::Goal),
        "expected a type.id 70 Goal event"
    );
    assert!(
        s.last_plays.iter().any(|p| p.kind == PlayKind::YellowCard),
        "expected a type.id 94 Yellow Card event"
    );
}

#[test]
fn maps_basketball_summary_flat_plays_array() {
    // Non-football, non-soccer summaries carry a flat `plays` array
    // (fixture: tail of the real 2026-08-29 CHI@NY WNBA payload).
    let json = include_str!("../fixtures/wnba_summary.json");
    let s = map_summary(League::Wnba, json).unwrap();
    // Every play in the fixture, uncapped — the display cap is the tile's job.
    assert_eq!(s.last_plays.len(), 10, "the whole fixture list, not a cap");
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
    let games = map_scoreboard(League::Nfl, json, et()).unwrap();
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
    let live = map_scoreboard(League::Nfl, include_str!("../fixtures/nfl_scoreboard.json"), et()).unwrap();
    assert!(!live.is_empty());
    assert!(live.iter().all(|g| g.odds.is_none()));
}

#[test]
fn a_malformed_event_is_skipped_not_fatal() {
    // Second event has no competitors (ESPN ships placeholder rows in preseason).
    let json = r#"{"events":[
      {"id":"1","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}},"competitors":[
        {"homeAway":"away","score":"0","team":{"id":"1","abbreviation":"NE"}},
        {"homeAway":"home","score":"0","team":{"id":"2","abbreviation":"SEA"}}]}]},
      {"id":"2","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}}}]}
    ]}"#;
    let games = map_scoreboard(League::Nfl, json, et()).unwrap();
    assert_eq!(games.len(), 1, "the good event survives the bad one");
    assert_eq!(games[0].id, "1");
}

#[test]
fn all_events_unmappable_is_an_error_not_an_empty_board() {
    // Schema drift, not a quiet day: every event fails. The provider caches a
    // body only after it maps, so Ok(empty) here would evict last-good cache.
    let json = r#"{"events":[
      {"id":"1","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}}}]},
      {"id":"2","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}}}]}
    ]}"#;
    assert!(map_scoreboard(League::Nfl, json, et()).is_err());
    // A genuinely empty slate is still a good map.
    let empty = map_scoreboard(League::Nfl, r#"{"events":[]}"#, et()).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn start_is_local_and_never_a_raw_string() {
    let games =
        map_scoreboard(League::Wnba, include_str!("../fixtures/wnba_scoreboard.json"), et())
            .unwrap();
    let pre = games
        .iter()
        .find(|g| g.status == Status::Pre)
        .expect("fixture has a pre game");
    let start = pre.start.expect("start parsed");
    assert_eq!(start.offset(), et());
}

#[test]
fn mlb_maps_linescore_hits_errors_matchup_and_play_period() {
    let games =
        map_scoreboard(League::Mlb, include_str!("../fixtures/mlb_scoreboard.json"), et()).unwrap();
    let g = games.iter().find(|g| g.id == "401816718").unwrap();
    assert!(g.linescore.len() >= 7, "per-inning linescore, got {:?}", g.linescore);
    match &g.extras {
        gameday::domain::Extras::Baseball { hits, errors } => {
            assert!(
                hits.is_some() && errors.is_some(),
                "hits/errors are on every MLB competitor"
            );
        }
        other => panic!("expected Baseball extras, got {other:?}"),
    }
    let sit = g.situation.as_ref().unwrap();
    assert!(
        sit.pitcher.is_some() && sit.batter.is_some(),
        "situation.pitcher/batter present live"
    );
    // The scoreboard lastPlay is a pitch ("Pitch 6 : Ball 3"); the tile wants
    // the human label plus the batter, and the inning where the clock would be.
    let p = &g.last_plays[0];
    assert_eq!(p.text, "Walk — A. Riley");
    assert_eq!(p.period, "B7");
    assert_eq!(p.clock, "");
}

#[test]
fn soccer_details_become_match_events() {
    let games =
        map_scoreboard(League::Epl, include_str!("../fixtures/epl_scoreboard.json"), et()).unwrap();
    let g = games.iter().find(|g| g.id == "401879314").unwrap();
    let gameday::domain::Extras::Soccer { events } = &g.extras else {
        panic!("soccer extras")
    };
    assert!(!events.is_empty());
    let goal = events
        .iter()
        .find(|e| e.kind == gameday::domain::EventKind::Goal)
        .unwrap();
    assert!(goal.minute.ends_with('\''), "{}", goal.minute);
    assert!(!goal.player.is_empty());
}

#[test]
fn cfb_rank_comes_from_curated_rank() {
    let json = r#"{"events":[{"id":"1","date":"2026-08-29T23:30Z","competitions":[{"status":{"type":{"state":"post"},"period":4},"competitors":[
      {"homeAway":"away","score":"26","curatedRank":{"current":99},"team":{"id":"1","abbreviation":"SJSU"}},
      {"homeAway":"home","score":"42","curatedRank":{"current":14},"team":{"id":"2","abbreviation":"USC"}}]}]}]}"#;
    let g = &map_scoreboard(League::Cfb, json, et()).unwrap()[0];
    assert_eq!(g.home.rank, Some(14));
    assert_eq!(g.away.rank, None, "ESPN uses 99 for unranked");
}

#[test]
fn football_timeouts_map_from_situation() {
    let json = r#"{"events":[{"id":"1","date":"2026-09-13T17:00Z","competitions":[{"status":{"type":{"state":"in"},"period":4,"displayClock":"1:27"},
      "situation":{"awayTimeouts":1,"homeTimeouts":3,"downDistanceText":"1st & Goal","possessionText":"TB 3","possession":"1"},
      "competitors":[{"homeAway":"away","score":"27","team":{"id":"1","abbreviation":"KC"}},{"homeAway":"home","score":"24","team":{"id":"2","abbreviation":"TB"}}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, json, et()).unwrap()[0];
    assert_eq!(g.timeouts, Some((1, 3)));
}

use gameday::provider::map::map_stats;

#[test]
fn mlb_summary_keeps_only_at_bat_results_and_scoring_and_tags_the_inning() {
    let s = map_summary(League::Mlb, include_str!("../fixtures/mlb_summary_min.json")).unwrap();
    let texts: Vec<&str> = s.last_plays.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "Rodríguez singles to right, Crawford to third",
            "Raleigh struck out swinging.",
            "Devers homered to right (24)",
        ],
        "pitches (P) and inning markers (I) are dropped; newest first"
    );
    assert_eq!(s.last_plays[0].period, "T9");
    assert_eq!(s.last_plays[2].period, "B3");
    assert_eq!(s.scoring_plays.len(), 1);
    assert_eq!(s.scoring_plays[0].team, "BOS");
}

#[test]
fn the_home_run_kind_rides_the_narrative_play() {
    // fixtures/live/mlb_summary_live_full.json: the type 28 (Home Run) pitch
    // row for atBatId 4018167930503 carries the kind, but sits on its own row
    // — the batter's outcome text lives on a separate narrative row (type 57
    // Play Result), joined only by atBatId (verified: `jq '.plays[] |
    // select(.type.id=="28")'` and `select(.atBatId=="4018167930503")`).
    let json = include_str!("../fixtures/live/mlb_summary_live_full.json");
    let s = map_summary(League::Mlb, json).unwrap();
    let homer = s
        .last_plays
        .iter()
        .find(|p| p.text == "Crow-Armstrong homered to center (413 feet), Kelly scored.")
        .expect("the narrative row for the home-run at-bat is still mapped");
    assert_eq!(homer.kind, PlayKind::HomeRun, "the pitch row's kind (28) rides the narrative row");
    // P rows (227 of them) stay filtered out; the mapped play count is
    // unchanged from today's mapping — pinned via jq over the fixture: rows
    // with non-empty text whose summaryType isn't P/I/A/C.
    assert_eq!(s.last_plays.len(), 79);
}

#[test]
fn a_scoring_non_homer_is_run_scoring_play() {
    // The live fixture's only two scoring at-bats are both home runs
    // (verified: `jq '[.plays[] | select(.scoringPlay==true)]'` → 2 rows,
    // both Home Run) — no sac-fly/RBI-single at-bat exists there to pin, so
    // this exercises the join mechanism itself on a minimal synthetic
    // at-bat: a sacrifice-fly pitch row (type 35, ESPN's real id — see
    // kinds.rs) joined by atBatId to its scoring narrative sibling.
    let json = r#"{"header":{"competitions":[{"competitors":[{"team":{"id":"1","abbreviation":"BOS"}}]}]},
      "plays":[
        {"summaryType":"P","atBatId":"1","type":{"id":"35"},"scoreValue":0,"text":"Pitch 1 : Ball In Play","team":{"id":"1"},"period":{"type":"Bottom","number":3}},
        {"summaryType":"S","atBatId":"1","type":{"id":"57"},"scoreValue":1,"scoringPlay":true,"text":"Devers sacrifice fly to center, Duran scores.","team":{"id":"1"},"period":{"type":"Bottom","number":3}}
      ]}"#;
    let s = map_summary(League::Mlb, json).unwrap();
    assert_eq!(s.last_plays.len(), 1, "the sac-fly pitch row is filtered, the narrative row stays");
    assert_eq!(s.last_plays[0].text, "Devers sacrifice fly to center, Duran scores.");
    assert_eq!(s.last_plays[0].kind, PlayKind::RunScoringPlay, "score_value > 0, joined pitch id isn't 28");
}

#[test]
fn summary_is_not_truncated_to_eight() {
    // 20 flat plays with the scoring play at index 3 — the old split_off(len-8)
    // dropped it and every scoring surface went blank (review finding #2).
    let mut plays = String::new();
    for i in 0..20 {
        if i > 0 {
            plays.push(',');
        }
        let scoring = i == 3;
        plays.push_str(&format!(
            r#"{{"text":"play {i}","scoringPlay":{scoring},"team":{{"id":"1"}},"clock":{{"displayValue":"{}:00"}}}}"#,
            12 - (i % 12)
        ));
    }
    let json = format!(
        r#"{{"header":{{"competitions":[{{"competitors":[{{"team":{{"id":"1","abbreviation":"DEN"}}}}]}}]}},"plays":[{plays}]}}"#
    );
    let s = map_summary(League::Nfl, &json).unwrap();
    assert_eq!(s.last_plays.len(), 20);
    assert_eq!(s.scoring_plays.len(), 1);
    assert_eq!(s.scoring_plays[0].text, "play 3");
    assert!(s.last_plays.iter().any(|p| p.scoring && p.text == "play 3"));
}

#[test]
fn a_competitor_with_an_unknown_home_away_skips_the_event() {
    // Schema drift, not an away team: writing a third side into `away` would
    // render a tile that claims two home teams played each other.
    let drift = r#"{"events":[{"id":"9","competitions":[{"status":{"displayClock":"0:00","period":0,"type":{"state":"pre","completed":false}},"competitors":[
      {"homeAway":"home","score":"0","team":{"id":"1","abbreviation":"NE","displayName":"Patriots","color":"002244","alternateColor":"c60c30"}},
      {"homeAway":"neutral","score":"0","team":{"id":"2","abbreviation":"SEA","displayName":"Seahawks","color":"002244","alternateColor":"69be28"}}]}]}]}"#;
    let ev: serde_json::Value = serde_json::from_str(drift).unwrap();
    let err = gameday::provider::map::map_event(League::Nfl, &ev["events"][0], et()).unwrap_err();
    assert!(
        format!("{err}").contains("homeAway"),
        "the skip names the field that drifted: {err}"
    );
    // The whole slate is that one event, so nothing maps.
    assert!(map_scoreboard(League::Nfl, drift, et()).is_err());
    // The same payload with a real away side maps fine.
    let ok = drift.replace("\"neutral\"", "\"away\"");
    assert_eq!(map_scoreboard(League::Nfl, &ok, et()).unwrap().len(), 1);
}

#[test]
fn sports_with_no_extra_source_map_to_extras_none() {
    // Football drive text and NHL shots have no scoreboard source; they carry
    // no half-built variant until sub-project 3 gives them one.
    let json = include_str!("../fixtures/nfl_scoreboard.json");
    let g = &map_scoreboard(League::Nfl, json, et()).unwrap()[0];
    assert_eq!(g.extras, gameday::domain::Extras::None);
}

#[test]
fn grouped_box_score_maps_and_missing_leaders_is_empty_not_error() {
    let stats = map_stats(include_str!("../fixtures/mlb_summary_min.json")).unwrap();
    let hits = stats
        .rows
        .iter()
        .find(|r| r.label == "H")
        .expect("grouped stats flattened");
    assert_eq!((hits.away.as_str(), hits.home.as_str()), ("11", "9"));
    assert!(stats.leaders.is_empty());
}

#[test]
fn standings_rows_are_sorted_by_win_pct_then_wins_then_name() {
    let t = map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json")).unwrap();
    for g in &t.groups {
        let pct = |r: &gameday::domain::StandingRow| {
            let gp = r.wins + r.losses + r.third.unwrap_or(0);
            if gp == 0 {
                0.0
            } else {
                (r.wins as f64 + 0.5 * r.third.unwrap_or(0) as f64) / gp as f64
            }
        };
        for w in g.rows.windows(2) {
            assert!(
                pct(&w[0]) >= pct(&w[1]) - 1e-9,
                "{} before {} in {}",
                w[0].abbr,
                w[1].abbr,
                g.name
            );
        }
    }
}

#[test]
fn standings_label_is_the_season_when_present_else_none() {
    let t = map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json")).unwrap();
    assert_eq!(t.season, None, "this fixture carries no season key");
    let json = r#"{"name":"X","season":{"displayName":"2025-26"},"children":[{"name":"East","standings":{"entries":[{"team":{"abbreviation":"BOS","name":"Celtics"},"stats":[{"type":"wins","value":58},{"type":"losses","value":24}]}]}}]}"#;
    let t = map_standings(League::Nba, json).unwrap();
    assert_eq!(t.season.as_deref(), Some("2025-26"));
}

#[test]
fn ties_column_only_when_the_sport_has_one() {
    let json = r#"{"name":"MLB","children":[{"name":"AL","standings":{"entries":[{"team":{"abbreviation":"TB","name":"Rays"},"stats":[{"type":"wins","value":82},{"type":"losses","value":55},{"type":"ties","value":0}]}]}}]}"#;
    let t = map_standings(League::Mlb, json).unwrap();
    assert_eq!(
        t.groups[0].rows[0].third, None,
        "MLB sends ties=0 for every team; drop the column"
    );
}

/// The live FBS payload's two awkward shapes, both checked 2026-08-31 against
/// `…/college-football/standings?group=80`: the Sun Belt nests its divisions
/// as grandchildren, and a stat worth zero is simply absent — a 1-0 team
/// carries `wins` and no `losses`. Requiring both dropped every undefeated
/// team, which in preseason is the entire table.
#[test]
fn cfb_divisions_become_their_own_groups_and_an_absent_zero_stat_is_zero() {
    let json = r#"{"id":"80","name":"FBS","season":{"displayName":"2026"},"children":[
        {"name":"Big Ten Conference","standings":{"entries":[
            {"team":{"abbreviation":"OSU","name":"Buckeyes"},"stats":[{"type":"wins","value":1}]},
            {"team":{"abbreviation":"MICH","name":"Wolverines"},"stats":[{"type":"losses","value":1}]}]}},
        {"name":"Sun Belt Conference","children":[
            {"name":"Sun Belt - East","standings":{"entries":[
                {"team":{"abbreviation":"APP","name":"Mountaineers"},"stats":[{"type":"wins","value":1}]}]}},
            {"name":"Sun Belt - West","standings":{"entries":[
                {"team":{"abbreviation":"TROY","name":"Trojans"},"stats":[{"type":"losses","value":1}]}]}}]}]}"#;
    let t = map_standings(League::Cfb, json).unwrap();
    assert_eq!(t.season.as_deref(), Some("2026"));
    let names: Vec<&str> = t.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        ["Big Ten Conference", "Sun Belt Conference · Sun Belt - East", "Sun Belt Conference · Sun Belt - West"]
    );
    let osu = &t.groups[0].rows[0];
    assert_eq!((osu.abbr.as_str(), osu.wins, osu.losses), ("OSU", 1, 0), "1-0 sorts first");
    let mich = &t.groups[0].rows[1];
    assert_eq!((mich.abbr.as_str(), mich.wins, mich.losses), ("MICH", 0, 1));
    assert_eq!(osu.third, None, "no ties stat, no ties column");
}

/// The zero-stat rule has a floor: one of wins/losses is the zero the feed
/// didn't send, but *neither* is not a 0-0 team — it's an entry we can't read,
/// and inventing an 0-0 record for it would put a phantom in the table.
#[test]
fn an_entry_with_neither_wins_nor_losses_is_dropped_not_read_as_0_0() {
    let json = r#"{"name":"FBS","children":[{"name":"Big Ten Conference","standings":{"entries":[
        {"team":{"abbreviation":"OSU","name":"Buckeyes"},"stats":[{"type":"wins","value":3}]},
        {"team":{"abbreviation":"PUR","name":"Boilermakers"},"stats":[{"type":"losses","value":2}]},
        {"team":{"abbreviation":"GHOST","name":"Nobody"},"stats":[{"type":"playoffseed","value":7},{"type":"streak","value":1}]}]}}]}"#;
    let t = map_standings(League::Cfb, json).unwrap();
    let rows = &t.groups[0].rows;
    let abbrs: Vec<&str> = rows.iter().map(|r| r.abbr.as_str()).collect();
    assert_eq!(abbrs, ["OSU", "PUR"], "an entry with no W and no L is not a row");
    assert_eq!((rows[0].wins, rows[0].losses), (3, 0), "wins only maps as W-0");
    assert_eq!((rows[1].wins, rows[1].losses), (0, 2), "losses only maps as 0-L");
}

/// The truth test: real, untrimmed scoreboards and summaries from all nine
/// leagues, captured once by `scripts/capture-fixtures.sh` and frozen here.
/// Hand-written fixtures only prove the mapper reads what we imagined; these
/// prove it reads what ESPN actually sends. Every event that carries two
/// competitors must map — a skip means a real game would vanish from the
/// board. (Events with fewer than two competitors are ESPN's own placeholder
/// rows; the per-event skip exists precisely for them.)
#[test]
fn every_league_maps_its_full_scoreboard_and_summary_with_no_skips() {
    let cases: [(League, &str, &str); 9] = [
        (
            League::Nfl,
            include_str!("../fixtures/nfl_scoreboard_full.json"),
            include_str!("../fixtures/nfl_summary_full.json"),
        ),
        (
            League::Cfb,
            include_str!("../fixtures/cfb_scoreboard_full.json"),
            include_str!("../fixtures/cfb_summary_full.json"),
        ),
        (
            League::Cbb,
            include_str!("../fixtures/cbb_scoreboard_full.json"),
            include_str!("../fixtures/cbb_summary_full.json"),
        ),
        (
            League::Nba,
            include_str!("../fixtures/nba_scoreboard_full.json"),
            include_str!("../fixtures/nba_summary_full.json"),
        ),
        (
            League::Wnba,
            include_str!("../fixtures/wnba_scoreboard_full.json"),
            include_str!("../fixtures/wnba_summary_full.json"),
        ),
        (
            League::Nhl,
            include_str!("../fixtures/nhl_scoreboard_full.json"),
            include_str!("../fixtures/nhl_summary_full.json"),
        ),
        (
            League::Mlb,
            include_str!("../fixtures/mlb_scoreboard_full.json"),
            include_str!("../fixtures/mlb_summary_full.json"),
        ),
        (
            League::Epl,
            include_str!("../fixtures/epl_scoreboard_full.json"),
            include_str!("../fixtures/epl_summary_full.json"),
        ),
        (
            League::Mls,
            include_str!("../fixtures/mls_scoreboard_full.json"),
            include_str!("../fixtures/mls_summary_full.json"),
        ),
    ];
    for (league, sb, sm) in cases {
        let raw: serde_json::Value = serde_json::from_str(sb).unwrap();
        let mappable = raw["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                e["competitions"][0]["competitors"]
                    .as_array()
                    .is_some_and(|c| c.len() >= 2)
            })
            .count();
        let games = map_scoreboard(league, sb, et()).unwrap();
        assert_eq!(
            games.len(),
            mappable,
            "{}: every event with two competitors maps (none skipped)",
            league.slug()
        );
        for g in &games {
            assert!(g.start.is_some(), "{}: {} has no start", league.slug(), g.id);
            if g.status != Status::Pre {
                assert!(
                    !g.linescore.is_empty() || matches!(league, League::Epl | League::Mls),
                    "{}: {} linescore",
                    league.slug(),
                    g.id
                );
            }
        }
        let s = map_summary(league, sm).unwrap();
        assert!(!s.last_plays.is_empty(), "{}: summary plays", league.slug());
        if league == League::Mlb {
            assert!(
                s.last_plays.iter().all(|p| !p.text.starts_with("Pitch ")),
                "MLB pitch rows leaked"
            );
        }
        if !s.scoring_plays.is_empty() {
            assert!(
                s.last_plays.iter().any(|p| p.scoring),
                "{}: scoring flag lost",
                league.slug()
            );
        }
        let stats = map_stats(sm).unwrap();
        assert!(
            !stats.rows.is_empty(),
            "{}: box score rows (grouped or flat)",
            league.slug()
        );
    }
}

/// Spec §1: no committed fixture carried live game state, so no test could
/// catch a live-situation regression. This is that regression net — for
/// every `fixtures/live/*_scoreboard_live.json`, find the event ESPN itself
/// marked live (`status.type.state == "in"`) with a `situation` object, map
/// it, and assert the mapped `Game` actually carries a non-empty situation
/// in the shape that league's mapper is supposed to fill in.
#[test]
fn live_fixtures_carry_live_state() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/live");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let Some(slug) = name.strip_suffix("_scoreboard_live.json") else { continue };
        let league = League::from_slug(slug).unwrap_or_else(|| panic!("unknown league slug in fixture name: {slug}"));
        let json = std::fs::read_to_string(&path).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&json).unwrap();
        let events = raw["events"].as_array().unwrap();
        let live_id = events
            .iter()
            .find(|ev| {
                ev["status"]["type"]["state"].as_str() == Some("in")
                    && ev["competitions"][0]["situation"].is_object()
            })
            .and_then(|ev| ev["id"].as_str())
            .unwrap_or_else(|| panic!("{name}: no live event with a situation object"));

        let games = map_scoreboard(league, &json, et()).unwrap();
        let g = games.iter().find(|g| g.id == live_id).unwrap_or_else(|| panic!("{name}: live event {live_id} did not map"));
        assert_eq!(g.status, Status::Live, "{name}: {live_id} should map to Status::Live");
        let sit = g.situation.as_ref().unwrap_or_else(|| panic!("{name}: {live_id} mapped with no situation at all"));

        match league {
            League::Cfb | League::Nfl => {
                assert!(!sit.down_distance.is_empty(), "{name}: {live_id} down/distance empty");
                assert!(sit.possession.is_some(), "{name}: {live_id} possession missing");
                // spec v3.4 §3: the structured fields, not the strings.
                assert!(sit.down.is_some(), "{name}: {live_id} down missing");
                assert!(sit.distance.is_some(), "{name}: {live_id} distance missing");
                assert!(sit.yard_line.is_some(), "{name}: {live_id} yardLine missing");
                assert!(sit.is_red_zone.is_some(), "{name}: {live_id} isRedZone missing");
            }
            League::Mlb => {
                assert!(
                    sit.balls.is_some() || sit.strikes.is_some() || sit.outs.is_some(),
                    "{name}: {live_id} carries no count fields"
                );
                assert!(sit.on_base.is_some(), "{name}: {live_id} carries no base state");
            }
            League::Epl | League::Mls => {
                let gameday::domain::Extras::Soccer { events } = &g.extras else {
                    panic!("{name}: {live_id} soccer game without Soccer extras");
                };
                assert!(!events.is_empty(), "{name}: {live_id} carries no match events");
            }
            _ => {
                assert!(!sit.down_distance.is_empty(), "{name}: {live_id} situation summary empty");
            }
        }
        checked += 1;
    }
    assert!(checked > 0, "no fixtures/live/*_scoreboard_live.json found");
}

/// v3.1-era capture bug (fixed in `scripts/capture-fixtures.sh`, spec §1):
/// summaries were piped through a filter that capped `plays[]` at exactly
/// 80, silently truncating live games that carry 300-540. This fixture is a
/// real live-game capture and must stay untruncated.
#[test]
fn the_mlb_live_summary_is_untruncated() {
    let json = include_str!("../fixtures/live/mlb_summary_live_full.json");
    let raw: serde_json::Value = serde_json::from_str(json).unwrap();
    let plays = raw["plays"].as_array().unwrap();
    assert!(plays.len() > 100, "expected > 100 plays, fixture has {}", plays.len());
    assert!(
        plays.iter().any(|p| p["summaryType"].as_str() == Some("P")),
        "expected at least one pitch (summaryType == \"P\") row"
    );
}

/// Spec v3.4 §3: the live scoreboard carries `situation.down`, `.distance`,
/// `.yardLine` and `.isRedZone` as real JSON numbers and booleans — the
/// mapper reads those, not `downDistanceText`/`possessionText`. Values
/// pinned from the real capture (`fixtures/live/cfb_scoreboard_live.json`,
/// event 401856663: `"down":3,"distance":10,"yardLine":75,"isRedZone":false`).
#[test]
fn live_situation_maps_integers_not_strings() {
    let json = include_str!("../fixtures/live/cfb_scoreboard_live.json");
    let games = map_scoreboard(League::Cfb, json, et()).unwrap();
    let g = games.iter().find(|g| g.id == "401856663").unwrap();
    let sit = g.situation.as_ref().unwrap();
    assert_eq!(sit.down, Some(3));
    assert_eq!(sit.distance, Some(10));
    // Absolute field coordinate from the HOME goal line: UAPB (away) has the
    // ball on its own 25, which is 75 yards from MIZ's goal.
    assert_eq!(sit.yard_line, Some(75));
    assert_eq!(sit.is_red_zone, Some(false));
    // The drive line rides on `situation.lastPlay.drive.description`.
    assert_eq!(sit.drive_desc.as_deref(), Some("1 play, 0 yards, 0:06"));
    // ESPN says this is not the red zone, so there is no red-zone meter — no
    // text parse gets a second opinion.
    assert_eq!(g.meter, None);

    // The other live CFB event pins the coordinate again: IDHO (away) on its
    // own 41 => yardLine 59.
    let g2 = games.iter().find(|g| g.id == "401856768").unwrap();
    let sit2 = g2.situation.as_ref().unwrap();
    assert_eq!(
        (sit2.down, sit2.distance, sit2.yard_line),
        (Some(2), Some(5), Some(59))
    );
    assert_eq!(sit2.is_red_zone, Some(false));

    // A non-football live event: the football fields stay None rather than
    // being invented from the count.
    let mlb = include_str!("../fixtures/live/mlb_scoreboard_live.json");
    let games = map_scoreboard(League::Mlb, mlb, et()).unwrap();
    let g = games.iter().find(|g| g.id == "401816788").unwrap();
    let sit = g.situation.as_ref().unwrap();
    assert_eq!(
        (
            sit.down,
            sit.distance,
            sit.yard_line,
            sit.is_red_zone,
            sit.drive_desc.as_deref()
        ),
        (None, None, None, None, None)
    );
    assert!(sit.outs.is_some(), "the baseball fields still map");
}

/// Spec v3.4 §3: the red-zone meter is ESPN's `isRedZone` plus the absolute
/// `yardLine`, not `possessionText.rsplit_once(' ')`. Yards-to-goal is the
/// distance to the goal the possessing team is attacking.
#[test]
fn the_red_zone_meter_reads_the_flag_and_the_yard_line() {
    // Away team (id 1) possesses at yardLine 6 => 6 yards from the HOME goal
    // line, which is the goal the away team attacks. `possessionText` here is
    // a shape the old rsplit parse would have choked on.
    let away = r#"{"events":[{"id":"1","competitions":[{"status":{"type":{"state":"in"},"period":4,"displayClock":"1:27"},
      "situation":{"down":1,"distance":6,"yardLine":6,"isRedZone":true,"downDistanceText":"1st & Goal","possessionText":"weird &format 3","possession":"1"},
      "competitors":[{"homeAway":"away","score":"27","team":{"id":"1","abbreviation":"KC"}},{"homeAway":"home","score":"24","team":{"id":"2","abbreviation":"TB"}}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, away, et()).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 6 }));

    // Home team (id 2) at the same coordinate attacks the far goal, so it is
    // 94 yards out — and ESPN says so with isRedZone false.
    let home = away
        .replace(r#""isRedZone":true"#, r#""isRedZone":false"#)
        .replace(r#""possession":"1""#, r#""possession":"2""#);
    let g = &map_scoreboard(League::Nfl, &home, et()).unwrap()[0];
    assert_eq!(g.meter, None);

    // Home team inside its opponent's 20: yardLine 88 => 12 to goal.
    let home_rz = away
        .replace(r#""yardLine":6"#, r#""yardLine":88"#)
        .replace(r#""possession":"1""#, r#""possession":"2""#);
    let g = &map_scoreboard(League::Nfl, &home_rz, et()).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 12 }));
}

/// Spec v3.4 §3: the scoreboard's `situation.lastPlay` is a real `Play` with
/// a `kind` and a `score_value`, mapped through the same per-league tables
/// the summary uses.
#[test]
fn last_play_carries_its_kind_at_scoreboard_cadence() {
    // The live CFB capture's last plays are both type 5 (Rush), scoreValue 0
    // — honestly `Other`, with the score value carried rather than dropped.
    let json = include_str!("../fixtures/live/cfb_scoreboard_live.json");
    let games = map_scoreboard(League::Cfb, json, et()).unwrap();
    let g = games.iter().find(|g| g.id == "401856663").unwrap();
    let lp = &g.last_plays[0];
    assert_eq!(lp.kind, PlayKind::Other, "type 5 Rush is not in the table");
    assert_eq!(lp.score_value, Some(0));

    // A scoring last play goes through `kinds::football_kind`.
    let td = r#"{"events":[{"id":"1","competitions":[{"status":{"type":{"state":"in"},"period":4,"displayClock":"1:27"},
      "situation":{"down":1,"distance":10,"yardLine":94,"isRedZone":true,"downDistanceText":"1st & Goal","possession":"2",
        "lastPlay":{"type":{"id":"67"},"scoringType":{"name":"touchdown"},"scoreValue":6,"text":"Mahomes pass to Kelce for 6 yards","team":{"id":"2"}}},
      "competitors":[{"homeAway":"away","score":"27","team":{"id":"1","abbreviation":"KC"}},{"homeAway":"home","score":"24","team":{"id":"2","abbreviation":"TB"}}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, td, et()).unwrap()[0];
    assert_eq!(g.last_plays[0].kind, PlayKind::Touchdown);
    assert_eq!(g.last_plays[0].score_value, Some(6));

    // MLB's last play is a pitch row; its id runs through the MLB table.
    let mlb = include_str!("../fixtures/live/mlb_scoreboard_live.json");
    let games = map_scoreboard(League::Mlb, mlb, et()).unwrap();
    let g = games.iter().find(|g| g.id == "401816792").unwrap();
    assert_eq!(g.last_plays[0].kind, PlayKind::Other, "type 5 Ball");
    assert_eq!(g.last_plays[0].score_value, Some(0));
}

// ---------------------------------------------------------------- NHL strength (spec v3.4 §4)

#[test]
fn nhl_strength_is_structural_and_current() {
    use gameday::domain::{Extras, HockeyStrength};
    let json = include_str!("../fixtures/live/nhl_summary_final_full.json");
    let s = map_summary(League::Nhl, json).unwrap();
    // The fixture's last play is 522 End of Game at even strength (701), so
    // the game-level strength is Even — the most recent play's, not a
    // scan-for-the-interesting-one.
    assert!(
        matches!(s.extras, Extras::Hockey { strength: HockeyStrength::Even, .. }),
        "expected an even-strength Extras::Hockey, got {:?}",
        s.extras
    );
    // Truncate the same real payload at its last power-play play (a 702 tail)
    // and the strength follows: PIT's 12:36 first-period shot.
    let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
    let plays = v["plays"].as_array().unwrap().clone();
    let last_pp = plays
        .iter()
        .rposition(|p| p["strength"]["id"].as_str() == Some("702"))
        .expect("fixture has 702 plays");
    v["plays"] = serde_json::Value::Array(plays[..=last_pp].to_vec());
    let s = map_summary(League::Nhl, &v.to_string()).unwrap();
    assert!(
        matches!(s.extras, Extras::Hockey { strength: HockeyStrength::PowerPlay, .. }),
        "a 702 tail is a power play: {:?}",
        s.extras
    );
    // …and that tail is what puts Meter::Penalty on the board for the first
    // time since v1. The newest penalty at that point is WSH's 2nd-period
    // minor at 15:37 (the 702 tail is PIT's play at 16:56, inside the two
    // minutes), and `seconds` is the nominal length, not time remaining.
    assert_eq!(
        s.meter,
        Some(Meter::Penalty { team_abbr: "WSH".into(), seconds: 120 }),
        "the special-teams tail builds the penalty meter"
    );
}

#[test]
fn a_finished_game_has_no_penalty_meter() {
    // The strength of the newest play is the only expiry signal the payload
    // carries: the fixture's game-ending play is 701, so nothing is being
    // served and no meter is built.
    let json = include_str!("../fixtures/live/nhl_summary_final_full.json");
    assert_eq!(map_summary(League::Nhl, json).unwrap().meter, None);
}

#[test]
fn penalties_carry_their_metadata() {
    use gameday::domain::{Extras, PenaltyEvent};
    let json = include_str!("../fixtures/live/nhl_summary_final_full.json");
    let s = map_summary(League::Nhl, json).unwrap();
    let Extras::Hockey { penalties, .. } = &s.extras else {
        panic!("expected Extras::Hockey, got {:?}", s.extras);
    };
    // Three penalty plays in the fixture, oldest first. The first is
    // Fehervary (team id 23 = WSH), High-sticking, 1st period at 10:48.
    assert_eq!(penalties.len(), 3, "{penalties:?}");
    assert_eq!(
        penalties[0],
        PenaltyEvent {
            team: "WSH".into(),
            minutes: 2,
            kind: "Minor".into(),
            period: 1,
            clock: "10:48".into(),
        }
    );
    assert_eq!(penalties[2].period, 2, "the newest penalty is the 2nd-period one");
    assert_eq!(penalties[2].clock, "15:37");
}

#[test]
fn the_pp_string_prefix_is_gone() {
    // spec v3.4 §4: strength used to be collapsed into a "PP · " text prefix
    // on power-play goals. No NHL fixture carries a 702 goal, so this flips
    // the real 903 (empty net) goal to 702 — under the old mapper that row's
    // text came back prefixed; strength is structural now, so the text is
    // the feed's own and the strength rides Extras::Hockey.
    use gameday::domain::{Extras, HockeyStrength};
    let json = include_str!("../fixtures/live/nhl_summary_final_full.json");
    let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
    let plays = v["plays"].as_array_mut().unwrap();
    let goal = plays
        .iter_mut()
        .find(|p| p["scoringPlay"].as_bool() == Some(true))
        .expect("fixture has a goal");
    goal["strength"]["id"] = serde_json::Value::String("702".into());
    let text = goal["text"].as_str().unwrap().to_string();
    let s = map_summary(League::Nhl, &v.to_string()).unwrap();
    assert!(
        s.last_plays.iter().all(|p| !p.text.starts_with("PP · ")),
        "no mapped play text wears a strength prefix"
    );
    assert!(
        s.last_plays.iter().any(|p| p.text == text),
        "the power-play goal keeps the feed's own words: {text}"
    );
    assert!(
        matches!(s.extras, Extras::Hockey { .. }),
        "strength lives in Extras::Hockey instead"
    );
    // And the strength enum covers the ids the research pinned.
    assert_ne!(HockeyStrength::PowerPlay, HockeyStrength::Shorthanded);
}
