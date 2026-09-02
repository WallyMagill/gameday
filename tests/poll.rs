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
    let first = s.due(&w, false, t0);
    assert_eq!(first.len(), 1, "one league per tick on a cold start, got {first:?}");
    // Then, past the cold pass, one scoreboard per league per live window —
    // counted over four windows so the ±20% jitter can't make an exact count
    // a coin flip: 60s / 15s = 4 each, and jitter can shift one either way.
    let mut count = std::collections::HashMap::new();
    let start = COLD_SPREAD + TICK;
    for i in 0..((start + Duration::from_secs(60)).as_millis() / 200) {
        let now = t0 + Duration::from_millis(200 * i as u64);
        for r in s.due(&w, false, now) {
            if let Request::Scoreboard(l) = &r {
                if now >= t0 + start {
                    *count.entry(*l).or_insert(0) += 1;
                }
            }
            s.report(&r, true, now);
        }
    }
    for l in League::ALL {
        let n = count.get(&l).copied().unwrap_or(0);
        assert!((3..=5).contains(&n), "{} fetched {n}x in 60s, expected ~4", l.slug());
    }
}

#[test]
fn a_cold_start_spreads_its_first_pass_over_cold_spread_not_the_idle_minute() {
    // The defect this pins: with nothing live, the first pass used to spread
    // over the 60s idle cadence, so Home named the wrong "next" game until the
    // board that had the real answer arrived at the tail of that minute.
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let w = wants(&League::ALL, false);
    let mut fired: Vec<League> = Vec::new();
    for i in 0..=((COLD_SPREAD + TICK).as_millis() / 200) {
        let now = t0 + Duration::from_millis(200 * i as u64);
        for r in s.due(&w, false, now) {
            if let Request::Scoreboard(l) = &r {
                fired.push(*l);
            }
            s.report(&r, true, now);
        }
    }
    for l in League::ALL {
        assert!(fired.contains(&l), "{} never fired inside COLD_SPREAD: {fired:?}", l.slug());
    }
    // Spread, not burst: nine leagues never land on one tick.
    assert!(s.due(&w, false, t0).len() <= 2);
}

#[test]
fn a_backed_off_league_is_not_clamped_when_the_cadence_shrinks() {
    let mut s = Scheduler::new(3);
    let t0 = Instant::now();
    let mut w = wants(&[League::Nfl], false);
    let _ = s.due(&w, false, t0);
    // Ten failures: the backoff is minutes out, far past the live cadence.
    for _ in 0..10 {
        s.report(&Request::Scoreboard(League::Nfl), false, t0);
    }
    let before = s.next_retry(League::Nfl, t0).unwrap();
    assert!(before > SCOREBOARD_LIVE, "backoff should be minutes, got {before:?}");
    w.any_live = true;
    let _ = s.due(&w, false, t0 + Duration::from_millis(200));
    let after = s.next_retry(League::Nfl, t0).unwrap();
    assert_eq!(before, after, "a live cadence must not pull a backed-off retry forward");
}

#[test]
fn a_failed_standings_fetch_retries_at_aux_retry_not_the_ttl() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Cfb], false);
    w.standings = Some(League::Cfb);
    assert!(s.due(&w, false, t0).contains(&Request::Standings(League::Cfb)));
    s.report_aux(&Request::Standings(League::Cfb), false, t0);
    assert!(
        !s.due(&w, false, t0 + AUX_RETRY - Duration::from_secs(1))
            .contains(&Request::Standings(League::Cfb)),
        "still inside the retry window"
    );
    assert!(
        s.due(&w, false, t0 + AUX_RETRY + TICK)
            .contains(&Request::Standings(League::Cfb)),
        "a failed standings fetch retries at {AUX_RETRY:?}, not {STANDINGS_TTL:?}"
    );
    // A success leaves the TTL alone.
    s.report_aux(&Request::Standings(League::Cfb), true, t0 + AUX_RETRY + TICK);
    assert!(!s
        .due(&w, false, t0 + AUX_RETRY + TICK + Duration::from_secs(60))
        .contains(&Request::Standings(League::Cfb)));
}

#[test]
fn summary_and_stats_only_for_the_zoomed_game() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Mlb], true);
    // No zoom: never a summary, whatever is live.
    let reqs: Vec<Request> = (0..200).flat_map(|i| s.due(&w, false, t0 + Duration::from_millis(200 * i))).collect();
    assert!(!reqs.iter().any(|r| matches!(r, Request::Summary(..) | Request::Stats(..))), "{reqs:?}");
    w.zoomed = Some((League::Mlb, "401".into()));
    let reqs = s.due(&w, false, t0 + Duration::from_secs(60));
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
        for r in s.due(&w, false, now) { n += 1; s.report(&r, true, now); }
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
    let _ = s.due(&w, false, t0);
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
    let w = wants(&[League::Nfl, League::Nba, League::Mlb], false);
    let _ = s.due(&w, false, t0);
    let reqs = s.due(&w, true, t0 + Duration::from_millis(200));
    assert_eq!(reqs.iter().filter(|r| matches!(r, Request::Scoreboard(_))).count(), 3);
}

#[test]
fn idle_cadence_is_slow_and_dated_and_standings_fire_on_target_change() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Cfb], false);
    let _ = s.due(&w, false, t0);
    assert!(s.due(&w, false, t0 + Duration::from_secs(30)).is_empty(), "idle: nothing due at 30s");
    assert_eq!(s.due(&w, false, t0 + Duration::from_secs(61)), vec![Request::Scoreboard(League::Cfb)]);
    let d = time::Date::from_calendar_date(2026, time::Month::August, 29).unwrap();
    w.dated = Some((League::Cfb, d));
    w.standings = Some(League::Cfb);
    let reqs = s.due(&w, false, t0 + Duration::from_secs(62));
    assert!(reqs.contains(&Request::Dated(League::Cfb, d)));
    assert!(reqs.contains(&Request::Standings(League::Cfb)));
    assert!(s.due(&w, false, t0 + Duration::from_secs(63)).is_empty(), "same targets don't refire inside their windows");
}

#[test]
fn going_live_pulls_pending_idle_slots_forward() {
    // A league scheduled a slow idle minute out must not stay parked there
    // once a game goes live — the faster cadence pulls the slot forward.
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Nfl], false);
    assert_eq!(s.due(&w, false, t0), vec![Request::Scoreboard(League::Nfl)]);
    assert!(s.due(&w, false, t0 + Duration::from_secs(30)).is_empty(), "idle: still parked at 30s");
    w.any_live = true;
    let mut fired = None;
    for i in 0..((SCOREBOARD_LIVE + TICK).as_millis() / 200) {
        let now = t0 + Duration::from_millis(200 * i as u64);
        if !s.due(&w, false, now).is_empty() {
            fired = Some(now);
            break;
        }
    }
    assert!(
        fired.is_some(),
        "going live must make the board due within SCOREBOARD_LIVE, not the idle minute"
    );
}
