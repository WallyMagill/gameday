//! Favorite-score alerts: after each board apply, a favorited team's score
//! delta rings the terminal bell and flashes a header banner, any tab, with a
//! per-game cooldown (spec §3). Deltas are diffed here against this module's
//! own score memory — seeded silently on first sighting so startup never
//! alerts — because `App::apply_boards` has already overwritten its
//! `last_scores` by the time it can ask.

use crate::app::LIVE_TICKS_PER_SEC;
use crate::config::Favorite;
use crate::domain::{Game, League};
use std::collections::HashMap;

/// Per-game quiet window after an alert fires (spec: "30s per-game cooldown").
const COOLDOWN_SECS: u64 = 30;
/// How long the banner stays in the header. A guess at "long enough to read a
/// short line, short enough not to own the header" — not measured.
const BANNER_SECS: u64 = 8;

/// Cooldown in render ticks. Alerts only fire while a game is live, so the
/// live cadence is the honest conversion; at the idle 1 tick/s a stale banner
/// merely lingers longer, it never re-fires early.
pub const COOLDOWN_TICKS: u64 = COOLDOWN_SECS * LIVE_TICKS_PER_SEC;
/// Banner lifetime in render ticks, same cadence reasoning as the cooldown.
pub const BANNER_TICKS: u64 = BANNER_SECS * LIVE_TICKS_PER_SEC;

/// One active header banner, e.g. `★ KC SCORES  27-24` (favorite's own score
/// first). Dropped by `App::advance_tick` once `until_tick` passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub text: String,
    pub until_tick: u64,
}

/// Score memory + cooldown ledger for the alert diff. Owned by `App`,
/// consulted once per `apply_boards`.
#[derive(Default)]
pub struct AlertState {
    /// Last-seen (away, home) per game id — the delta source.
    last_scores: HashMap<String, (u16, u16)>,
    /// game id -> first tick at which it may alert again.
    cooldown_until: HashMap<String, u64>,
}

impl AlertState {
    /// Diff `boards` against the previous check; the first favorited team
    /// whose own score moved (in a game off cooldown) yields the banner
    /// alert. Every delta is consumed either way — a cooled-down or
    /// non-favorite score change never alerts retroactively.
    pub fn check(
        &mut self,
        favorites: &[Favorite],
        boards: &HashMap<League, Vec<Game>>,
        tick: u64,
    ) -> Option<Alert> {
        let mut fired: Option<Alert> = None;
        for game in boards.values().flatten() {
            let score = (game.away_score, game.home_score);
            let Some(prev) = self.last_scores.insert(game.id.clone(), score) else {
                continue; // first sighting seeds silently
            };
            if prev == score || fired.is_some() {
                continue;
            }
            if self
                .cooldown_until
                .get(&game.id)
                .is_some_and(|&until| tick < until)
            {
                continue;
            }
            // The favorited side whose own score moved (not just "a favorite
            // is playing" — the opponent scoring is not their alert).
            let scored = [
                (&game.away, game.away_score, prev.0, game.home_score),
                (&game.home, game.home_score, prev.1, game.away_score),
            ]
            .into_iter()
            .find(|(team, now, before, _)| {
                now != before
                    && favorites.iter().any(|f| {
                        f.league == game.league && f.team_abbr.eq_ignore_ascii_case(&team.abbr)
                    })
            });
            if let Some((team, own, _, other)) = scored {
                self.cooldown_until
                    .insert(game.id.clone(), tick + COOLDOWN_TICKS);
                fired = Some(Alert {
                    text: format!("★ {} SCORES  {own}-{other}", team.abbr),
                    until_tick: tick + BANNER_TICKS,
                });
            }
        }
        // Drop memory for games no board carries any more — unbounded growth
        // over a days-long session otherwise, and a recycled id would diff
        // against a dead score instead of seeding silently.
        self.last_scores
            .retain(|id, _| boards.values().flatten().any(|g| g.id == *id));
        self.cooldown_until
            .retain(|id, _| boards.values().flatten().any(|g| g.id == *id));
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Status, Team};

    fn team(abbr: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            logo_key: format!("nfl/{}", abbr.to_lowercase()),
            ..Default::default()
        }
    }

    fn game(id: &str, away: &str, home: &str, away_score: u16, home_score: u16) -> Game {
        Game {
            id: id.into(),
            league: League::Nfl,
            away: team(away),
            home: team(home),
            away_score,
            home_score,
            status: Status::Live,
            period: "Q4".into(),
            clock: "1:27".into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            ..Game::default()
        }
    }

    fn boards(games: Vec<Game>) -> HashMap<League, Vec<Game>> {
        HashMap::from([(League::Nfl, games)])
    }

    fn kc_favorite() -> Vec<Favorite> {
        vec![Favorite {
            league: League::Nfl,
            team_abbr: "KC".into(),
        }]
    }

    #[test]
    fn favorite_score_delta_alerts_with_star_text() {
        let mut state = AlertState::default();
        let favs = kc_favorite();
        // First sighting seeds silently — startup never alerts.
        assert_eq!(
            state.check(&favs, &boards(vec![game("1", "KC", "TB", 20, 24)]), 0),
            None
        );
        let alert = state
            .check(&favs, &boards(vec![game("1", "KC", "TB", 27, 24)]), 5)
            .expect("favorite delta must alert");
        assert_eq!(alert.text, "★ KC SCORES  27-24");
        assert_eq!(alert.until_tick, 5 + BANNER_TICKS);
    }

    #[test]
    fn same_game_within_cooldown_stays_silent_then_rearms() {
        let mut state = AlertState::default();
        let favs = kc_favorite();
        state.check(&favs, &boards(vec![game("1", "KC", "TB", 20, 24)]), 0);
        assert!(state
            .check(&favs, &boards(vec![game("1", "KC", "TB", 27, 24)]), 5)
            .is_some());
        // Another delta inside the 30s window: silent, and the delta is consumed.
        let inside = 5 + COOLDOWN_TICKS - 1;
        assert_eq!(
            state.check(&favs, &boards(vec![game("1", "KC", "TB", 30, 24)]), inside),
            None
        );
        // Past the window a fresh delta alerts again.
        let past = 5 + COOLDOWN_TICKS;
        assert!(state
            .check(&favs, &boards(vec![game("1", "KC", "TB", 37, 24)]), past)
            .is_some());
    }

    #[test]
    fn non_favorite_delta_is_silent() {
        let mut state = AlertState::default();
        let favs = kc_favorite();
        // DAL@PHI has no favorite; TB scoring against favorite KC is also
        // not KC's alert.
        state.check(
            &favs,
            &boards(vec![
                game("1", "DAL", "PHI", 0, 0),
                game("2", "KC", "TB", 20, 24),
            ]),
            0,
        );
        assert_eq!(
            state.check(
                &favs,
                &boards(vec![
                    game("1", "DAL", "PHI", 7, 0),
                    game("2", "KC", "TB", 20, 31)
                ]),
                5
            ),
            None
        );
    }

    #[test]
    fn favorite_league_must_match() {
        let mut state = AlertState::default();
        let favs = vec![Favorite {
            league: League::Nba,
            team_abbr: "KC".into(),
        }];
        state.check(&favs, &boards(vec![game("1", "KC", "TB", 20, 24)]), 0);
        assert_eq!(
            state.check(&favs, &boards(vec![game("1", "KC", "TB", 27, 24)]), 5),
            None,
            "an NBA favorite must not alert on the NFL board"
        );
    }
}
