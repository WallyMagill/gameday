use std::collections::HashMap;

use crate::domain::{Game, GameStats, League, Summary};
use crate::provider::{ProviderError, SportsProvider};

pub struct MemoryProvider {
    pub boards: HashMap<League, Vec<Game>>,
    pub summaries: HashMap<String, Summary>,
    pub stats: HashMap<String, GameStats>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self {
            boards: HashMap::new(),
            summaries: HashMap::new(),
            stats: HashMap::new(),
        }
    }

    pub fn insert_board(&mut self, league: League, games: Vec<Game>) {
        self.boards.insert(league, games);
    }
}

impl SportsProvider for MemoryProvider {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        Ok((self.boards.get(&league).cloned().unwrap_or_default(), false))
    }

    fn summary(&self, _league: League, game_id: &str) -> Result<(Summary, bool), ProviderError> {
        self.summaries
            .get(game_id)
            .cloned()
            .map(|s| (s, false))
            .ok_or_else(|| ProviderError::Http("missing summary".into()))
    }

    fn stats(&self, _league: League, game_id: &str) -> Result<(GameStats, bool), ProviderError> {
        self.stats
            .get(game_id)
            .cloned()
            .map(|s| (s, false))
            .ok_or_else(|| {
                ProviderError::Http(format!(
                    "no stats seeded for game_id={game_id:?}, have: {:?}",
                    self.stats.keys().collect::<Vec<_>>()
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use crate::provider::SportsProvider;

    fn g() -> Game {
        let t = |a: &str| Team {
            id: a.into(),
            abbr: a.into(),
            name: a.into(),
            logo_key: "nfl/x".into(),
            ..Default::default()
        };
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: t("KC"),
            home: t("TB"),
            away_score: 3,
            home_score: 0,
            status: Status::Live,
            period: "Q1".into(),
            clock: "15:00".into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            start_time: None,
            broadcast: None,
        }
    }

    #[test]
    fn scoreboard_returns_inserted() {
        let mut m = MemoryProvider::new();
        m.insert_board(League::Nfl, vec![g()]);
        let (got, stale) = m.scoreboard(League::Nfl).unwrap();
        assert_eq!(got[0].id, "1");
        assert!(!stale);
        let (empty, stale) = m.scoreboard(League::Cfb).unwrap();
        assert!(empty.is_empty());
        assert!(!stale);
    }

    #[test]
    fn summary_by_id() {
        let mut m = MemoryProvider::new();
        m.summaries.insert(
            "1".into(),
            Summary {
                last_plays: vec![Play {
                    clock: "1:00".into(),
                    text: "TD".into(),
                    scoring: true,
                    ..Default::default()
                }],
                scoring_plays: vec![],
                meter: None,
            },
        );
        let (s, stale) = m.summary(League::Nfl, "1").unwrap();
        assert_eq!(s.last_plays[0].text, "TD");
        assert!(!stale);
    }
}
