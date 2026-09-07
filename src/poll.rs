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
/// touching the network. 10 minutes: standings are fetched on demand and
/// move at game granularity, not play granularity.
pub const STANDINGS_TTL: Duration = Duration::from_secs(600);
/// How long a failed dated-slate fetch waits before the same (league, date)
/// is tried again. A guess, not a measurement: without it a transient error
/// would leave the traveled board empty forever; 15s matches the summary
/// cadence so a retry can't hammer ESPN during an outage.
pub const DATED_RETRY: Duration = Duration::from_secs(15);
/// How long a failed on-demand fetch (standings, a dated slate, the zoomed
/// game's summary/stats) waits before it is asked for again. Same 15s as
/// [`DATED_RETRY`]: a failed fetch is retried at the summary cadence, not
/// left behind the 10-minute standings TTL where the user would stare at an
/// empty table for the rest of the session.
pub const AUX_RETRY: Duration = Duration::from_secs(15);
/// The window a cold start spreads its first scoreboard pass over. Nine
/// leagues over 5s is < 2 req/s, well inside the measured tolerance (40
/// rapid requests, zero non-200s); the alternative — spreading the first
/// pass over the full idle minute — left Home naming the wrong next game for
/// ~40s while the board that had the answer hadn't been fetched yet.
pub const COLD_SPREAD: Duration = Duration::from_secs(5);
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

/// One score delta the UI could not attribute to a scoring play: the
/// scoreboard's `lastPlay` at that poll was a later snap or pitch. The
/// summary is the authority, so the UI asks for it exactly once per
/// sequence number. `seq` is monotonic per App, so the same game scoring
/// twice is two requests and a republished snapshot is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchupReq {
    pub league: League,
    pub game_id: String,
    pub seq: u64,
}

/// What the UI wants right now — a snapshot the UI thread publishes.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Wants {
    pub leagues: Vec<League>,
    pub any_live: bool,
    pub zoomed: Option<(League, String)>,
    pub dated: Option<(League, time::Date)>,
    pub standings: Option<League>,
    pub catchup: Vec<CatchupReq>,
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
    /// The scoreboard interval the last `due` pass used, so a cadence that
    /// speeds up (idle -> live) can pull already-scheduled slots forward.
    last_every: Option<Duration>,
    last_catchup_seq: u64,
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
            last_every: None,
            last_catchup_seq: 0,
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
    /// `refresh_now` is the UI's one-shot R: it makes every scoreboard due.
    pub fn due(&mut self, wants: &Wants, refresh_now: bool, now: Instant) -> Vec<Request> {
        let mut out = Vec::new();
        let every = if wants.any_live {
            SCOREBOARD_LIVE
        } else {
            SCOREBOARD_IDLE
        };
        // A cadence that just got faster (a game went live) pulls every
        // pending slot forward, so a league parked an idle minute out isn't
        // stuck at that minute while the board is live. Only on the change:
        // clamping every pass would also cancel the upward half of the
        // jitter and quietly raise the request budget.
        if self.last_every.is_some_and(|last| every < last) {
            for st in self.leagues.values_mut() {
                // A backed-off league keeps its backoff: the cadence changing
                // says nothing about whether ESPN started answering again,
                // and pulling the retry forward would hammer an outage.
                if st.attempt > 0 {
                    continue;
                }
                if let Some(slot) = st.next_due {
                    st.next_due = Some(slot.min(now + every));
                }
            }
        }
        self.last_every = Some(every);
        // Stagger: a league that has never been scheduled gets its first slot
        // i*COLD_SPREAD/n after `now`, so a cold start (or :config enabling
        // nine leagues) spreads over five seconds instead of firing nine at
        // once — and Home doesn't spend most of an idle minute naming the
        // wrong "next" game because the board that had it hadn't arrived yet.
        // After that first fetch each league re-arms at `now + every` and
        // `report` re-jitters it, so the spread persists rather than
        // collapsing into a burst.
        let n = wants.leagues.len().max(1) as u32;
        for (i, league) in wants.leagues.iter().enumerate() {
            let st = self.leagues.entry(*league).or_default();
            let slot = *st
                .next_due
                .get_or_insert_with(|| now + COLD_SPREAD * i as u32 / n);
            if refresh_now || slot <= now + TICK {
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
        // Catch-ups: one Summary per new sequence number, and no freshness
        // window of their own — the ask is already paced on the UI side (a
        // retry waits a poll) and bounded by its attempt count, so a second
        // gate here would only delay the answer. The live budget measured on
        // 2026-09-07 was 0 catch-up summaries against three score changes;
        // every delta took the fast path.
        for c in wants
            .catchup
            .iter()
            .filter(|c| c.seq > self.last_catchup_seq)
        {
            // The zoomed game's summary is this same fetch. If the block
            // above already asked for it this pass, the catch-up rides that
            // request rather than sending a second identical one.
            if out
                .iter()
                .any(|r| matches!(r, Request::Summary(_, id) if id == &c.game_id))
            {
                continue;
            }
            out.push(Request::Summary(c.league, c.game_id.clone()));
            // …and when the catch-up is the one asking for the zoomed game,
            // it stamps the zoom's freshness window too: the data landed, so
            // the next pass must not immediately repeat it from that side.
            if wants
                .zoomed
                .as_ref()
                .is_some_and(|(_, id)| id == &c.game_id)
            {
                self.last_summary = Some((c.game_id.clone(), now));
            }
        }
        if let Some(max) = wants.catchup.iter().map(|c| c.seq).max() {
            self.last_catchup_seq = self.last_catchup_seq.max(max);
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

    /// Record an on-demand fetch's outcome. `due` paces these by "when did I
    /// last ask", which on a failure would park standings behind the 10-minute
    /// TTL — so a failure rewinds that stamp to [`AUX_RETRY`] before the
    /// window closes, and the next pass past that retries. Success is a no-op:
    /// `due` already stamped it.
    pub fn report_aux(&mut self, req: &Request, ok: bool, now: Instant) {
        if ok {
            return;
        }
        // `now - (every - AUX_RETRY)`: the window has AUX_RETRY left to run.
        let rewind = |every: Duration| {
            now.checked_sub(every.saturating_sub(AUX_RETRY))
                .unwrap_or(now)
        };
        match req {
            Request::Scoreboard(_) => {}
            Request::Summary(_, id) => {
                self.last_summary = Some((id.clone(), rewind(SUMMARY_EVERY)));
            }
            Request::Stats(_, id) => {
                self.last_stats = Some((id.clone(), rewind(STATS_EVERY)));
            }
            Request::Dated(league, date) => {
                self.last_dated = Some(((*league, *date), rewind(DATED_RETRY)));
            }
            Request::Standings(league) => {
                self.last_standings = Some((*league, rewind(STANDINGS_TTL)));
            }
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
