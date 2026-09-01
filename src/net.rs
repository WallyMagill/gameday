//! Connection truth: one small state machine that answers "is what you are
//! looking at real right now?" — the header chip, the footer's freshness
//! label, and the board's offline message all read it, so they can never
//! disagree with each other.
//!
//! It runs on `std::time::Instant` (monotonic) on purpose: a wall-clock jump
//! (sleep/wake, NTP step) must not turn a fresh board stale or a stale board
//! fresh. `App::now()`'s frozen clock is for rendering dates, not for
//! measuring how old a fetch is.

use std::time::{Duration, Instant};

/// How old the last fresh board may get before the header stops claiming the
/// numbers are live. 3× the live scoreboard cadence (15 s, `poll::SCOREBOARD_LIVE`):
/// one missed poll is noise, three in a row is a problem worth naming.
pub const STALE_AFTER: Duration = Duration::from_secs(45);

/// What the header chip says. `Live` is the silent state — the tiles already
/// say LIVE, so a chip there would be noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetChip {
    NoDataYet,
    Live,
    Stale {
        age: Duration,
    },
    Offline {
        retry_in: Option<Duration>,
        error: String,
    },
}

impl NetChip {
    /// The chip's text, or None while `Live` (nothing to say).
    pub fn label(&self) -> Option<String> {
        match self {
            NetChip::Live => None,
            NetChip::NoDataYet => Some("NO DATA YET".to_string()),
            NetChip::Stale { age } => Some(format!("STALE {}", short_age(age.as_secs()))),
            NetChip::Offline { retry_in, .. } => Some(match retry_in {
                Some(d) => format!("OFFLINE · retry {}", short_age(d.as_secs())),
                None => "OFFLINE".to_string(),
            }),
        }
    }
}

/// Every successful apply and every failed fetch, reduced to the four states
/// above. `Default` is "the app just started and has never seen a board".
#[derive(Debug, Default, Clone)]
pub struct NetStatus {
    /// When the last board was applied, fresh or from cache.
    last_ok: Option<Instant>,
    /// Whether that last apply was served from the on-disk cache.
    last_ok_stale: bool,
    last_err: Option<(Instant, String, Option<Duration>)>,
    ever_ok: bool,
}

impl NetStatus {
    /// A board was applied. `stale = true` means the provider served it from
    /// cache, which is instantly a `Stale` chip however recent the apply is.
    pub fn ok(&mut self, now: Instant, stale: bool) {
        self.last_ok = Some(now);
        self.last_ok_stale = stale;
        self.ever_ok = true;
    }

    /// A scoreboard fetch failed. `retry_in` is the delay the scheduler has
    /// already picked, so the chip can name the next attempt.
    pub fn failed(&mut self, now: Instant, error: String, retry_in: Option<Duration>) {
        self.last_err = Some((now, error, retry_in));
    }

    pub fn chip(&self, now: Instant) -> NetChip {
        let age = self
            .last_ok
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or_default();
        // "Live" is the narrow claim: the last event was a fresh board, and
        // it is younger than STALE_AFTER.
        let fresh = self.ever_ok && !self.last_ok_stale && age < STALE_AFTER;
        if let Some((at, error, retry_in)) = &self.last_err {
            // A failure outranks the board it followed unless a fresh board
            // is still standing behind it — one failed poll on top of a
            // 2-second-old board is noise, not an outage. With no board at
            // all, the failure is the only thing we know.
            if !fresh && self.last_ok.is_none_or(|t| *at >= t) {
                return NetChip::Offline {
                    retry_in: *retry_in,
                    error: error.clone(),
                };
            }
        }
        if !self.ever_ok {
            return NetChip::NoDataYet;
        }
        if fresh {
            NetChip::Live
        } else {
            NetChip::Stale { age }
        }
    }

    /// The footer's freshness age: "UPD 12s" while live, and frozen with a
    /// trailing ` ·` while stale or offline — the number stops being a
    /// promise the moment the data stops arriving. None before first data.
    pub fn upd_label(&self, now: Instant) -> Option<String> {
        let last_ok = self.last_ok?;
        let secs = now.saturating_duration_since(last_ok).as_secs();
        let label = format!("UPD {}", short_age(secs));
        Some(match self.chip(now) {
            NetChip::Live => label,
            _ => format!("{label} ·"),
        })
    }
}

/// "12s" under a minute, whole minutes past it. Same shape everywhere an age
/// or a delay is rendered, so the eye reads them as one kind of number.
fn short_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn chip_walks_no_data_live_stale_offline_and_back() {
        let t0 = Instant::now();
        let mut n = NetStatus::default();
        assert!(matches!(n.chip(t0), NetChip::NoDataYet));
        assert_eq!(n.upd_label(t0), None);
        n.ok(t0, false);
        assert!(matches!(n.chip(t0 + Duration::from_secs(10)), NetChip::Live));
        assert_eq!(
            n.upd_label(t0 + Duration::from_secs(10)).as_deref(),
            Some("UPD 10s")
        );
        let later = t0 + STALE_AFTER + Duration::from_secs(1);
        assert!(matches!(n.chip(later), NetChip::Stale { .. }));
        assert_eq!(
            n.upd_label(later).as_deref(),
            Some("UPD 46s ·"),
            "frozen marker while stale"
        );
        n.failed(
            later,
            "ESPN 403 nfl scoreboard".into(),
            Some(Duration::from_secs(40)),
        );
        match n.chip(later) {
            NetChip::Offline { retry_in, error } => {
                assert_eq!(retry_in, Some(Duration::from_secs(40)));
                assert!(error.contains("403"));
            }
            other => panic!("{other:?}"),
        }
        n.ok(later + Duration::from_secs(5), false);
        assert!(
            matches!(n.chip(later + Duration::from_secs(6)), NetChip::Live),
            "recovery clears offline"
        );
    }

    #[test]
    fn a_cached_apply_is_stale_immediately() {
        let t0 = Instant::now();
        let mut n = NetStatus::default();
        n.ok(t0, true);
        assert!(matches!(n.chip(t0), NetChip::Stale { .. }));
    }

    #[test]
    fn labels_read_like_the_header() {
        let t0 = Instant::now();
        let mut n = NetStatus::default();
        assert_eq!(n.chip(t0).label().as_deref(), Some("NO DATA YET"));
        n.ok(t0, false);
        assert_eq!(n.chip(t0).label(), None, "the tiles already say LIVE");
        assert_eq!(
            n.chip(t0 + Duration::from_secs(240)).label().as_deref(),
            Some("STALE 4m")
        );
        n.failed(
            t0 + Duration::from_secs(240),
            "ESPN 403 nfl scoreboard".into(),
            Some(Duration::from_secs(40)),
        );
        assert_eq!(
            n.chip(t0 + Duration::from_secs(240)).label().as_deref(),
            Some("OFFLINE · retry 40s")
        );
    }
}
