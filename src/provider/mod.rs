pub mod espn;
pub mod map;
pub mod memory;

use crate::domain::StandingsTable;
use crate::{Game, GameStats, League, Summary};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http {0}")]
    Http(String),
    #[error("map {0}")]
    Map(#[from] map::MapError),
    #[error("io {0}")]
    Io(#[from] std::io::Error),
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
