use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::domain::{League, StandingsTable};
use crate::poll::STANDINGS_TTL;
use crate::provider::map::{map_scoreboard, map_standings, map_stats, map_summary, served_day};
use crate::provider::{ProviderError, Refusal, SportsProvider};
use crate::{Game, GameStats, Summary};

/// Guess: ESPN answers in well under a second when it answers at all; 10s is
/// "the socket is dead", not "slow" — long enough to survive a hiccup, short
/// enough that the poll thread doesn't wedge behind one request.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest response body accepted, counted AFTER gzip decoding (ureq's own
/// limit sits under its decoder, so 65 KB of gzip once decoded to 64 MiB
/// unchecked). Receipt: the largest real payload in the owner's cache is
/// 1,057,405 bytes (an MLB summary, 2026-11); 16 MiB is about 15 times that.
pub const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

/// Read at most `cap` decoded bytes from `reader` (the body of `url`). More
/// than `cap` is [`Refusal::TooLarge`]; nothing past `cap + 1` bytes is ever
/// buffered. Bytes are read, not chars, so the cut can never split a UTF-8
/// sequence into a read error; ureq's lossy decoder already made them UTF-8.
/// `key` is left empty for `stamp` to fill, as in `http`.
fn read_capped(reader: impl std::io::Read, cap: u64, url: &str) -> Result<String, ProviderError> {
    use std::io::Read;
    let mut buf = Vec::new();
    reader
        .take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| ProviderError::Http {
            status: 0,
            key: String::new(),
            url: url.into(),
            detail: format!("body read: {e}"),
        })?;
    if buf.len() as u64 > cap {
        return Err(ProviderError::Refused {
            key: String::new(),
            url: url.into(),
            why: Refusal::TooLarge { cap },
        });
    }
    Ok(String::from_utf8(buf)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Honest UA: names the program, its version, and where to complain.
pub const USER_AGENT: &str = concat!(
    "gameday/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/WallyMagill/gameday)"
);

pub struct EspnProvider {
    pub cache_dir: PathBuf,
    /// Local UTC offset, read once on the main thread (`text::startup_offset`)
    /// and carried here: the poll thread can't read the TZ database itself.
    pub offset: time::UtcOffset,
    agent: ureq::Agent,
    /// [`MAX_BODY_BYTES`]; a field only so a test can shrink it.
    max_body: u64,
    /// The latest local date on which each league's bare board served that
    /// date (`day.date` == today): from then on the league is not lagging
    /// for that date, whatever a later bare body says. A guard for a case
    /// not observed: a CDN serving the rolled and unrolled bare board on
    /// alternating polls during ESPN's rollover would otherwise move a live
    /// game between the fresh bare copy and the up-to-a-minute-old dated
    /// copy, so its score could go backwards (a flash, a catch-up request,
    /// a favorite's banner and bell; see `App::apply_boards` in
    /// `app/merge.rs`). Cheap: one date per league, and a lock held only for
    /// one lookup. `--once` is a new process per run and notifies nobody, so
    /// it never needs it (it runs there too, harmlessly).
    rolled: std::sync::Mutex<std::collections::HashMap<League, time::Date>>,
}

/// What one HTTP attempt produced: a body (with the ETag to store beside it),
/// or ESPN saying the cache is already current.
#[derive(Debug, PartialEq)]
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
        ProviderError::Refused { url, why, .. } => ProviderError::Refused {
            key: key.to_string(),
            url,
            why,
        },
        other => other,
    }
}

impl EspnProvider {
    pub fn new(cache_dir: PathBuf, offset: time::UtcOffset) -> Self {
        Self::with_agent(cache_dir, offset, HTTP_TIMEOUT, true)
    }

    /// Split out so tests can shrink the bound (real timeouts would make a
    /// stalled-server test take as long as the timeout itself) and talk to a
    /// local plain-HTTP server (`https_only: false`). Production always
    /// passes `true`: every ESPN URL is https, and ureq's default follows a
    /// redirect from https down to http.
    fn with_agent(
        cache_dir: PathBuf,
        offset: time::UtcOffset,
        timeout: Duration,
        https_only: bool,
    ) -> Self {
        let config = ureq::Agent::config_builder()
            // Three timeouts stand in for ureq 2's connect+read pair:
            // connect (the TCP handshake), recv_response (headers — a
            // server that accepts and then never answers would otherwise
            // block forever, since ureq 3 leaves this unbounded by
            // default), and recv_body (the response body). ureq 2's single
            // `timeout_read` covered both headers and body; ureq 3 splits
            // them, so both must be set to keep the same bound.
            //
            // Resolve (DNS) is a fourth phase ureq 3 leaves unbounded; a
            // stalled lookup would hold `--once` past its documented 10 s.
            .timeout_resolve(Some(timeout))
            .timeout_connect(Some(timeout))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .user_agent(USER_AGENT)
            .https_only(https_only)
            // 4xx/5xx surface as `Error::StatusCode(code)`. 304 is neither —
            // ureq 3's `is_client_error() || is_server_error()` check never
            // classifies a 3xx as an error regardless of this flag — so it
            // always arrives as an `Ok` response, and the `Ok` arm below
            // checks the status itself before reading the body.
            .http_status_as_error(true)
            // The proxy variables from the environment, in ureq's order
            // (SECURITY.md lists them; HTTP_PROXY applies to https too). This
            // is what makes `HTTPS_PROXY=http://127.0.0.1:9 gameday` the
            // offline test.
            .proxy(ureq::Proxy::try_from_env())
            .build();
        Self {
            cache_dir,
            offset,
            agent: ureq::Agent::new_with_config(config),
            max_body: MAX_BODY_BYTES,
            rolled: Default::default(),
        }
    }

