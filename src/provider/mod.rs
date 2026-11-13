pub mod espn;
pub mod kinds;
pub mod map;
pub mod memory;

use crate::domain::StandingsTable;
use crate::{Game, GameStats, League, Summary};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// `status` 0 means the request never got a response (DNS, TCP, timeout).
    /// `key` is the resource that failed (the cache key, `nfl-scoreboard`) —
    /// that is what a reader is shown; `url` is always the real URL, for logs.
    #[error("http status={status} {key} url={url} {detail}")]
    Http {
        status: u16,
        key: String,
        url: String,
        detail: String,
    },
    #[error("map {key}: {source}")]
    Map {
        key: String,
        #[source]
        source: map::MapError,
    },
    /// ESPN answered (or redirected), and gameday refused the answer. Its
    /// own variant so the footer can say what happened instead of
    /// "unreachable", without reading the detail text.
    #[error("refused {key} url={url} {why}")]
    Refused {
        key: String,
        url: String,
        why: Refusal,
    },
    #[error("io {0}")]
    Io(#[from] std::io::Error),
}

/// Why an answer was refused. Each `Display` names the value, the
/// expectation, and the knob.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The decoded body ran past `cap` bytes; reading stopped one byte later.
    #[error(
        "decoded body passed {cap} bytes (reading stopped at {}), expected at most {cap} (MAX_BODY_BYTES in src/provider/espn.rs)",
        cap.saturating_add(1)
    )]
    TooLarge { cap: u64 },
    /// A redirect pointed at a URL that is not https.
    #[error(
        "redirect to {to}, expected https (https_only in EspnProvider::new, src/provider/espn.rs)"
    )]
    NotHttps { to: String },
}

impl ProviderError {
    /// One footer-sized phrase: what failed and where. `key` is the cache key
    /// (`nfl-scoreboard`), which reads as "league resource".
    pub fn short(&self) -> String {
        match self {
            ProviderError::Http {
                status: 0,
                key,
                detail,
                ..
            } if detail.contains("timed out") || detail.contains("timeout") => {
                format!("ESPN timeout {}", key_of(key))
            }
            ProviderError::Http { status: 0, key, .. } => {
                format!("ESPN unreachable {}", key_of(key))
            }
            ProviderError::Http { status, key, .. } => format!("ESPN {status} {}", key_of(key)),
            ProviderError::Map { key, .. } => format!("ESPN bad body {}", key_of(key)),
            ProviderError::Refused {
                key,
                why: Refusal::TooLarge { .. },
                ..
            } => format!("ESPN too big {}", key_of(key)),
            ProviderError::Refused {
                key,
                why: Refusal::NotHttps { .. },
                ..
            } => format!("ESPN not https {}", key_of(key)),
            ProviderError::Io(e) => format!("disk {e}"),
        }
    }

    /// [`short`](Self::short) for the footer or stderr, after writing the
    /// full text (status, URL, detail) to gameday.log. The poll loop meets
    /// the same failure every 15 s while it lasts, so the line is written
    /// once per distinct text (`log::note_once`), like the mapper's skips.
    pub fn short_logged(&self) -> String {
        let full = self.to_string();
        crate::log::note_once(&format!("provider:{full}"), &format!("gameday: {full}"));
        self.short()
    }
}

/// A cache key (`nfl-scoreboard`) reads as "league resource" once its hyphens
/// are spaces. Only ever handed keys — never URLs.
fn key_of(key: &str) -> String {
    key.replace('-', " ")
}

pub trait SportsProvider {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError>;
    /// Scoreboard for a specific (non-today) date — the slate time-travel
    /// path. Fetched once when the viewed date changes, cached per date,
    /// never live-polled (past/future slates don't move play-by-play).
    fn scoreboard_on(
        &self,
        league: League,
        date: time::Date,
    ) -> Result<(Vec<Game>, bool), ProviderError>;
    fn summary(&self, league: League, game_id: &str) -> Result<(Summary, bool), ProviderError>;
    /// Box score for one game — same summary payload as `summary`, different
    /// mapping. Polled only for the zoomed game (see `crate::poll::Scheduler`).
    fn stats(&self, league: League, game_id: &str) -> Result<(GameStats, bool), ProviderError>;
    /// League standings — fetched on demand when the Standings view opens,
    /// never polled continuously (see `crate::poll::STANDINGS_TTL`).
    fn standings(&self, league: League) -> Result<(StandingsTable, bool), ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The footer gets the short form; gameday.log gets the full text, once
    /// per distinct failure however many polls repeat it.
    #[test]
    fn short_logged_writes_the_full_text_to_the_log_once() {
        let _g = crate::log::test_guard();
        let dir = std::env::temp_dir().join(format!("gd-short-logged-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gameday.log");
        crate::log::set_file(path.clone());
        let err = ProviderError::Refused {
            key: "nfl-scoreboard".into(),
            url: "https://example.invalid/short-logged".into(),
            why: Refusal::TooLarge { cap: 7 },
        };
        assert_eq!(err.short_logged(), "ESPN too big nfl scoreboard");
        assert_eq!(err.short_logged(), "ESPN too big nfl scoreboard");
        let log = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("example.invalid/short-logged"))
            .collect();
        assert_eq!(lines.len(), 1, "one line for a repeated failure: {log:?}");
        assert!(lines[0].ends_with(&format!("gameday: {err}")), "{lines:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
