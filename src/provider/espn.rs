use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{League, StandingsTable};
use crate::provider::map::{map_scoreboard, map_standings, map_stats, map_summary};
use crate::poll::STANDINGS_TTL;
use crate::provider::{ProviderError, SportsProvider};
use crate::{Game, GameStats, Summary};

/// Guess: ESPN answers in well under a second when it answers at all; 10s is
/// "the socket is dead", not "slow" — long enough to survive a hiccup, short
/// enough that the poll thread doesn't wedge behind one request.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Honest UA: names the program, its version, and where to complain.
pub const USER_AGENT: &str = concat!(
    "gameday/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/WallyMagill/game-day)"
);

pub struct EspnProvider {
    pub cache_dir: PathBuf,
    /// Local UTC offset, read once on the main thread (`text::startup_offset`)
    /// and carried here: the poll thread can't read the TZ database itself.
    pub offset: time::UtcOffset,
    agent: ureq::Agent,
}

/// What one HTTP attempt produced: a body (with the ETag to store beside it),
/// or ESPN saying the cache is already current.
pub(crate) enum Fetched {
    Body { body: String, etag: Option<String> },
    NotModified,
}

/// `http` knows the URL; only `fetch_with` knows which cache key that URL is
/// serving, so it stamps the key on the way out. The URL is left alone.
fn stamp(err: ProviderError, key: &str) -> ProviderError {
    match err {
        ProviderError::Http {
            status,
            url,
            detail,
            ..
        } => ProviderError::Http {
            status,
            key: key.to_string(),
            url,
            detail,
        },
        other => other,
    }
}

impl EspnProvider {
    pub fn new(cache_dir: PathBuf, offset: time::UtcOffset) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(HTTP_TIMEOUT)
            .timeout_read(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build();
        Self {
            cache_dir,
            offset,
            agent,
        }
    }

    fn etag_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.etag"))
    }

    fn http(&self, url: &str, etag: Option<&str>) -> Result<Fetched, ProviderError> {
        let mut req = self.agent.get(url).set("Accept", "application/json");
        if let Some(tag) = etag {
            req = req.set("If-None-Match", tag);
        }
        match req.call() {
            // `key` is left empty here and filled in by `stamp`: this layer
            // only knows the URL it asked for.
            Ok(r) => {
                let etag = r.header("etag").map(str::to_string);
                let body = r.into_string().map_err(|e| ProviderError::Http {
                    status: 0,
                    key: String::new(),
                    url: url.into(),
                    detail: format!("body read: {e}"),
                })?;
                Ok(Fetched::Body { body, etag })
            }
            Err(ureq::Error::Status(304, _)) => Ok(Fetched::NotModified),
            Err(ureq::Error::Status(code, _)) => Err(ProviderError::Http {
                status: code,
                key: String::new(),
                url: url.into(),
                detail: String::new(),
            }),
            Err(e) => Err(ProviderError::Http {
                status: 0,
                key: String::new(),
                url: url.into(),
                detail: e.to_string(),
            }),
        }
    }

    /// Map BEFORE caching; fall back to the cache on transport OR mapping
    /// failure. `stale` is true only when the returned payload is the cached
    /// one because the fresh one failed (a 304 is fresh by definition).
    pub(crate) fn fetch_with<T>(
        &self,
        key: &str,
        http: impl FnOnce(Option<&str>) -> Result<Fetched, ProviderError>,
        map: impl Fn(&str) -> Result<T, ProviderError>,
    ) -> Result<(T, bool), ProviderError> {
        let cached = cache_read(&self.cache_dir, key).ok();
        // An ETag without the body it describes is worse than none: a 304
        // would leave us with nothing to serve.
        let etag = std::fs::read_to_string(self.etag_path(key)).ok();
        let fresh_err = match http(etag.as_deref().filter(|_| cached.is_some())) {
            Ok(Fetched::Body { body, etag }) => match map(&body) {
                Ok(v) => {
                    // Best-effort like the sidecar below: a full disk must not
                    // turn a mapped fresh payload into an error.
                    let _ = cache_write(&self.cache_dir, key, &body);
                    // The sidecar is an optimization, never a result: a write
                    // that fails costs one unconditional refetch later, so it
                    // must not turn a good payload into an error.
                    match etag {
                        Some(t) => {
                            let _ = std::fs::write(self.etag_path(key), t);
                        }
                        None => {
                            let _ = std::fs::remove_file(self.etag_path(key));
                        }
                    }
                    return Ok((v, false));
                }
                Err(e) => e,
            },
            Ok(Fetched::NotModified) => match &cached {
                Some(body) => return map(body).map(|v| (v, false)),
                None => ProviderError::Http {
                    status: 304,
                    key: key.into(),
                    url: String::new(),
                    detail: "304 with no cache".into(),
                },
            },
            Err(e) => stamp(e, key),
        };
        match cached {
            Some(body) => map(&body).map(|v| (v, true)).map_err(|_| fresh_err),
            None => Err(fresh_err),
        }
    }

    fn fetch<T>(
        &self,
        url: &str,
        key: &str,
        map: impl Fn(&str) -> Result<T, ProviderError>,
    ) -> Result<(T, bool), ProviderError> {
        self.fetch_with(key, |etag| self.http(url, etag), map)
    }
}

