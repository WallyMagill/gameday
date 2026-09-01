use gameday::domain::*;
use gameday::poll::plan;
use std::time::Duration;

fn g(id: &str, league: League, live: bool) -> Game {
    let t = |a: &str| Team {
        id: a.into(), abbr: a.into(), name: a.into(),
        color: [0; 3], alt_color: [0; 3], logo_key: "nfl/x".into(),
        ..Default::default()
    };
    Game {
        id: id.into(), league, away: t("A"), home: t("B"),
        away_score: 0, home_score: 0,
        status: if live { Status::Live } else { Status::Final },
        period: "".into(), clock: "".into(), situation: None,
        last_plays: vec![], meter: None, ..Game::default()
    }
}

#[test]
fn live_visible_requests_summary_and_fast_scoreboard() {
    let games = [g("1", League::Nfl, true)];
    let p = plan(&games, &[], None);
    assert_eq!(p.scoreboard_every, Duration::from_secs(20));
    assert_eq!(p.summary_every, Duration::from_secs(15));
    assert_eq!(p.summary_ids, vec![(League::Nfl, "1".into())]);
    assert!(p.scoreboard_leagues.contains(&League::Nfl));
}

#[test]
fn no_live_is_slow_and_no_summary() {
    let games = [g("1", League::Nfl, false)];
    let p = plan(&games, &[League::Nfl], None);
    assert_eq!(p.scoreboard_every, Duration::from_secs(60));
    assert!(p.summary_ids.is_empty());
    assert!(p.scoreboard_leagues.contains(&League::Nfl));
}

#[test]
fn extra_league_is_unioned() {
    let games = [g("1", League::Nfl, true)];
    let p = plan(&games, &[League::Cfb], None);
    assert!(p.scoreboard_leagues.contains(&League::Nfl));
    assert!(p.scoreboard_leagues.contains(&League::Cfb));
    assert!(!p.summary_ids.iter().any(|(l, _)| *l == League::Cfb));
}

#[test]
fn does_not_summary_poll_final_games() {
    let games = [g("1", League::Nfl, true), g("2", League::Nfl, false)];
    let p = plan(&games, &[], None);
    assert_eq!(p.summary_ids, vec![(League::Nfl, "1".into())]);
}

#[test]
fn zoomed_game_requests_stats_on_a_30s_cadence() {
    let games = [g("1", League::Nfl, true)];
    let p = plan(&games, &[], Some((League::Nfl, "1".into())));
    assert_eq!(p.stats_for, Some((League::Nfl, "1".into())));
    assert_eq!(p.stats_every, Duration::from_secs(30));
    let p = plan(&games, &[], None);
    assert_eq!(p.stats_for, None, "no zoom, no stats polling");
}
