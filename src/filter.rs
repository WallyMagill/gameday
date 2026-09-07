//! The `/` filter: prefix tokens over team words, not a substring over every
//! field. `/ore` once matched Baltimore, Vanderbilt, Eastern Shore and a
//! "Forest" headline (U2); a token now has to start the abbr or a word of
//! the location or name, and every token has to land on one of the two
//! teams. A token that is a league slug scopes the query to that league.

use crate::domain::{Game, League, Team};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Query {
    /// Lowercased tokens; each must prefix-match a word of either team.
    tokens: Vec<String>,
    /// Set by the first token that is a league slug; a second slug stays a
    /// plain token (and matches nothing), which is the honest reading.
    league: Option<League>,
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut q = Query::default();
        for tok in s.split_whitespace() {
            let tok = tok.to_lowercase();
            match (q.league, League::from_slug(&tok)) {
                (None, Some(l)) => q.league = Some(l),
                _ => q.tokens.push(tok),
            }
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.league.is_none()
    }

    pub fn matches(&self, game: &Game) -> bool {
        if self.league.is_some_and(|l| l != game.league) {
            return false;
        }
        self.tokens.iter().all(|t| {
            [&game.away, &game.home]
                .into_iter()
                .any(|team| starts_a_word(team, t))
        })
    }
}

/// A word "starts with" a token either as written or with its punctuation
/// stripped — `/hawaii` has to reach `Hawai'i` and `/oh` has to reach `Miami
/// (OH)`'s `(OH)`. The raw word is tried too so a token that IS punctuation
/// (`/a&m`) still lands.
fn starts_a_word(team: &Team, token: &str) -> bool {
    let bare = |w: &str| -> String { w.chars().filter(|c| c.is_alphanumeric()).collect() };
    team.abbr.to_lowercase().starts_with(token)
        || team
            .location
            .split_whitespace()
            .chain(team.name.split_whitespace())
            .any(|w| {
                let w = w.to_lowercase();
                w.starts_with(token) || bare(&w).starts_with(token)
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Team;

    fn game(league: League, away: (&str, &str, &str), home: (&str, &str, &str)) -> Game {
        let t = |(abbr, location, name): (&str, &str, &str)| Team {
            abbr: abbr.into(),
            location: location.into(),
            name: name.into(),
            ..Default::default()
        };
        Game {
            league,
            away: t(away),
            home: t(home),
            ..Default::default()
        }
    }

    #[test]
    fn a_token_starts_a_word_and_never_sits_inside_one() {
        let q = Query::parse("ore");
        assert!(q.matches(&game(
            League::Cfb,
            ("ORST", "Oregon State", "Beavers"),
            ("BOIS", "Boise State", "Broncos")
        )));
        assert!(
            !q.matches(&game(
                League::Nfl,
                ("BAL", "Baltimore", "Ravens"),
                ("KC", "Kansas City", "Chiefs")
            )),
            "ore inside baltimORE"
        );
        assert!(!q.matches(&game(
            League::Cfb,
            ("WAKE", "Wake Forest", "Demon Deacons"),
            ("VAN", "Vanderbilt", "Commodores")
        )));
    }

    #[test]
    fn a_league_slug_scopes_and_the_rest_must_all_match() {
        let q = Query::parse("nfl kc");
        assert!(q.matches(&game(
            League::Nfl,
            ("KC", "Kansas City", "Chiefs"),
            ("TB", "Tampa Bay", "Buccaneers")
        )));
        assert!(
            !q.matches(&game(
                League::Cbb,
                ("KC", "Kansas City", "Roos"),
                ("UNI", "Northern Iowa", "Panthers")
            )),
            "wrong league"
        );
        let both = Query::parse("kc tb");
        assert!(both.matches(&game(
            League::Nfl,
            ("KC", "Kansas City", "Chiefs"),
            ("TB", "Tampa Bay", "Buccaneers")
        )));
        assert!(
            !both.matches(&game(
                League::Nfl,
                ("KC", "Kansas City", "Chiefs"),
                ("BUF", "Buffalo", "Bills")
            )),
            "every token must land"
        );
    }

    #[test]
    fn an_empty_query_matches_everything_and_case_is_ignored() {
        assert!(Query::parse("").is_empty());
        assert!(Query::parse("  ").matches(&game(
            League::Mlb,
            ("NYY", "New York", "Yankees"),
            ("BOS", "Boston", "Red Sox")
        )));
        assert!(Query::parse("RED").matches(&game(
            League::Mlb,
            ("NYY", "New York", "Yankees"),
            ("BOS", "Boston", "Red Sox")
        )));
    }

    #[test]
    fn a_word_matches_past_its_own_punctuation() {
        // `/hawaii` has to reach `Hawai'i`; the apostrophe is not a letter
        // the typed query can spell.
        let q = Query::parse("hawaii");
        assert!(q.matches(&game(
            League::Cbb,
            ("HAW", "Hawai'i", "Rainbow Warriors"),
            ("BOIS", "Boise State", "Broncos")
        )));
        // `/oh` has to reach `Miami (OH)` — the parenthesized disambiguator
        // is its own word, and the punctuation around it is not a letter
        // either. The abbr itself (`M-OH`) does not start with "oh", so this
        // only passes through the location word.
        let q = Query::parse("oh");
        assert!(q.matches(&game(
            League::Cbb,
            ("M-OH", "Miami (OH)", "RedHawks"),
            ("BOIS", "Boise State", "Broncos")
        )));
    }

    #[test]
    fn a_bare_league_slug_scopes_every_game_in_it_and_only_it() {
        let q = Query::parse("nfl");
        assert!(!q.is_empty());
        assert!(q.matches(&game(
            League::Nfl,
            ("KC", "Kansas City", "Chiefs"),
            ("TB", "Tampa Bay", "Buccaneers")
        )));
        assert!(
            !q.matches(&game(
                League::Cfb,
                ("KC", "Kansas City", "Chiefs"),
                ("TB", "Tampa Bay", "Buccaneers")
            )),
            "the slug scopes to its own league even with identical team words"
        );
    }

    #[test]
    fn a_league_slug_reads_case_insensitively_like_every_other_token() {
        let q = Query::parse("NFL kc");
        assert!(q.matches(&game(
            League::Nfl,
            ("KC", "Kansas City", "Chiefs"),
            ("TB", "Tampa Bay", "Buccaneers")
        )));
    }
}
