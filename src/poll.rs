use crate::domain::{Game, League, Status};
use std::time::Duration;

pub struct PollPlan {
    pub scoreboard_leagues: Vec<League>,
    pub summary_ids: Vec<(League, String)>, // visible live only
    pub scoreboard_every: Duration,
    pub summary_every: Duration,
}

// One global cadence for all nine leagues on purpose: payload sizes were
// checked (MLB scoreboard ~372KB, CBB ~268KB) and showed no latency/rate
// problem worth a per-league schedule.
pub fn plan(visible_games: &[Game], extra_leagues: &[League]) -> PollPlan {
    let summary_ids: Vec<(League, String)> = visible_games
        .iter()
        .filter(|g| g.status == Status::Live)
        .map(|g| (g.league, g.id.clone()))
        .collect();

    let has_live = !summary_ids.is_empty();
    let scoreboard_every = if has_live {
        Duration::from_secs(20)
    } else {
        Duration::from_secs(60)
    };

    let mut scoreboard_leagues: Vec<League> = Vec::new();
    for league in extra_leagues {
        if !scoreboard_leagues.contains(league) {
            scoreboard_leagues.push(*league);
        }
    }
    for game in visible_games {
        if !scoreboard_leagues.contains(&game.league) {
            scoreboard_leagues.push(game.league);
        }
    }

    PollPlan {
        scoreboard_leagues,
        summary_ids,
        scoreboard_every,
        summary_every: Duration::from_secs(15),
    }
}