    fn etag_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.etag"))
    }

    fn http(&self, url: &str, etag: Option<&str>) -> Result<Fetched, ProviderError> {
        let mut req = self.agent.get(url).header("Accept", "application/json");
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        match req.call() {
            // ureq 3 only classifies 4xx/5xx as `Error::StatusCode`; a 304
            // is a plain `Ok` response, so it must be caught here — before
            // the body read below, since a 304 body is empty and would
            // otherwise map to a bogus `Fetched::Body`.
            Ok(r) if r.status().as_u16() == 304 => Ok(Fetched::NotModified),
            // `key` is left empty here and filled in by `stamp`: this layer
            // only knows the URL it asked for.
            Ok(mut r) => {
                let etag = r
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                // `read_capped` is the one bound, on the decoded bytes, so it
                // holds whichever side of ureq's decoder ureq's own `limit`
                // sits on (under it through 3.5.0, after it from 3.5.2).
                // Wire bytes that decode to nothing cost time, not memory,
                // and `timeout_recv_body` bounds the time. `lossy_utf8` keeps
                // what `read_to_string` did with bad UTF-8.
                let reader = r.body_mut().with_config().lossy_utf8(true).reader();
                let body = read_capped(reader, self.max_body, url)?;
                Ok(Fetched::Body { body, etag })
            }
            Err(ureq::Error::StatusCode(code)) => Err(ProviderError::Http {
                status: code,
                key: String::new(),
                url: url.into(),
                detail: String::new(),
            }),
            // `https_only` refused a hop: every ESPN URL is https, so in
            // production this is a redirect down to http.
            Err(ureq::Error::RequireHttpsOnly(to)) => Err(ProviderError::Refused {
                key: String::new(),
                url: url.into(),
                why: Refusal::NotHttps { to },
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
                            let _ = crate::atomic::write(&self.etag_path(key), t);
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
                Some(body) => {
                    let v = map(body)?;
                    // A 304 says the cached body is current as of now, so its
                    // age restarts: `cached_within`'s gate and standings' TTL
                    // read the file's mtime, and without this a 304 would be
                    // asked again on every call once the file is old.
                    let _ = cache_touch(&self.cache_dir, key);
                    return Ok((v, false));
                }
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

    /// The gate `fetch_with` doesn't have on its own: a cache entry younger
    /// than `max_age` is served as fresh without calling `http` at all — a
    /// status bar polling every 30 s must not cost more than the TUI's own
    /// cadence. Anything older (or unmapped) falls through to `fetch_with`,
    /// which still falls back to the cache, marked stale, when `http` fails.
    /// Split out from `scoreboard_cached_within` so a test can supply a
    /// counting `http` closure instead of a real host.
    pub(crate) fn cached_within<T>(
        &self,
        key: &str,
        max_age: Duration,
        http: impl FnOnce(Option<&str>) -> Result<Fetched, ProviderError>,
        map: impl Fn(&str) -> Result<T, ProviderError>,
    ) -> Result<(T, bool), ProviderError> {
        if cache_age(&self.cache_dir, key).is_some_and(|age| age < max_age) {
            if let Ok(body) = cache_read(&self.cache_dir, key) {
                if let Ok(v) = map(&body) {
                    return Ok((v, false));
                }
            }
        }
        self.fetch_with(key, http, map)
    }

    fn local_today(&self) -> time::Date {
        time::OffsetDateTime::now_utc()
            .to_offset(self.offset)
            .date()
    }

    /// `--once`'s scoreboard: a cache entry younger than `max_age` is served
    /// as fresh without touching the network; anything older goes through
    /// the same transport `scoreboard` uses, falling back to the cache,
    /// marked stale, when the fetch fails. A failed morning fetch (see
    /// `scoreboard_today`) is not logged: `--once` keeps gameday.log clean
    /// (see `once::fetch`).
    pub fn scoreboard_cached_within(
        &self,
        league: League,
        max_age: Duration,
    ) -> Result<(Vec<Game>, bool), ProviderError> {
        self.scoreboard_today(
            league,
            self.local_today(),
            max_age,
            |url, etag| self.http(url, etag),
            |_| {},
        )
    }

    /// The league's board for `today` (the local date): the bare scoreboard
    /// (`max_age` gates it as in `cached_within`; `Duration::ZERO` always
    /// asks), plus, when ESPN's day has not rolled over yet, the dated board
    /// for `today` merged in by event id.
    ///
    /// The morning gap: ESPN's bare board serves its own "current day", which
    /// on 2026-11-13 still said 2026-11-12 at 10:15 ET and had rolled by
    /// 11:46 ET. Until it rolls, tonight's games are on no other screen. So
    /// when the bare body's `day.date` is exactly the day before `today`,
    /// the dated board for `today` (`<league>-scoreboard-YYYYMMDD`, the key
    /// date travel uses) is asked for at most once per [`LAGGING_DAY_EVERY`]
    /// whatever the outcome: its cache entry's age is the gate and every
    /// attempt stamps it, so the bound holds across `--once` processes too.
    /// An empty slate is asked for once per date, and a league whose bare
    /// board has served `today` in this process never lags on that date
    /// again. A failed dated fetch never touches the bare result: the board,
    /// its staleness flag and the footer are what they were without it; a
    /// cached dated body for `today` is used when there is one, and
    /// `dated_failed` is handed the error when there is none.
    ///
    /// `http(url, etag)` and `today` are parameters so a test can stand in
    /// for ESPN and the clock.
    pub(crate) fn scoreboard_today(
        &self,
        league: League,
        today: time::Date,
        max_age: Duration,
        http: impl Fn(&str, Option<&str>) -> Result<Fetched, ProviderError>,
        dated_failed: impl FnOnce(&ProviderError),
    ) -> Result<(Vec<Game>, bool), ProviderError> {
        // CFB is dated to the local date already (see `scoreboard_url_for`).
        let url = scoreboard_url_for(league, (league == League::Cfb).then_some(today));
        let key = format!("{}-scoreboard", league.slug());
        let ((games, served), stale) = self.cached_within(
            &key,
            max_age,
            |etag| http(&url, etag),
            |body| {
                let games = map_scoreboard(league, body, self.offset).map_err(|source| {
                    ProviderError::Map {
                        key: key.clone(),
                        source,
                    }
                })?;
                Ok((games, served_day(body)))
            },
        )?;
        let rolled = {
            let mut rolled = self.rolled.lock().unwrap_or_else(|e| e.into_inner());
            if served == Some(today) {
                rolled.insert(league, today);
            }
            rolled.get(&league) == Some(&today)
        };
        // NFL's bare board is the current week and CFB's is already dated:
        // neither ever lags a day.
        let lagging = !matches!(league, League::Nfl | League::Cfb)
            && !rolled
            && served.is_some_and(|d| today.previous_day() == Some(d));
        if !lagging {
            return Ok((games, stale));
        }
        let dated_url = scoreboard_on_url(league, today);
        let dated_key = dated_key(league, today);
        let map_dated = |body: &str| {
            map_scoreboard(league, body, self.offset).map_err(|source| ProviderError::Map {
                key: dated_key.clone(),
                source,
            })
        };
        let cached = cache_read(&self.cache_dir, &dated_key)
            .ok()
            .and_then(|body| map_dated(&body).ok());
        // A slate for today that ESPN answered empty (`events: []`, not a
        // body where nothing mapped: that is an error and never cached) is
        // not asked for again on this date; the key is per date, so the
        // next local day asks afresh. Given up: a game added to today's
        // slate after the empty answer is not seen until ESPN's bare board
        // rolls over or the cache entry is pruned.
        if cached.as_ref().is_some_and(Vec::is_empty) {
            return Ok((games, stale));
        }
        let young =
            cache_age(&self.cache_dir, &dated_key).is_some_and(|age| age < LAGGING_DAY_EVERY);
        let tonight = if young {
            // Inside the window nothing is asked: the cached slate when
            // there is one, else (an earlier attempt failed) the bare board
            // alone.
            cached
        } else {
            let got = self.fetch_with(&dated_key, |etag| http(&dated_url, etag), map_dated);
            // Every attempt counts against the window, whatever its outcome:
            // the file's mtime is the gate, so stamp it now, creating an
            // empty stub when the attempt failed with nothing cached.
            // Without this a failing dated URL is asked on every 15 s poll
            // and every `--once` run. The stub is safe: `prune_cache` knows
            // the name, it maps to nothing (so it serves as "no cache"), and
            // it has no `.etag` sidecar. A failed stamp costs only the gate.
            let _ = cache_touch(&self.cache_dir, &dated_key);
            match got {
                Ok((tonight, _)) => Some(tonight),
                Err(e) => {
                    dated_failed(&e);
                    None
                }
            }
        };
        let Some(tonight) = tonight else {
            return Ok((games, stale));
        };
        // The bare copy wins: it is the league's board everywhere else in
        // the app, and it is fetched every poll while the dated copy can be
        // up to LAGGING_DAY_EVERY older.
        let mut games = games;
        for g in tonight {
            if !games.iter().any(|b| b.id == g.id) {
                games.push(g);
            }
        }
        Ok((games, stale))
    }
}

/// How often a lagging league's dated board for the local date may be
/// asked for (see `EspnProvider::scoreboard_today`): the app's idle
/// scoreboard interval, [`crate::poll::SCOREBOARD_IDLE`], so a lagging
/// league costs at most one extra request a minute, whether the attempt
/// succeeds or fails and whether the board polls every 15 s, every 60 s, or
/// a status bar runs `--once` every 2 s. A league is lagging while its bare
/// board's `day.date` is the local date minus one and today's slate for it
/// is not known to be empty; an empty slate costs one request for the date.
/// Known limit, accepted: a game from tonight's slate that goes live before
/// ESPN rolls its day over updates every 60 s, not 15 s, until the rollover
/// puts it on the bare board.
pub const LAGGING_DAY_EVERY: Duration = crate::poll::SCOREBOARD_IDLE;

/// Cache key of a dated scoreboard: `<league>-scoreboard-YYYYMMDD`.
fn dated_key(league: League, date: time::Date) -> String {
    format!(
        "{}-scoreboard-{:04}{:02}{:02}",
        league.slug(),
        date.year(),
        date.month() as u8,
        date.day()
    )
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

/// Standings live under `apis/v2` on the same host, where `scoreboard_url` and
/// `summary_url` both use `apis/site/v2`. That one path segment is the whole
/// difference, and it is the one that answers: verified live for NFL and NHL
/// on 2026-08-30, and the `apis/site/v2` fallback was never needed.
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
    crate::atomic::write(&dir.join(key), body)
}

/// Set a cache entry's mtime to now, creating it empty when it does not
/// exist: its bytes are never truncated or rewritten, so this cannot leave a
/// partial file (the property [`crate::atomic::write`] keeps for writes).
/// The mtime is what [`cache_age`] reads.
fn cache_touch(dir: &Path, key: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(key))?
        .set_modified(SystemTime::now())
}

/// How old (since last write) a per-game or per-date cache entry may get
/// before startup removes it. A guess, chosen wide: a pinned game drops 6
/// hours after its final, date travel reaches 7 days from today, and the
/// cache only serves a body when the fresh fetch fails, so an entry nothing
/// has rewritten in two weeks is not on screen; pruning one by mistake costs
/// one refetch.
pub const CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The entries that pile up, one per game or per date: `<league>-<id>-summary`,
/// `<league>-<id>-stats`, `<league>-scoreboard-YYYYMMDD`, and their `.etag`
/// sidecars; and the temp file a killed write left behind
/// (`.<entry>.<pid>-<seq>.tmp`, see [`crate::atomic::write`]) for any entry the
/// cache writes. Never the bare `<league>-scoreboard` (the last good payload
/// STALE serves) or `<league>-standings`, one file per league, and never a
/// name it does not recognize.
fn is_accumulating(name: &str) -> bool {
    match crate::atomic::leftover_target(name) {
        Some(entry) => cache_entry(entry).is_some(),
        None => cache_entry(name) == Some(true),
    }
}

/// `Some(true)` for a per-game or per-date entry (or its `.etag`),
/// `Some(false)` for a per-league one, None for a name the cache never writes.
fn cache_entry(name: &str) -> Option<bool> {
    let name = name.strip_suffix(".etag").unwrap_or(name);
    let (slug, rest) = name.split_once('-')?;
    League::from_slug(slug)?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if rest == "scoreboard" || rest == "standings" {
        return Some(false);
    }
    if let Some(date) = rest.strip_prefix("scoreboard-") {
        return (date.len() == 8 && digits(date)).then_some(true);
    }
    match rest.rsplit_once('-') {
        Some((id, "summary" | "stats")) => digits(id).then_some(true),
        _ => None,
    }
}

/// Remove accumulating entries (see [`is_accumulating`]) in `dir` last
/// written more than `max_age` before `now`. Returns how many were removed.
/// A missing dir is nothing to prune; a failure is returned for the caller
/// to log, naming the first file that could not be removed.
pub fn prune_cache(dir: &Path, now: SystemTime, max_age: Duration) -> Result<usize, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("read {}: {e}", dir.display())),
    };
    let mut removed = 0;
    let mut failed: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_accumulating(name) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if !old {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(e) => failed.push(format!("{}: {e}", entry.path().display())),
        }
    }
    match failed.first() {
        None => Ok(removed),
        Some(first) => Err(format!(
            "removed {removed}, {} failed (first: {first})",
            failed.len()
        )),
    }
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
        self.scoreboard_today(
            league,
            self.local_today(),
            Duration::ZERO,
            |url, etag| self.http(url, etag),
            // To gameday.log only: the board itself is fine without the
            // dated copy, so the footer has nothing to say.
            |e| {
                let _ = e.short_logged();
            },
        )
    }

    fn scoreboard_on(
        &self,
        league: League,
        date: time::Date,
    ) -> Result<(Vec<Game>, bool), ProviderError> {
        let url = scoreboard_on_url(league, date);
        let key = dated_key(league, date);
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

    /// `--once`'s cache gate, hermetic: `scoreboard`'s URL is baked in, so a
    /// local listener can't intercept it — `cached_within` is the split-out
    /// gate underneath it, and a counting closure stands in for `http` here
    /// instead of a real host or a real timeout.
    #[test]
    fn a_young_scoreboard_cache_is_served_without_the_network() {
        use std::cell::Cell;
        let dir = tmp("once-cache");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"good\":1}").unwrap();
        let calls = Cell::new(0);
        let refused = |_: Option<&str>| {
            calls.set(calls.get() + 1);
            Err(ProviderError::Http {
                status: 0,
                key: String::new(),
                url: String::new(),
                detail: "refused".into(),
            })
        };
        let (_, stale) = p
            .cached_within("k", Duration::from_secs(60), refused, ok_map)
            .unwrap();
        assert_eq!(calls.get(), 0, "young cache: no HTTP attempted");
        assert!(!stale, "young cache: served fresh");

        let (_, stale) = p
            .cached_within("k", Duration::ZERO, refused, ok_map)
            .unwrap();
        assert_eq!(calls.get(), 1, "old cache: HTTP attempted exactly once");
        assert!(
            stale,
            "old cache: HTTP attempted and failed, cache served stale"
        );
    }

    /// The morning gap, from real bodies (`fixtures/PROVENANCE-2026-11-13-morning.md`):
    /// at 10:08 ET on 2026-11-13 ESPN's bare NBA board still served
    /// 2026-11-12. `LAST_NIGHT` is that slate (one final) and `TONIGHT` is
    /// `?dates=20261113` (two of its nine games). A dated body carries no
    /// `day`, so `bare_on` adds the `day` a bare body has.
    const LAST_NIGHT: &str = include_str!("../../fixtures/nba_scoreboard_2026-11-12.json");
    const TONIGHT: &str = include_str!("../../fixtures/nba_scoreboard_2026-11-13.json");
    const TODAY: time::Date = time::macros::date!(2026 - 11 - 13);

    fn bare_on(body: &str, day: Option<&str>) -> String {
        let mut v: serde_json::Value = serde_json::from_str(body).unwrap();
        if let Some(day) = day {
            v["day"] = serde_json::json!({ "date": day });
        }
        v.to_string()
    }

    fn ids(json: &str) -> Vec<String> {
        map_scoreboard(League::Nba, json, time::UtcOffset::UTC)
            .unwrap()
            .into_iter()
            .map(|g| g.id)
            .collect()
    }

    /// ESPN as a closure: the bare URL answers `bare` (a test may swap it),
    /// the dated URL for TODAY answers `dated` (None = a 403). Counts the
    /// requests to each.
    struct Espn {
        bare: std::cell::RefCell<String>,
        dated: Option<String>,
        bare_calls: std::cell::Cell<u32>,
        dated_calls: std::cell::Cell<u32>,
    }

    impl Espn {
        fn new(bare: String, dated: Option<&str>) -> Self {
            Self {
                bare: bare.into(),
                dated: dated.map(str::to_string),
                bare_calls: std::cell::Cell::new(0),
                dated_calls: std::cell::Cell::new(0),
            }
        }
        fn http(&self, url: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            let body = if url.ends_with("/scoreboard") {
                self.bare_calls.set(self.bare_calls.get() + 1);
                Some(self.bare.borrow().clone())
            } else {
                assert!(url.ends_with("?dates=20261113"), "unexpected url {url}");
                self.dated_calls.set(self.dated_calls.get() + 1);
                self.dated.clone()
            };
            body.map(|body| Fetched::Body { body, etag: None })
                .ok_or_else(|| ProviderError::Http {
                    status: 403,
                    key: String::new(),
                    url: url.into(),
                    detail: String::new(),
                })
        }
        fn board(&self, p: &EspnProvider, max_age: Duration) -> (Vec<Game>, bool) {
            p.scoreboard_today(
                League::Nba,
                TODAY,
                max_age,
                |u, e| self.http(u, e),
                |e| panic!("dated fetch failed: {e}"),
            )
            .unwrap()
        }
        /// `board` for a dated URL that is expected to fail.
        fn quiet(&self, p: &EspnProvider, max_age: Duration) -> (Vec<Game>, bool) {
            p.scoreboard_today(League::Nba, TODAY, max_age, |u, e| self.http(u, e), |_| {})
                .unwrap()
        }
    }

    /// Make a cache entry `by` older than now, as if written then.
    fn backdate(dir: &Path, key: &str, by: Duration) {
        std::fs::File::options()
            .write(true)
            .open(dir.join(key))
            .unwrap()
            .set_modified(SystemTime::now() - by)
            .unwrap();
    }

    /// A window and a second: old enough for the gate to open.
    const PAST_WINDOW: Duration = LAGGING_DAY_EVERY.saturating_add(Duration::from_secs(1));

    #[test]
    fn a_board_still_on_yesterday_also_shows_tonights_games() {
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), Some(TONIGHT));
        let p = EspnProvider::with_agent(
            tmp("morning"),
            time::macros::offset!(-5),
            HTTP_TIMEOUT,
            false,
        );
        let (games, stale) = espn.board(&p, Duration::ZERO);
        assert!(!stale);
        let got: Vec<String> = games.iter().map(|g| g.id.clone()).collect();
        let want: Vec<String> = [ids(LAST_NIGHT), ids(TONIGHT)].concat();
        assert_eq!(got, want, "last night's final, then tonight's two");
        assert_eq!(espn.dated_calls.get(), 1);

        // What the user sees at 10:08 ET: the NBA tab and Home both carry
        // tonight's games.
        let dir = tmp("morning-app");
        let mut app = crate::app::App::new(
            crate::config::Config::default_all(),
            vec![],
            dir,
            time::macros::offset!(-5),
        );
        app.now_override = Some(time::macros::datetime!(2026-11-13 10:08 -5));
        app.apply_boards(League::Nba, games, stale);
        let tonight = ids(TONIGHT);
        for tab in [crate::app::Tab::League(League::Nba), crate::app::Tab::Home] {
            app.tab = tab;
            let shown: Vec<String> = app.visible_games().into_iter().map(|g| g.id).collect();
            assert!(
                tonight.iter().all(|id| shown.contains(id)),
                "{tab:?} shows {shown:?}, expected tonight's {tonight:?} among them"
            );
        }
    }

    #[test]
    fn a_board_that_is_not_one_day_behind_asks_for_nothing_more() {
        let p = provider(&tmp("not-lagging"));
        let lagging = bare_on(LAST_NIGHT, Some("2026-11-12"));
        // ESPN already on today, two days behind, ahead, or no `day` at all.
        for day in [
            Some("2026-11-13"),
            Some("2026-11-11"),
            Some("2026-11-14"),
            None,
        ] {
            let espn = Espn::new(bare_on(LAST_NIGHT, day), Some(TONIGHT));
            let (games, _) = espn.board(&p, Duration::ZERO);
            assert_eq!(espn.dated_calls.get(), 0, "day {day:?}");
            assert_eq!(games.len(), 1, "day {day:?}");
        }
        // NFL's week board and CFB's dated board never lag a day, whatever
        // their body says: one request, the board's own.
        for league in [League::Nfl, League::Cfb] {
            let calls = std::cell::Cell::new(0);
            let _ = p.scoreboard_today(
                league,
                TODAY,
                Duration::ZERO,
                |_, _| {
                    calls.set(calls.get() + 1);
                    Ok(Fetched::Body {
                        body: lagging.clone(),
                        etag: None,
                    })
                },
                |_| {},
            );
            assert_eq!(calls.get(), 1, "{league:?}");
        }
    }

    #[test]
    fn a_failed_dated_fetch_leaves_the_bare_board_fresh_and_whole() {
        let dir = tmp("dated-403");
        let p = provider(&dir);
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), None);
        let mut failed = Vec::new();
        let (games, stale) = p
            .scoreboard_today(
                League::Nba,
                TODAY,
                Duration::ZERO,
                |u, e| espn.http(u, e),
                |e| failed.push(e.short()),
            )
            .unwrap();
        assert!(!stale, "the bare board answered: not STALE");
        assert_eq!(
            games.iter().map(|g| &g.id).collect::<Vec<_>>(),
            ids(LAST_NIGHT).iter().collect::<Vec<_>>()
        );
        assert_eq!(failed, ["ESPN 403 nba scoreboard 20261113"]);
        assert_eq!(espn.dated_calls.get(), 1);

        // The next poll, inside the window, with nothing cached: the failed
        // attempt counts, so nothing is asked and the bare board stands alone.
        let (games, stale) = espn.quiet(&p, Duration::ZERO);
        assert!(!stale);
        assert_eq!(games.len(), 1);
        assert_eq!(
            espn.dated_calls.get(),
            1,
            "a failure with nothing cached waits the window too"
        );

        // With tonight's slate cached from an earlier fetch, a failure
        // serves that copy, and the attempt counts against the window.
        let key = "nba-scoreboard-20261113";
        cache_write(&dir, key, TONIGHT).unwrap();
        backdate(&dir, key, PAST_WINDOW);
        let (games, stale) = espn.board(&p, Duration::ZERO);
        assert!(!stale);
        assert_eq!(games.len(), 3, "last night's one and the cached two");
        assert_eq!(espn.dated_calls.get(), 2);
        espn.board(&p, Duration::ZERO);
        assert_eq!(espn.dated_calls.get(), 2, "a failure waits the window too");
    }

    #[test]
    fn a_game_in_both_bodies_appears_once_as_the_bare_board_has_it() {
        let tonight: serde_json::Value = serde_json::from_str(TONIGHT).unwrap();
        let mut shared = tonight["events"][0].clone();
        // Moved start: tells the bare copy from the dated one.
        shared["date"] = "2026-11-14T00:30Z".into();
        let mut bare: serde_json::Value =
            serde_json::from_str(&bare_on(LAST_NIGHT, Some("2026-11-12"))).unwrap();
        bare["events"].as_array_mut().unwrap().push(shared);
        let espn = Espn::new(bare.to_string(), Some(TONIGHT));
        let (games, _) = espn.board(&provider(&tmp("both")), Duration::ZERO);
        let id = &ids(TONIGHT)[0];
        let copies: Vec<&Game> = games.iter().filter(|g| &g.id == id).collect();
        assert_eq!(copies.len(), 1, "{id} once");
        assert_eq!(
            copies[0].start,
            Some(time::macros::datetime!(2026-11-14 00:30 UTC)),
            "the bare board's copy"
        );
        assert_eq!(games.len(), 3);
    }

    #[test]
    fn the_dated_board_is_asked_for_at_most_once_a_window() {
        let dir = tmp("dated-window");
        let p = provider(&dir);
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), Some(TONIGHT));
        // The live board's 15 s polls, back to back: one dated request.
        for _ in 0..4 {
            let (games, _) = espn.board(&p, Duration::ZERO);
            assert_eq!(games.len(), 3);
        }
        assert_eq!(espn.dated_calls.get(), 1);
        // Once the cached copy is a window old, the next poll asks again.
        backdate(&dir, "nba-scoreboard-20261113", PAST_WINDOW);
        espn.board(&p, Duration::ZERO);
        assert_eq!(espn.dated_calls.get(), 2);
    }

    /// `--once` is a new process each run: the cache is the only memory it
    /// has. A run that finds both entries young asks ESPN for nothing and
    /// still prints tonight's games.
    #[test]
    fn once_shows_tonights_games_and_a_second_run_asks_for_nothing() {
        let dir = tmp("once-morning");
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), Some(TONIGHT));
        let want: Vec<String> = [ids(LAST_NIGHT), ids(TONIGHT)].concat();
        for run in 0..2 {
            let (games, stale) = espn.board(&provider(&dir), crate::poll::SCOREBOARD_LIVE);
            assert!(!stale);
            let got: Vec<String> = games.into_iter().map(|g| g.id).collect();
            assert_eq!(got, want, "run {run}");
        }
        assert_eq!(espn.dated_calls.get(), 1);
    }

    /// A status bar running `--once` every 2 s while the dated URL fails
    /// with nothing cached: the bare board comes from its young cache with
    /// no request, and each run is a new process, so only the cache can
    /// hold the gate. One dated request for the window, not one a run.
    #[test]
    fn once_runs_with_a_failing_dated_board_ask_for_it_once_a_window() {
        let dir = tmp("once-dated-403");
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), None);
        for run in 0..5 {
            let (games, stale) = espn.quiet(&provider(&dir), crate::poll::SCOREBOARD_LIVE);
            assert!(!stale, "run {run}");
            assert_eq!(games.len(), 1, "run {run}");
        }
        assert_eq!(espn.bare_calls.get(), 1);
        assert_eq!(espn.dated_calls.get(), 1);
    }

    /// The day after a league's last game before a gap, its bare board
    /// stays on that game day (`fixtures/wnba_scoreboard_2026-11-13.json`
    /// carries 2026-10-31), so it lags the whole local day. Today's empty
    /// slate is asked for once, not once a minute all day.
    #[test]
    fn an_empty_slate_for_today_is_asked_for_once() {
        let dir = tmp("dated-empty");
        let p = provider(&dir);
        let espn = Espn::new(
            bare_on(LAST_NIGHT, Some("2026-11-12")),
            Some(r#"{"events":[]}"#),
        );
        assert_eq!(espn.board(&p, Duration::ZERO).0.len(), 1);
        backdate(&dir, "nba-scoreboard-20261113", PAST_WINDOW);
        assert_eq!(espn.board(&p, Duration::ZERO).0.len(), 1);
        assert_eq!(espn.dated_calls.get(), 1);
    }

    /// A CDN that serves the rolled and the unrolled bare board on
    /// alternating polls: once the league's bare board has served today, a
    /// later unrolled body is not lagging, so a live game never falls back
    /// to the older dated copy (its score could go backwards).
    #[test]
    fn a_league_that_has_rolled_over_does_not_lag_again_that_day() {
        let dir = tmp("rolled");
        let p = provider(&dir);
        let espn = Espn::new(bare_on(LAST_NIGHT, Some("2026-11-12")), Some(TONIGHT));
        assert_eq!(espn.board(&p, Duration::ZERO).0.len(), 3, "lagging");
        *espn.bare.borrow_mut() = bare_on(TONIGHT, Some("2026-11-13"));
        assert_eq!(espn.board(&p, Duration::ZERO).0.len(), 2, "rolled");
        *espn.bare.borrow_mut() = bare_on(LAST_NIGHT, Some("2026-11-12"));
        backdate(&dir, "nba-scoreboard-20261113", PAST_WINDOW);
        let (games, _) = espn.board(&p, Duration::ZERO);
        let got: Vec<String> = games.into_iter().map(|g| g.id).collect();
        assert_eq!(got, ids(LAST_NIGHT), "unrolled again: the bare board alone");
        assert_eq!(espn.dated_calls.get(), 1);
    }

    /// A 304 says the cached body is current now: its age restarts, so a
    /// gate on that age (`--once`'s, the dated board's, standings') does
    /// not ask again on the next call.
    #[test]
    fn a_304_restarts_the_cache_age() {
        let dir = tmp("304-age");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"v\":1}").unwrap();
        backdate(&dir, "k", Duration::from_secs(120));
        let calls = std::cell::Cell::new(0);
        let not_modified = |_: Option<&str>| {
            calls.set(calls.get() + 1);
            Ok(Fetched::NotModified)
        };
        for _ in 0..2 {
            let (_, stale) = p
                .cached_within("k", Duration::from_secs(60), not_modified, ok_map)
                .unwrap();
            assert!(!stale);
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn prune_removes_old_per_game_and_per_date_entries_and_keeps_the_rest() {
        let dir = tmp("prune");
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 60 * 60);
        let ancient = now - 400 * day;
        let old = now - CACHE_MAX_AGE - day;
        let young = now - CACHE_MAX_AGE + day;
        let files: [(&str, SystemTime, bool); 20] = [
            // (name, mtime, survives)
            ("nfl-401873001-summary", old, false),
            ("nfl-401873001-summary.etag", old, false),
            ("cfb-401752001-stats", old, false),
            ("nfl-scoreboard-20260913", old, false),
            ("nfl-scoreboard-20260913.etag", old, false),
            ("nba-401810001-summary", young, true),
            ("mlb-scoreboard-20261101", young, true),
            // The last good payload STALE serves: kept at any age.
            ("nfl-scoreboard", ancient, true),
            ("nfl-scoreboard.etag", ancient, true),
            ("cfb-scoreboard", ancient, true),
            // One per league, and names it does not recognize: kept.
            ("nfl-standings", ancient, true),
            ("xfl-401-summary", ancient, true),
            ("nfl-scoreboard-2026", ancient, true),
            ("notes.txt", ancient, true),
            // A killed write's temp file, for any entry the cache writes,
            // once old; one young enough to be a write in flight is kept.
            (".nfl-scoreboard.4242-0.tmp", old, false),
            (".nfl-401873001-summary.etag.4242-17.tmp", old, false),
            (".cfb-scoreboard.77-3.tmp", young, true),
            // Temp-shaped names it does not recognize: kept.
            (".notes.txt.4242-0.tmp", ancient, true),
            (".nfl-scoreboard.4242.tmp", ancient, true),
            ("nfl-scoreboard.4242-0.tmp", ancient, true),
        ];
        for (name, mtime, _) in files {
            let path = dir.join(name);
            std::fs::write(&path, "{}").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(mtime)
                .unwrap();
        }
        assert_eq!(prune_cache(&dir, now, CACHE_MAX_AGE), Ok(7));
        for (name, _, survives) in files {
            assert_eq!(dir.join(name).exists(), survives, "{name}");
        }
        assert_eq!(
            prune_cache(&dir.join("absent"), now, CACHE_MAX_AGE),
            Ok(0),
            "no cache dir yet is nothing to prune"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A removal that fails is reported, naming the file, not swallowed and
    /// not a panic.
    #[cfg(unix)]
    #[test]
    fn a_prune_that_cannot_remove_names_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("prune-ro");
        let path = dir.join("nfl-401-summary");
        std::fs::write(&path, "{}").unwrap();
        let day = Duration::from_secs(24 * 60 * 60);
        let now = SystemTime::now() + CACHE_MAX_AGE + day;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root (a packager's container) ignores the read-only bit, so the
        // removal this test needs to fail would succeed. Probe it first.
        if std::fs::write(dir.join("probe"), "").is_ok() {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::remove_dir_all(&dir).ok();
            eprintln!("skipped: a 0o555 directory is writable for this user (root?), so no removal can fail");
            return;
        }
        let got = prune_cache(&dir, now, CACHE_MAX_AGE);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = got.unwrap_err();
        assert!(
            err.starts_with("removed 0, 1 failed") && err.contains("nfl-401-summary"),
            "{err}"
        );
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
    /// The production agent with `https_only` off, so the one-shot servers
    /// below (plain HTTP on 127.0.0.1) can answer it.
    fn provider(dir: &Path) -> EspnProvider {
        EspnProvider::with_agent(dir.to_path_buf(), time::UtcOffset::UTC, HTTP_TIMEOUT, false)
    }

    /// ureq checks `https_only` on every hop with the same error, so a
    /// plain-http URL stands in for an https-to-http redirect (no local
    /// https server here). The footer must not call it "unreachable", and
    /// the log line names the URL, what was expected, and the knob.
    #[test]
    fn the_production_agent_refuses_plain_http() {
        let p = EspnProvider::new(tmp("https-only"), time::UtcOffset::UTC);
        let err = stamp(
            p.http("http://127.0.0.1:9/x", None).unwrap_err(),
            "nfl-scoreboard",
        );
        assert!(
            matches!(
                &err,
                ProviderError::Refused { why: Refusal::NotHttps { to }, .. } if to == "http://127.0.0.1:9/x"
            ),
            "{err:?}"
        );
        assert_eq!(err.short(), "ESPN not https nfl scoreboard");
        let full = err.to_string();
        for part in ["http://127.0.0.1:9/x", "expected https", "https_only"] {
            assert!(full.contains(part), "{part:?} missing from {full:?}");
        }
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
    fn user_agent_names_the_project_and_the_public_repo() {
        assert!(USER_AGENT.starts_with("gameday/"));
        assert!(
            USER_AGENT.ends_with("(+https://github.com/WallyMagill/gameday)"),
            "the contact URL must be the public repo: {USER_AGENT}"
        );
    }

    /// A refused TCP connect never reaches the HTTP layer at all — this
    /// exercises ureq's transport-error catch-all (`Err(e) => ...`), not the
    /// status-code arms. It holds under both ureq 2 and 3: a transport
    /// failure must keep `status: 0` with the detail text (which `short()`
    /// reads for the word "timeout"). The real-server tests below exercise
    /// the status-code arms (304 / 200 / 4xx) that this one can't reach.
    #[test]
    fn transport_errors_keep_status_zero_and_the_detail_text() {
        let dir = tmp("transport");
        let p = provider(&dir);
        // Port 9 on localhost refuses at once (nothing listens there), so
        // this doesn't wait out the 10s connect timeout on every run.
        let err = p.http("http://127.0.0.1:9/never", None).unwrap_err();
        match err {
            ProviderError::Http {
                status,
                detail,
                url,
                ..
            } => {
                assert_eq!(status, 0, "transport failures carry status 0");
                assert!(
                    !detail.is_empty(),
                    "detail must carry the transport error text"
                );
                assert_eq!(url, "http://127.0.0.1:9/never");
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    /// A one-shot raw HTTP/1.1 server: accepts a single connection, reads
    /// the request through the blank line that ends the headers (and hands
    /// that request text back over the channel for the caller to inspect),
    /// then writes `response` verbatim and lets the stream drop. Read/write
    /// timeouts on the accepted socket mean a malformed request fails the
    /// test instead of hanging it.
    fn one_shot_server(
        response: &'static str,
    ) -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
        one_shot_server_bytes(response.as_bytes().to_vec())
    }

    /// [`one_shot_server`] for a response that isn't text (a gzip body).
    fn one_shot_server_bytes(
        response: Vec<u8>,
    ) -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if line == "\r\n" || line == "\n" {
                                break;
                            }
                            request.push_str(&line);
                        }
                    }
                }
                let _ = tx.send(request);
                let mut stream = stream;
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        (addr, rx)
    }

    /// ureq 3 only classifies 4xx/5xx as `Error::StatusCode`; a real 304 —
    /// even with an ETag on it — arrives as a plain `Ok` response with an
    /// empty body. Regression for the bug where that empty body was mapped
    /// as `Fetched::Body`, failed to parse, and silently served the cache
    /// marked stale even though the 304 said it was current.
    #[test]
    fn a_304_from_a_real_server_is_not_modified_even_with_an_etag() {
        let (addr, rx) = one_shot_server(
            "HTTP/1.1 304 Not Modified\r\nETag: \"abc\"\r\nContent-Length: 0\r\n\r\n",
        );
        let dir = tmp("real-304");
        let p = provider(&dir);
        let got = p
            .http(&format!("http://{addr}/x"), Some("\"abc\""))
            .unwrap();
        assert_eq!(got, Fetched::NotModified);
        let req = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            req.lines().any(|l| {
                let mut parts = l.splitn(2, ':');
                let name = parts.next().unwrap_or("").trim();
                let value = parts.next().unwrap_or("").trim();
                name.eq_ignore_ascii_case("if-none-match") && value == "\"abc\""
            }),
            "expected an If-None-Match: \"abc\" header in the request, got: {req:?}"
        );
    }

    #[test]
    fn a_200_from_a_real_server_reads_the_body_and_the_etag() {
        let (addr, _rx) = one_shot_server(
            "HTTP/1.1 200 OK\r\nETag: \"def\"\r\nContent-Length: 7\r\n\r\n{\"v\":1}",
        );
        let dir = tmp("real-200");
        let p = provider(&dir);
        let got = p.http(&format!("http://{addr}/x"), None).unwrap();
        assert_eq!(
            got,
            Fetched::Body {
                body: "{\"v\":1}".to_string(),
                etag: Some("\"def\"".to_string()),
            }
        );
    }

    /// The cap counts decoded bytes: 96 bytes of gzip (65,536 zero bytes,
    /// `gzip.compress(b"\0" * 65536, mtime=0)`) against a 1,000-byte cap must
    /// fail, though the compressed body is well under it.
    #[test]
    fn a_gzip_body_over_the_decoded_cap_is_an_error_naming_the_cap() {
        const GZIP_64K_ZEROS: &[u8] = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\xff\xed\xc1\x01\x01\x00\x00\x00\x80\x90\xfe\xaf\xee\x08\x0a\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x6a\xeb\x8e\x97\xd7\x00\x00\x01\x00";
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 96\r\n\r\n".to_vec();
        response.extend_from_slice(GZIP_64K_ZEROS);
        assert_eq!(GZIP_64K_ZEROS.len(), 96);
        let (addr, _rx) = one_shot_server_bytes(response);
        let mut p = provider(&tmp("gzip-bomb"));
        p.max_body = 1_000;
        let err = stamp(
            p.http(&format!("http://{addr}/x"), None).unwrap_err(),
            "nfl-scoreboard",
        );
        assert!(
            matches!(
                &err,
                ProviderError::Refused {
                    why: Refusal::TooLarge { cap: 1_000 },
                    ..
                }
            ),
            "{err:?}"
        );
        // The footer says the answer was too big, not that ESPN was
        // unreachable; the log line names the value, the cap, and the knob.
        assert_eq!(err.short(), "ESPN too big nfl scoreboard");
        assert!(
            err.to_string().ends_with(
                "decoded body passed 1000 bytes (reading stopped at 1001), expected at most 1000 (MAX_BODY_BYTES in src/provider/espn.rs)"
            ),
            "{err}"
        );
    }

    #[test]
    fn read_capped_takes_exactly_the_cap_and_refuses_one_more() {
        let too_large = |cap| ProviderError::Refused {
            key: String::new(),
            url: "u".into(),
            why: Refusal::TooLarge { cap },
        };
        assert_eq!(read_capped(&b"abcd"[..], 4, "u").unwrap(), "abcd");
        let err = read_capped(&b"abcde"[..], 4, "u").unwrap_err();
        assert_eq!(err.to_string(), too_large(4).to_string());
        // A cut that lands inside a multi-byte char is still the cap error,
        // not a UTF-8 read error.
        let err = read_capped("aé".as_bytes(), 2, "u").unwrap_err();
        assert_eq!(err.to_string(), too_large(2).to_string());
    }

    #[test]
    fn a_403_from_a_real_server_is_the_error_with_status() {
        let (addr, _rx) = one_shot_server("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
        let dir = tmp("real-403");
        let p = provider(&dir);
        let err = p.http(&format!("http://{addr}/x"), None).unwrap_err();
        match err {
            ProviderError::Http { status, .. } => assert_eq!(status, 403),
            other => panic!("expected Http, got {other:?}"),
        }
    }

    /// ureq 3 leaves `recv_response` (the response-headers timeout)
    /// unbounded by default, unlike ureq 2's single `timeout_read`, which
    /// covered both headers and body. A server that accepts the connection
    /// and then never answers must not wedge the poll thread forever.
    /// `with_agent` overrides the real 10s `HTTP_TIMEOUT` down to 300ms
    /// so this test doesn't take as long as the bound it's proving.
    #[test]
    fn a_stalled_server_times_out_instead_of_hanging_forever() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                // Accept, then go silent — bounded to 1s so the thread
                // can't outlive the test even if the timeout under test
                // fails to fire.
                std::thread::sleep(Duration::from_millis(1000));
                drop(stream);
            }
        });
        let dir = tmp("stall");
        let p =
            EspnProvider::with_agent(dir, time::UtcOffset::UTC, Duration::from_millis(300), false);
        let err = p.http(&format!("http://{addr}/x"), None).unwrap_err();
        match err {
            ProviderError::Http { status, detail, .. } => {
                assert_eq!(
                    status, 0,
                    "a stalled response is a transport failure, not a status code"
                );
                assert!(
                    detail.to_lowercase().contains("timeout"),
                    "detail must say timeout so short() reports \"ESPN timeout\": {detail:?}"
                );
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }
}
