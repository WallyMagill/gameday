use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{League, StandingsTable};
use crate::provider::map::{map_scoreboard, map_standings, map_stats, map_summary};
use crate::provider::{ProviderError, SportsProvider};
use crate::{Game, GameStats, Summary};

/// Standings freshness window: a cache younger than this is served without
/// touching the network. 10 minutes per the v2 spec ("on demand, cache 10
/// min") — standings move at game granularity, not play granularity.
pub const STANDINGS_TTL: Duration = Duration::from_secs(10 * 60);

pub struct EspnProvider {
    pub cache_dir: PathBuf,
}

impl EspnProvider {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    fn fetch<T, F>(&self, url: &str, key: &str, map: F) -> Result<(T, bool), ProviderError>
    where
        F: FnOnce(&str) -> Result<T, ProviderError>,
    {
        let cached = cache_read(&self.cache_dir, key).ok();
        let http = match http_get(url) {
            Ok(body) => {
                cache_write(&self.cache_dir, key, &body)?;
                Ok(body)
            }
            Err(e) => Err(e),
        };
        http_or_cache(http, cached, map)
    }
}

pub fn scoreboard_url(league: League) -> String {
    let (sport, slug) = league.espn_path();
    let mut u =
        format!("https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{slug}/scoreboard");
    if league == League::Cfb {
        u.push_str("?groups=80");
    }
    u
}

