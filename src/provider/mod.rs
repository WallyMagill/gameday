pub mod espn;
pub mod map;
pub mod memory;

use crate::{Game, League, Summary};

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
    fn scoreboard(&self, league: League) -> Result<Vec<Game>, ProviderError>;
    fn summary(&self, league: League, game_id: &str) -> Result<Summary, ProviderError>;
}
