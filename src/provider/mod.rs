pub mod espn;
pub mod map;
pub mod memory;

use crate::domain::StandingsTable;
use crate::{Game, GameStats, League, Summary};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// `status` 0 means the request never got a response (DNS, TCP, timeout).
    /// `url` is what the message names — the provider stamps the cache key here
    /// when it gives up, and keeps the raw URL in `detail`.
    #[error("http status={status} url={url} {detail}")]
    Http {
        status: u16,
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
                url,
                detail,
            } if detail.contains("timed out") || detail.contains("timeout") => {
                format!("ESPN timeout {}", key_of(url))
            }
            ProviderError::Http { status: 0, url, .. } => {
                format!("ESPN unreachable {}", key_of(url))
            }
            ProviderError::Http { status, url, .. } => format!("ESPN {status} {}", key_of(url)),
            ProviderError::Map { key, .. } => format!("ESPN bad body {}", key_of(key)),
            ProviderError::Io(e) => format!("disk {e}"),
        }
    }
}

/// "…/sports/football/nfl/scoreboard?x" -> "nfl scoreboard"; a cache key
/// (`nfl-scoreboard`) reads the same way once its hyphens are spaces.
fn key_of(url: &str) -> String {
    if !url.contains("://") {
        return url.replace('-', " ");
    }
    let path = url.split('?').next().unwrap_or(url);
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    match parts.as_slice() {
        [res, league] if !league.is_empty() => format!("{league} {res}"),
        _ => url.to_string(),
    }
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
    /// mapping. Polled only for the zoomed game (see `poll::plan`).
    fn stats(&self, league: League, game_id: &str) -> Result<(GameStats, bool), ProviderError>;
    /// League standings — fetched on demand when the Standings view opens,
    /// never polled continuously (see `espn::STANDINGS_TTL`).
    fn standings(&self, league: League) -> Result<(StandingsTable, bool), ProviderError>;
}