/// Bare CFB scoreboard (no `dates`) returns the current *week*, not today —
/// that's why a Monday view showed Saturday's ~100 finals under Monday's
/// header. `today` picks the dated form (CFB only) so the header and slate
/// agree. NFL's "today" stays undated: the current week is what an NFL board
/// wants, and NFL's bare scoreboard is never a 100-game pile.
pub fn scoreboard_url_for(league: League, today: Option<time::Date>) -> String {
    let (sport, slug) = league.espn_path();
    let mut u =
        format!("https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{slug}/scoreboard");
    if league == League::Cfb {
        u.push_str("?groups=80");
        // Probed 2026-08-31 against the live endpoint: bare=99 (whole week),
        // dates=<Sat>=8, dates=<Sat>&limit=300=8 — that Saturday didn't hit
        // the cap, but the bare week count (99) shows the cap is real and
        // `limit=300` is cheap insurance against it.
        u.push_str("&limit=300");
        if let Some(d) = today {
            u.push_str(&format!(
                "&dates={:04}{:02}{:02}",
                d.year(),
                d.month() as u8,
                d.day()
            ));
        }
    }
    u
}

pub fn scoreboard_url(league: League) -> String {
    scoreboard_url_for(league, None)
}

/// Dated scoreboard: the same endpoint with `?dates=YYYYMMDD` (`&` when the
/// base URL already carries a query, i.e. CFB's `?groups=80&limit=300`).
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
    let base = format!("https://site.web.api.espn.com/apis/v2/sports/{sport}/{slug}/standings");
    match league {
        // College football has no one league table — you stand teams inside
        // a group. Checked 2026-08-31: `?group=80` returns FBS, 11
        // conferences (Sun Belt nested as two divisions), 138 teams. The bare
        // path happens to default to the same group today; pinning it means
        // the CFB table stays FBS when that default moves.
        League::Cfb => format!("{base}?group=80"),
        _ => base,
    }
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

