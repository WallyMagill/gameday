//! Request scheduler for the poll thread. Pure: `due` and `report` take the
//! clock as a parameter, so the budget is testable without sleeping.
use crate::domain::League;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// ESPN's scoreboard responds `cache-control: max-age=5` (measured
/// 2026-08-31); 15s = 3x their cache window, so we never ask for a payload
/// they would still be serving from cache.
pub const SCOREBOARD_LIVE: Duration = Duration::from_secs(15);
/// Nothing live anywhere: a minute is fast enough to catch a kickoff.
pub const SCOREBOARD_IDLE: Duration = Duration::from_secs(60);
/// Plays/drives for the zoomed game only — same cadence as the live board.
pub const SUMMARY_EVERY: Duration = Duration::from_secs(15);
/// Box-score cadence for the zoomed game. ~30s: stats move slower than
/// scores/plays, and the payload is the full summary (~hundreds of KB).
pub const STATS_EVERY: Duration = Duration::from_secs(30);
/// Standings freshness window: a cache younger than this is served without
/// touching the network. 10 minutes per the v2 spec ("on demand, cache 10
/// min") — standings move at game granularity, not play granularity.
pub const STANDINGS_TTL: Duration = Duration::from_secs(600);
/// How long a failed dated-slate fetch waits before the same (league, date)
/// is tried again. A guess, not a measurement: without it a transient error
/// would leave the traveled board empty forever; 15s matches the summary
/// cadence so a retry can't hammer ESPN during an outage.
pub const DATED_RETRY: Duration = Duration::from_secs(15);
/// Guess (the unofficial ESPN API sends no `Retry-After`): doubles per
/// consecutive failure, capped at [`BACKOFF_CAP`].
pub const BACKOFF_BASE: Duration = Duration::from_secs(5);
/// Five minutes is the floor on how stale a league's board may get while
/// ESPN is down — long enough to stop hammering, short enough that recovery
/// shows up without a restart.
pub const BACKOFF_CAP: Duration = Duration::from_secs(300);
/// +/-20%: enough to de-synchronize many clients, small enough to keep the
/// cadence readable.
pub const JITTER_PCT: u64 = 20;
/// The poll thread's tick. A slot inside one tick of `now` fires now instead
/// of waiting a whole extra tick, so a 15s cadence stays 15s rather than
/// drifting to 15.2s and shedding a fetch every window.
pub const TICK: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Scoreboard(League),
    Summary(League, String),
    Stats(League, String),
    Dated(League, time::Date),
    Standings(League),
}

/// What the UI wants right now — a snapshot the UI thread publishes.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Wants {
    pub leagues: Vec<League>,
    pub any_live: bool,
    pub zoomed: Option<(League, String)>,
    pub dated: Option<(League, time::Date)>,
    pub standings: Option<League>,
    pub refresh_now: bool,
}

#[derive(Default)]
struct LeagueState {
    next_due: Option<Instant>,
    attempt: u32,
}

pub struct Scheduler {
    leagues: HashMap<League, LeagueState>,
    last_summary: Option<(String, Instant)>,
    last_stats: Option<(String, Instant)>,
    last_dated: Option<((League, time::Date), Instant)>,
    last_standings: Option<(League, Instant)>,
    rng: u64,
}

impl Scheduler {
    pub fn new(seed: u64) -> Self {
        Self {
            leagues: HashMap::new(),
            last_summary: None,
            last_stats: None,
            last_dated: None,
            last_standings: None,
            rng: seed.max(1),
        }
    }

    /// xorshift64 — deterministic jitter so tests can pin the budget.
    fn jitter(&mut self, d: Duration) -> Duration {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let span = d.as_millis() as u64 * JITTER_PCT / 100;
        let off = (self.rng % (2 * span + 1)) as i64 - span as i64;
        Duration::from_millis((d.as_millis() as i64 + off).max(0) as u64)
    }

    /// Requests due at `now`, staggered so N leagues never fire in one burst.
    pub fn due(&mut self, wants: &Wants, now: Instant) -> Vec<Request> {
        let mut out = Vec::new();
        let every = if wants.any_live {
            SCOREBOARD_LIVE
        } else {
            SCOREBOARD_IDLE
        };
        // Stagger: a league that has never been scheduled gets its first slot
        // i*every/n after `now`, so a cold start (or :config enabling nine
        // leagues) spreads across the window instead of firing nine at once.
        let n = wants.leagues.len().max(1) as u32;
        for (i, league) in wants.leagues.iter().enumerate() {
            let st = self.leagues.entry(*league).or_default();
            let slot = *st.next_due.get_or_insert_with(|| now + every * i as u32 / n);
            if wants.refresh_now || slot <= now + TICK {
                out.push(Request::Scoreboard(*league));
                st.next_due = Some(now + every); // provisional; report() re-jitters
            }
        }
        self.leagues.retain(|l, _| wants.leagues.contains(l));
        if let Some((league, id)) = &wants.zoomed {
            let fresh = |last: &Option<(String, Instant)>, every: Duration| {
                last.as_ref()
                    .is_some_and(|(lid, t)| lid == id && now.duration_since(*t) < every)
            };
            if !fresh(&self.last_summary, SUMMARY_EVERY) {
                out.push(Request::Summary(*league, id.clone()));
                self.last_summary = Some((id.clone(), now));
            }
            if !fresh(&self.last_stats, STATS_EVERY) {
                out.push(Request::Stats(*league, id.clone()));
                self.last_stats = Some((id.clone(), now));
            }
        }
        if let Some(target) = wants.dated {
            let fresh = self
                .last_dated
                .is_some_and(|(t, at)| t == target && now.duration_since(at) < DATED_RETRY);
            if !fresh {
                out.push(Request::Dated(target.0, target.1));
                self.last_dated = Some((target, now));
            }
        }
        if let Some(league) = wants.standings {
            let fresh = self
                .last_standings
                .is_some_and(|(l, at)| l == league && now.duration_since(at) < STANDINGS_TTL);
            if !fresh {
                out.push(Request::Standings(league));
                self.last_standings = Some((league, now));
            }
        }
        out
    }

    /// Record an outcome; failures back the league off with jitter.
    pub fn report(&mut self, req: &Request, ok: bool, now: Instant) {
        let Request::Scoreboard(league) = req else {
            return;
        };
        if !ok {
            let st = self.leagues.entry(*league).or_default();
            st.attempt = st.attempt.saturating_add(1);
            let base = (BACKOFF_BASE * 2u32.saturating_pow(st.attempt - 1)).min(BACKOFF_CAP);
            let j = self.jitter(base);
            if let Some(st) = self.leagues.get_mut(league) {
                st.next_due = Some(now + j);
            }
            return;
        }
        // Success: clear the backoff and re-jitter the provisional slot `due`
        // set, so many clients don't converge on the same instant.
        let Some(st) = self.leagues.get_mut(league) else {
            return;
        };
        st.attempt = 0;
        let Some(slot) = st.next_due else { return };
        let every = slot.saturating_duration_since(now);
        let jittered = self.jitter(every);
        if let Some(st) = self.leagues.get_mut(league) {
            st.next_due = Some(now + jittered);
        }
    }

    /// How long until `league`'s backed-off retry, or None when it is healthy.
    pub fn next_retry(&self, league: League, now: Instant) -> Option<Duration> {
        let st = self.leagues.get(&league)?;
        (st.attempt > 0).then(|| {
            st.next_due
                .map(|d| d.saturating_duration_since(now))
                .unwrap_or_default()
        })
    }
}
