use std::path::{Path, PathBuf};

use crate::domain::League;
use crate::provider::map::{map_scoreboard, map_summary};
use crate::provider::{ProviderError, SportsProvider};
use crate::{Game, Summary};

pub struct EspnProvider {
    pub cache_dir: PathBuf,
}

impl EspnProvider {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }
}

pub fn scoreboard_url(league: League) -> String {
    let (sport, slug) = league.espn_path();
    let mut u = format!("https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{slug}/scoreboard");
    if league == League::Cfb {
        u.push_str("?groups=80");
    }
    u
}

pub fn summary_url(league: League, event_id: &str) -> String {
    let (sport, slug) = league.espn_path();
    format!("https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{slug}/summary?event={event_id}")
}

pub fn cache_write(dir: &Path, key: &str, body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(key), body)
}

pub fn cache_read(dir: &Path, key: &str) -> std::io::Result<String> {
    std::fs::read_to_string(dir.join(key))
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

impl SportsProvider for EspnProvider {
    fn scoreboard(&self, league: League) -> Result<Vec<Game>, ProviderError> {
        let url = scoreboard_url(league);
        let key = format!("{}-scoreboard", league.slug());
        match http_get(&url) {
            Ok(body) => {
                cache_write(&self.cache_dir, &key, &body)?;
                Ok(map_scoreboard(league, &body)?)
            }
            Err(e) => {
                if let Ok(cached) = cache_read(&self.cache_dir, &key) {
                    return Ok(map_scoreboard(league, &cached)?);
                }
                Err(e)
            }
        }
    }

    fn summary(&self, league: League, game_id: &str) -> Result<Summary, ProviderError> {
        let url = summary_url(league, game_id);
        let key = format!("{}-{game_id}-summary", league.slug());
        match http_get(&url) {
            Ok(body) => {
                cache_write(&self.cache_dir, &key, &body)?;
                Ok(map_summary(&body)?)
            }
            Err(e) => {
                if let Ok(cached) = cache_read(&self.cache_dir, &key) {
                    return Ok(map_summary(&cached)?);
                }
                Err(e)
            }
        }
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
    fn summary_url_nfl() {
        assert_eq!(
            summary_url(League::Nfl, "401"),
            "https://site.web.api.espn.com/apis/site/v2/sports/football/nfl/summary?event=401"
        );
    }

    #[test]
    fn never_uses_site_api_host() {
        assert!(!scoreboard_url(League::Nfl).contains("site.api.espn.com"));
        assert!(!summary_url(League::Nba, "1").contains("site.api.espn.com"));
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
}
