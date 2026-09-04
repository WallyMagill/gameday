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
    #[error("io {0}")]
    Io(#[from] std::io::Error),
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
            ProviderError::Io(e) => format!("disk {e}"),
        }
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
