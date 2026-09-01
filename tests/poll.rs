use gameday::domain::League;
use gameday::poll::*;
use std::time::{Duration, Instant};

fn wants(leagues: &[League], any_live: bool) -> Wants {
    Wants { leagues: leagues.to_vec(), any_live, ..Default::default() }
}

#[test]
fn scoreboards_are_staggered_not_burst() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let w = wants(&League::ALL, true);
    let first = s.due(&w, t0);
    assert_eq!(first.len(), 1, "one league per tick on a cold start, got {first:?}");
    // Across one live window every league gets exactly one scoreboard.
    let mut count = std::collections::HashMap::new();
    for i in 0..(SCOREBOARD_LIVE.as_millis() / 200) {
        for r in s.due(&w, t0 + Duration::from_millis(200 * i as u64)) {
            if let Request::Scoreboard(l) = &r { *count.entry(*l).or_insert(0) += 1; }
            s.report(&r, true, t0 + Duration::from_millis(200 * i as u64));
        }
    }
    for l in League::ALL { assert_eq!(count.get(&l), Some(&1), "{} fetched once per window", l.slug()); }
}

#[test]
fn summary_and_stats_only_for_the_zoomed_game() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Mlb], true);
    // No zoom: never a summary, whatever is live.
    let reqs: Vec<Request> = (0..200).flat_map(|i| s.due(&w, t0 + Duration::from_millis(200 * i))).collect();
    assert!(!reqs.iter().any(|r| matches!(r, Request::Summary(..) | Request::Stats(..))), "{reqs:?}");
    w.zoomed = Some((League::Mlb, "401".into()));
    let reqs = s.due(&w, t0 + Duration::from_secs(60));
    assert!(reqs.contains(&Request::Summary(League::Mlb, "401".into())));
    assert!(reqs.contains(&Request::Stats(League::Mlb, "401".into())), "a fresh zoom fetches stats at once");
}

#[test]
fn ten_minutes_of_nine_live_leagues_and_one_zoom_stays_under_budget() {
    let mut s = Scheduler::new(7);
    let t0 = Instant::now();
    let mut w = wants(&League::ALL, true);
    w.zoomed = Some((League::Nfl, "1".into()));
    let mut n = 0usize;
    for i in 0..(600 * 5) {
        let now = t0 + Duration::from_millis(200 * i);
        for r in s.due(&w, now) { n += 1; s.report(&r, true, now); }
    }
    // 9 leagues / 15s = 36/min, summary 4/min, stats 2/min => 42/min => 420 in 10 min (+jitter slack).
    assert!(n <= 440, "{n} requests in 10 minutes, budget 420 (+5% jitter slack)");
    assert!(n >= 380, "{n} — suspiciously few; the scheduler is starving something");
}

#[test]
fn a_failing_league_backs_off_alone_with_jitter_and_caps() {
    let mut s = Scheduler::new(3);
    let t0 = Instant::now();
    let w = wants(&[League::Nfl, League::Nba], true);
    let _ = s.due(&w, t0);
    s.report(&Request::Scoreboard(League::Nfl), false, t0);
    let r1 = s.next_retry(League::Nfl, t0).unwrap();
    assert!(r1 >= BACKOFF_BASE * 80 / 100 && r1 <= BACKOFF_BASE * 120 / 100, "first retry ≈5s ±20%, got {r1:?}");
    assert!(s.next_retry(League::Nba, t0).is_none(), "NBA is not backed off by NFL's failure");
    for _ in 0..10 { s.report(&Request::Scoreboard(League::Nfl), false, t0); }
    assert!(s.next_retry(League::Nfl, t0).unwrap() <= BACKOFF_CAP * 120 / 100);
    s.report(&Request::Scoreboard(League::Nfl), true, t0);
    assert!(s.next_retry(League::Nfl, t0).is_none(), "success clears the backoff");
}

#[test]
fn refresh_now_makes_every_scoreboard_due_at_once() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Nfl, League::Nba, League::Mlb], false);
    let _ = s.due(&w, t0);
    w.refresh_now = true;
    let reqs = s.due(&w, t0 + Duration::from_millis(200));
    assert_eq!(reqs.iter().filter(|r| matches!(r, Request::Scoreboard(_))).count(), 3);
}

#[test]
fn idle_cadence_is_slow_and_dated_and_standings_fire_on_target_change() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Cfb], false);
    let _ = s.due(&w, t0);
    assert!(s.due(&w, t0 + Duration::from_secs(30)).is_empty(), "idle: nothing due at 30s");
    assert_eq!(s.due(&w, t0 + Duration::from_secs(61)), vec![Request::Scoreboard(League::Cfb)]);
    let d = time::Date::from_calendar_date(2026, time::Month::August, 29).unwrap();
    w.dated = Some((League::Cfb, d));
    w.standings = Some(League::Cfb);
    let reqs = s.due(&w, t0 + Duration::from_secs(62));
    assert!(reqs.contains(&Request::Dated(League::Cfb, d)));
    assert!(reqs.contains(&Request::Standings(League::Cfb)));
    assert!(s.due(&w, t0 + Duration::from_secs(63)).is_empty(), "same targets don't refire inside their windows");
}