/// Dated scoreboard: the same endpoint with `?dates=YYYYMMDD` (`&` when the
/// base URL already carries a query, i.e. CFB's `?groups=80`).
pub fn scoreboard_on_url(league: League, date: time::Date) -> String {
    let mut u = scoreboard_url(league);
    u.push(if u.contains('?') { '&' } else { '?' });
    u.push_str(&format!(
        "dates={:04}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    ));
    u
}

pub fn summary_url(league: League, event_id: &str) -> String {
    let (sport, slug) = league.espn_path();
    format!(
        "https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{slug}/summary?event={event_id}"
    )
}

/// The `apis/v2` path (NOT `apis/site/v2` like scoreboard/summary) is the one
/// that answers — verified live for NFL and NHL on 2026-08-30; the plan's
/// site/v2 fallback was never needed.
pub fn standings_url(league: League) -> String {
    let (sport, slug) = league.espn_path();
    format!("https://site.web.api.espn.com/apis/v2/sports/{sport}/{slug}/standings")
}

pub fn cache_write(dir: &Path, key: &str, body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(key), body)
}

pub fn cache_read(dir: &Path, key: &str) -> std::io::Result<String> {
    std::fs::read_to_string(dir.join(key))
}

/// Age of a cache entry (time since last write), None when it doesn't exist
/// or the filesystem can't say.
pub fn cache_age(dir: &Path, key: &str) -> Option<Duration> {
    std::fs::metadata(dir.join(key))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
}

/// Guess: unofficial ESPN API has no Retry-After; poll loop uses 5 * 2^attempt, attempt capped at 4.
pub fn backoff_secs(attempt: u32) -> u64 {
    5 * 2u64.pow(attempt.min(4))
}

fn http_get(url: &str) -> Result<String, ProviderError> {
    let resp = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 (gameday/0.1)")
        .set("Accept", "application/json")
        .call();
    match resp {
        Ok(r) => r
            .into_string()
            .map_err(|e| ProviderError::Http(format!("status={} url={url}: {e}", 0))),
        Err(ureq::Error::Status(code, _)) => {
            Err(ProviderError::Http(format!("status={code} url={url}")))
        }
        Err(e) => Err(ProviderError::Http(format!("status=0 url={url}: {e}"))),
    }
}

/// HTTP body or optional disk cache → mapped payload and stale flag.
pub fn http_or_cache<T, F>(
    http: Result<String, ProviderError>,
    cached: Option<String>,
    map: F,
) -> Result<(T, bool), ProviderError>
where
    F: FnOnce(&str) -> Result<T, ProviderError>,
{
    match http {
        Ok(body) => Ok((map(&body)?, false)),
        Err(err) => match cached {
            Some(body) => Ok((map(&body)?, true)),
            None => Err(err),
        },
    }
}

impl SportsProvider for EspnProvider {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        let url = scoreboard_url(league);
        let key = format!("{}-scoreboard", league.slug());
        self.fetch(&url, &key, |body| {
            map_scoreboard(league, body).map_err(Into::into)
        })
    }

    fn scoreboard_on(
        &self,
        league: League,
        date: time::Date,
    ) -> Result<(Vec<Game>, bool), ProviderError> {
        let url = scoreboard_on_url(league, date);
        let key = format!(
            "{}-scoreboard-{:04}{:02}{:02}",
            league.slug(),
            date.year(),
            date.month() as u8,
            date.day()
        );
        self.fetch(&url, &key, |body| {
            map_scoreboard(league, body).map_err(Into::into)
        })
    }

    fn summary(&self, league: League, game_id: &str) -> Result<(Summary, bool), ProviderError> {
        let url = summary_url(league, game_id);
        let key = format!("{}-{game_id}-summary", league.slug());
        self.fetch(&url, &key, |body| map_summary(body).map_err(Into::into))
    }

    fn stats(&self, league: League, game_id: &str) -> Result<(GameStats, bool), ProviderError> {
        // Same endpoint as summary — the boxscore rides on the same payload —
        // but its own cache key: summary and stats poll on different cadences.
        let url = summary_url(league, game_id);
        let key = format!("{}-{game_id}-stats", league.slug());
        self.fetch(&url, &key, |body| map_stats(body).map_err(Into::into))
    }

    fn standings(&self, league: League) -> Result<(StandingsTable, bool), ProviderError> {
        let key = format!("{}-standings", league.slug());
        // A fresh-enough cache short-circuits HTTP entirely (fresh, not
        // stale): the view refetches every time it opens, and standings only
        // move when games end.
        if cache_age(&self.cache_dir, &key).is_some_and(|age| age < STANDINGS_TTL) {
            if let Ok(body) = cache_read(&self.cache_dir, &key) {
                return Ok((map_standings(league, &body)?, false));
            }
        }
        let url = standings_url(league);
        self.fetch(&url, &key, |body| {
            map_standings(league, body).map_err(Into::into)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::League;

    #[test]
    fn scoreboard_url_nfl() {
        assert_eq!(
            scoreboard_url(League::Nfl),
            "https://site.web.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard"
        );
    }

    #[test]
    fn scoreboard_url_cfb_uses_groups() {
        let u = scoreboard_url(League::Cfb);
        assert!(u.contains("college-football/scoreboard"), "{u}");
        assert!(u.contains("groups=80"), "{u}");
    }

    #[test]
    fn dated_scoreboard_url_appends_dates() {
        let d = time::Date::from_calendar_date(2026, time::Month::September, 13).unwrap();
        assert_eq!(
            scoreboard_on_url(League::Nfl, d),
            "https://site.web.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard?dates=20260913"
        );
        // CFB already carries ?groups=80 — dates joins with '&', not a second '?'.
        let u = scoreboard_on_url(League::Cfb, d);
        assert!(u.contains("?groups=80&dates=20260913"), "{u}");
        // Single-digit month/day zero-pad.
        let d2 = time::Date::from_calendar_date(2026, time::Month::January, 5).unwrap();
        assert!(scoreboard_on_url(League::Nba, d2).ends_with("?dates=20260105"));
    }

    #[test]
    fn summary_url_nfl() {
        assert_eq!(
            summary_url(League::Nfl, "401"),
            "https://site.web.api.espn.com/apis/site/v2/sports/football/nfl/summary?event=401"
        );
    }

    #[test]
    fn never_uses_site_api_host() {
        assert!(!scoreboard_url(League::Nfl).contains("site.api.espn.com"));
        let d = time::Date::from_calendar_date(2026, time::Month::September, 13).unwrap();
        assert!(!scoreboard_on_url(League::Nfl, d).contains("site.api.espn.com"));
        assert!(!summary_url(League::Nba, "1").contains("site.api.espn.com"));
        assert!(!standings_url(League::Nhl).contains("site.api.espn.com"));
    }

    #[test]
    fn standings_url_uses_the_apis_v2_path() {
        assert_eq!(
            standings_url(League::Nfl),
            "https://site.web.api.espn.com/apis/v2/sports/football/nfl/standings"
        );
        assert_eq!(
            standings_url(League::Nhl),
            "https://site.web.api.espn.com/apis/v2/sports/hockey/nhl/standings"
        );
    }

    #[test]
    fn fresh_standings_cache_is_served_without_the_network() {
        // A just-written cache entry is inside STANDINGS_TTL, so the provider
        // must answer from disk, fresh (stale=false proves no fetch was
        // attempted — the offline fallback path would mark it stale).
        let dir = std::env::temp_dir().join(format!("gd-standings-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = include_str!("../../fixtures/nfl_standings.json");
        cache_write(&dir, "nfl-standings", fixture).unwrap();
        let provider = EspnProvider::new(dir.clone());
        let (table, stale) = provider.standings(League::Nfl).unwrap();
        assert!(!stale, "fresh cache must not be marked stale");
        assert_eq!(table.groups.len(), 2);
        assert!(cache_age(&dir, "nfl-standings").unwrap() < STANDINGS_TTL);
        assert_eq!(cache_age(&dir, "missing-key"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_and_reads_cache() {
        let dir = std::env::temp_dir().join(format!("gd-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        cache_write(&dir, "nfl-scoreboard", "{\"ok\":1}").unwrap();
        assert!(cache_read(&dir, "nfl-scoreboard").unwrap().contains("ok"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_secs(0), 5);
        assert_eq!(backoff_secs(1), 10);
        assert_eq!(backoff_secs(4), 80);
        assert_eq!(backoff_secs(9), 80);
    }

    fn map_body(s: &str) -> Result<String, ProviderError> {
        Ok(s.to_string())
    }

    #[test]
    fn http_ok_is_fresh() {
        let got = http_or_cache(Ok("live".into()), Some("old".into()), map_body).unwrap();
        assert_eq!(got, ("live".into(), false));
    }

    #[test]
    fn http_err_with_cache_is_stale() {
        let err = ProviderError::Http("status=403 url=x".into());
        let got = http_or_cache(Err(err), Some("cached".into()), map_body).unwrap();
        assert_eq!(got, ("cached".into(), true));
    }

    #[test]
    fn http_err_without_cache_is_err() {
        let err = ProviderError::Http("status=500 url=x".into());
        let got = http_or_cache(Err(err), None, map_body);
        assert!(matches!(got, Err(ProviderError::Http(_))));
    }
}