impl SportsProvider for EspnProvider {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        // CFB's "today" needs the dated URL (bare returns the whole week);
        // NFL's "today" stays undated (the current week is what an NFL board
        // wants). Cache key stays `cfb-scoreboard` either way — it's "today".
        let today = if league == League::Cfb {
            Some(
                time::OffsetDateTime::now_utc()
                    .to_offset(self.offset)
                    .date(),
            )
        } else {
            None
        };
        let url = scoreboard_url_for(league, today);
        let key = format!("{}-scoreboard", league.slug());
        self.fetch(&url, &key, |body| {
            map_scoreboard(league, body, self.offset).map_err(|source| ProviderError::Map {
                key: key.clone(),
                source,
            })
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
            map_scoreboard(league, body, self.offset).map_err(|source| ProviderError::Map {
                key: key.clone(),
                source,
            })
        })
    }

    fn summary(&self, league: League, game_id: &str) -> Result<(Summary, bool), ProviderError> {
        let url = summary_url(league, game_id);
        let key = format!("{}-{game_id}-summary", league.slug());
        self.fetch(&url, &key, |body| {
            map_summary(league, body).map_err(|source| ProviderError::Map {
                key: key.clone(),
                source,
            })
        })
    }

    fn stats(&self, league: League, game_id: &str) -> Result<(GameStats, bool), ProviderError> {
        // Same endpoint as summary — the boxscore rides on the same payload —
        // but its own cache key: summary and stats poll on different cadences.
        let url = summary_url(league, game_id);
        let key = format!("{}-{game_id}-stats", league.slug());
        self.fetch(&url, &key, |body| {
            map_stats(body).map_err(|source| ProviderError::Map {
                key: key.clone(),
                source,
            })
        })
    }

    fn standings(&self, league: League) -> Result<(StandingsTable, bool), ProviderError> {
        let key = format!("{}-standings", league.slug());
        // A fresh-enough cache short-circuits HTTP entirely (fresh, not
        // stale): the view refetches every time it opens, and standings only
        // move when games end.
        if cache_age(&self.cache_dir, &key).is_some_and(|age| age < STANDINGS_TTL) {
            if let Ok(body) = cache_read(&self.cache_dir, &key) {
                if let Ok(table) = map_standings(league, &body) {
                    return Ok((table, false));
                }
            }
        }
        let url = standings_url(league);
        self.fetch(&url, &key, |body| {
            map_standings(league, body).map_err(|source| ProviderError::Map {
                key: key.clone(),
                source,
            })
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
        assert!(u.contains("limit=300"), "{u}");
    }

    #[test]
    fn standings_url_pins_cfb_to_fbs_and_leaves_the_others_bare() {
        let u = standings_url(League::Cfb);
        assert!(u.ends_with("college-football/standings?group=80"), "{u}");
        assert_eq!(
            standings_url(League::Nfl),
            "https://site.web.api.espn.com/apis/v2/sports/football/nfl/standings"
        );
    }

    #[test]
    fn dated_scoreboard_url_appends_dates() {
        let d = time::Date::from_calendar_date(2026, time::Month::September, 13).unwrap();
        assert_eq!(
            scoreboard_on_url(League::Nfl, d),
            "https://site.web.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard?dates=20260913"
        );
        // CFB already carries ?groups=80&limit=300 — dates joins with '&', not a second '?'.
        let u = scoreboard_on_url(League::Cfb, d);
        assert!(u.contains("?groups=80&limit=300&dates=20260913"), "{u}");
        // Single-digit month/day zero-pad.
        let d2 = time::Date::from_calendar_date(2026, time::Month::January, 5).unwrap();
        assert!(scoreboard_on_url(League::Nba, d2).ends_with("?dates=20260105"));
    }

    #[test]
    fn cfb_today_is_dated_and_uncapped() {
        let today = time::Date::from_calendar_date(2026, time::Month::August, 31).unwrap();
        let u = scoreboard_url_for(League::Cfb, Some(today));
        assert!(
            u.contains("groups=80") && u.contains("dates=20260831") && u.contains("limit=300"),
            "{u}"
        );
        assert!(
            !scoreboard_url_for(League::Nfl, Some(today)).contains("dates="),
            "NFL today stays undated (ESPN returns the current week, which is what an NFL board wants)"
        );
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
        let provider = EspnProvider::new(dir.clone(), time::UtcOffset::UTC);
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

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gd-espn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn provider(dir: &Path) -> EspnProvider {
        EspnProvider::new(dir.to_path_buf(), time::UtcOffset::UTC)
    }
    fn ok_map(s: &str) -> Result<String, ProviderError> {
        if s.starts_with('{') {
            Ok(s.to_string())
        } else {
            Err(ProviderError::Map {
                key: "k".into(),
                source: crate::provider::map::MapError::Missing("events"),
            })
        }
    }

    #[test]
    fn a_200_with_a_bad_body_serves_the_cache_and_does_not_poison_it() {
        let dir = tmp("poison");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"good\":1}").unwrap();
        let got = p
            .fetch_with(
                "k",
                |_| {
                    Ok(Fetched::Body {
                        body: "<html>blocked</html>".into(),
                        etag: None,
                    })
                },
                ok_map,
            )
            .unwrap();
        assert_eq!(
            got,
            ("{\"good\":1}".to_string(), true),
            "cached payload, marked stale"
        );
        assert_eq!(
            cache_read(&dir, "k").unwrap(),
            "{\"good\":1}",
            "disk untouched by the bad body"
        );
    }

    #[test]
    fn a_good_body_is_cached_after_it_maps_and_is_fresh() {
        let dir = tmp("fresh");
        let p = provider(&dir);
        let got = p
            .fetch_with(
                "k",
                |_| {
                    Ok(Fetched::Body {
                        body: "{\"v\":2}".into(),
                        etag: Some("\"abc\"".into()),
                    })
                },
                ok_map,
            )
            .unwrap();
        assert_eq!(got, ("{\"v\":2}".to_string(), false));
        assert_eq!(cache_read(&dir, "k").unwrap(), "{\"v\":2}");
        assert_eq!(
            std::fs::read_to_string(dir.join("k.etag")).unwrap(),
            "\"abc\""
        );
    }

    #[test]
    fn not_modified_serves_the_cache_fresh_and_sends_the_stored_etag() {
        let dir = tmp("etag");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"v\":1}").unwrap();
        std::fs::write(dir.join("k.etag"), "\"abc\"").unwrap();
        let mut seen = None;
        let got = p
            .fetch_with(
                "k",
                |etag| {
                    seen = etag.map(str::to_string);
                    Ok(Fetched::NotModified)
                },
                ok_map,
            )
            .unwrap();
        assert_eq!(seen.as_deref(), Some("\"abc\""));
        assert_eq!(
            got,
            ("{\"v\":1}".to_string(), false),
            "304 = the cache IS current"
        );
    }

    #[test]
    fn transport_error_without_cache_is_the_error_with_status_and_url() {
        let dir = tmp("noc");
        let p = provider(&dir);
        let err = p
            .fetch_with(
                "k",
                |_| {
                    Err(ProviderError::Http {
                        status: 403,
                        key: "k".into(),
                        url: "u".into(),
                        detail: String::new(),
                    })
                },
                ok_map,
            )
            .unwrap_err();
        assert!(matches!(err, ProviderError::Http { status: 403, .. }));
        assert_eq!(err.short(), "ESPN 403 k");
        // The key is what a reader sees; the URL still survives for the log.
        assert!(
            err.to_string().contains("url=u"),
            "url must survive stamping: {err}"
        );
    }

    #[test]
    fn a_fresh_body_without_an_etag_removes_a_stale_sidecar() {
        let dir = tmp("drop-etag");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"v\":1}").unwrap();
        std::fs::write(dir.join("k.etag"), "\"old\"").unwrap();
        let got = p
            .fetch_with(
                "k",
                |_| {
                    Ok(Fetched::Body {
                        body: "{\"v\":2}".into(),
                        etag: None,
                    })
                },
                ok_map,
            )
            .unwrap();
        assert_eq!(got, ("{\"v\":2}".to_string(), false));
        assert!(
            !dir.join("k.etag").exists(),
            "an ETag that no longer describes the cached body must not survive"
        );
    }

    #[test]
    fn a_sidecar_without_a_cached_body_is_not_sent() {
        // A 304 answered against a body we don't have would leave nothing to
        // serve, so the orphaned ETag stays home.
        let dir = tmp("orphan-etag");
        let p = provider(&dir);
        std::fs::write(dir.join("k.etag"), "\"abc\"").unwrap();
        let mut seen = Some("sentinel".to_string());
        let got = p
            .fetch_with(
                "k",
                |etag| {
                    seen = etag.map(str::to_string);
                    Ok(Fetched::Body {
                        body: "{\"v\":9}".into(),
                        etag: None,
                    })
                },
                ok_map,
            )
            .unwrap();
        assert_eq!(seen, None, "no cached body means no If-None-Match");
        assert_eq!(got, ("{\"v\":9}".to_string(), false));
    }

    #[test]
    fn user_agent_names_the_project_and_a_contact() {
        assert!(USER_AGENT.starts_with("gameday/"));
        assert!(USER_AGENT.contains("+https://"));
    }
}
