# gameday v3 · sub-project 1 — Truth · Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every number on the board is real, local, and current; the app degrades loudly instead of silently; Home is never empty while anything is live.

**Architecture:** Domain gains the fields the feeds already carry (`scoring_plays`, `start`, `linescore`, `timeouts`, per-sport `Extras`); the mapper becomes per-event fallible and owns time parsing; the provider maps before it caches and falls back to cache on mapping failure; a `Scheduler` in `poll.rs` replaces the inline cadence logic in `main.rs` (scoreboards for every enabled league, summary/stats for the zoomed game only); `App` gains network status, per-frame derived lists, and toasts for every verb. Visuals are untouched except where a field was previously printed wrong.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, ureq 2, serde_json, `time` 0.3 (`formatting`, `parsing`, `local-offset`), toml 0.8, dirs 5, tui-big-text 0.7. Tests: `cargo test` (unit + `tests/*.rs` with `ratatui::backend::TestBackend`), no network in tests.

**Spec:** `docs/superpowers/specs/2026-08-31-gameday-v3-1-truth-design.md` — read it first; every task below cites the section it implements.

## Global Constraints

- No network access in any test. Fixtures live in `fixtures/` and are loaded with `include_str!`.
- No new crates. `time` already has `parsing` + `formatting` + `local-offset`. No clap: CLI parsing stays hand-rolled in `src/main.rs`.
- `cargo test` green and `cargo clippy --all-targets` warning-free at the end of every task (the tree starts with 7 warnings; do not add any — fixing the existing ones is welcome but not required).
- Every error string names the actual value, the expected value/set, and the knob (spec §5), e.g. `config.toml:7 unknown league "NFLL", valid: nfl|cfb|cbb|nba|wnba|nhl|mlb|epl|mls`.
- Every numeric constant added gets a one-line comment saying whether it was measured (and where) or is a guess (and why). Receipts from the review: ESPN scoreboard `cache-control: max-age=5`; MLB summary 917 KB; MLB scoreboard 232 KB; 40 rapid sequential scoreboard requests produced zero non-200s.
- Nothing in this plan changes tile geometry, colors, digits, logos, themes, sidebar layout, or ranking. Those are sub-project 2. If a task tempts you to restyle, don't.
- Commit after every task with the message shown. Before each commit that touches rendering, run `cargo run --release -- dump` and open at least `out/board-broadcast.png` and the capture your task adds; the gallery needs `GAMEDAY_DUMP_FONT=<path to CascadiaMono.ttf>` for sextant glyphs to render in PNGs.
- Deviation from spec §1 recorded here: `Meter::Penalty` is **not** constructed in this sub-project. The only verified NHL signal is `plays[].strength.id == "702"` on *goals*, which says a goal was on the power play, not that a power play is in progress. Task 4 tags those plays `PP`; the meter waits for a live NHL fixture (sub-project 3). Do not guess a field name.

## File Structure

| File | Responsibility after this plan |
|---|---|
| `src/text.rs` | Truncation + **all time formatting**: `local_time`, `fmt_start`, `fmt_clock12`, `startup_offset` |
| `src/domain.rs` | Domain types; gains `Extras`, `MatchEvent`, `EventKind`, `Default for Game`, new fields |
| `src/provider/map.rs` | ESPN JSON → domain; per-event fallible; owns time parsing (takes a `UtcOffset`) |
| `src/provider/espn.rs` | HTTP + disk cache; map-then-cache; timeouts; ETag sidecar; structured errors |
| `src/provider/mod.rs` | `ProviderError` (structured), `SportsProvider` trait (unchanged signatures) |
| `src/poll.rs` | `Scheduler`: per-league cadence, backoff with jitter, stagger, zoomed-only summary/stats, budget |
| `src/main.rs` | CLI (`--help/--version/--config-dir`), TTY guard, panic hook, offset capture, poll thread driven by `Scheduler`, `Msg::Failed` |
| `src/config.rs` | XDG config dir with legacy fallback, `LoadOutcome` (config + parse error), pins |
| `src/home.rs` | Home ordering: pins → favorites → everything live |
| `src/app/mod.rs` | `App` state, apply/merge, key handlers (moved from `src/app.rs`) |
| `src/app/derive.rs` | `Derived` per-frame lists built once per draw |
| `src/app/chrome.rs` | header (with `NetStatus` chip), footer (toasts, completion candidates), help overlay |
| `src/app/net.rs` | `NetStatus` state machine: LIVE / STALE / OFFLINE / NO DATA YET |
| `src/keymap.rs` | Bindings gain `[/]` DATE; handler→keymap coverage test |
| `src/tiles/mod.rs` | Tile renderer reads `start`, `Play.period`, `Extras`, pin/fav glyphs; logo art memoized |
| `src/views/board.rs`, `zoom.rs`, `standings.rs` | Slate local times + scope-aware filter message; zoom linescore row; standings sort/label/columns |
| `tests/*.rs`, `fixtures/*.json` | Full-length fixtures per league; new tests per task |

---

### Task 1: Time helpers in `text.rs` (spec §2 Time)

**Files:**
- Modify: `src/text.rs`
- Test: `src/text.rs` (unit tests, same file)

**Interfaces:**
- Produces: `pub fn local_time(iso: &str, offset: time::UtcOffset) -> Option<time::OffsetDateTime>`; `pub fn fmt_start(start: OffsetDateTime, now: OffsetDateTime) -> String`; `pub fn fmt_clock12(t: OffsetDateTime) -> String` (`9:38:07 PM`); `pub fn startup_offset() -> time::UtcOffset` (captured on the main thread; UTC + one stderr line on failure).

- [ ] **Step 1: Write the failing tests** — append to `src/text.rs`:

```rust
#[cfg(test)]
mod time_tests {
    use super::*;
    use time::macros::datetime;
    use time::UtcOffset;

    #[test]
    fn local_time_parses_espn_iso_and_applies_offset() {
        let la = UtcOffset::from_hms(-7, 0, 0).unwrap();
        let t = local_time("2026-09-01T01:38Z", la).unwrap();
        // 01:38 UTC on Sep 1 is 6:38 PM on Aug 31 in Los Angeles.
        assert_eq!(t, datetime!(2026-08-31 18:38 -7));
        let london = UtcOffset::from_hms(1, 0, 0).unwrap();
        assert_eq!(local_time("2026-09-01T01:38Z", london).unwrap(), datetime!(2026-09-01 02:38 +1));
        // ESPN also sends full offsets and fractional seconds on some feeds.
        assert!(local_time("2026-09-13T17:00:00Z", la).is_some());
        assert!(local_time("2026-09-13T17:00:00.000+00:00", la).is_some());
        assert_eq!(local_time("not a date", la), None);
        assert_eq!(local_time("", la), None);
    }

    #[test]
    fn fmt_start_is_clock_today_and_day_clock_within_the_week() {
        let now = datetime!(2026-08-31 21:30 -4);
        assert_eq!(fmt_start(datetime!(2026-08-31 21:38 -4), now), "9:38 PM");
        assert_eq!(fmt_start(datetime!(2026-09-03 20:20 -4), now), "THU 8:20 PM");
        assert_eq!(fmt_start(datetime!(2026-09-06 13:00 -4), now), "SUN 1:00 PM");
        // A week or more out: the date, never a bare weekday that could mean two days.
        assert_eq!(fmt_start(datetime!(2026-09-13 13:00 -4), now), "SEP 13 1:00 PM");
        // Midnight and noon edges.
        assert_eq!(fmt_start(datetime!(2026-08-31 00:05 -4), now), "12:05 AM");
        assert_eq!(fmt_start(datetime!(2026-08-31 12:00 -4), now), "12:00 PM");
    }

    #[test]
    fn fmt_clock12_has_seconds_and_meridiem() {
        assert_eq!(fmt_clock12(datetime!(2026-08-31 21:30:01 -4)), "9:30:01 PM");
        assert_eq!(fmt_clock12(datetime!(2026-08-31 00:00:00 -4)), "12:00:00 AM");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib text::time_tests 2>&1 | tail -5`
Expected: compile error `cannot find function local_time`.

- [ ] **Step 3: Implement** — add to `src/text.rs` above the test modules:

```rust
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

/// ESPN's `date` fields are RFC 3339 with a `Z` and no seconds
/// (`2026-09-01T01:38Z`) on the scoreboard, and full `…:00.000+00:00` on some
/// summaries. `Rfc3339` needs seconds, so the short form is padded first.
pub fn local_time(iso: &str, offset: UtcOffset) -> Option<OffsetDateTime> {
    let iso = iso.trim();
    if iso.is_empty() {
        return None;
    }
    // "YYYY-MM-DDTHH:MMZ" -> "YYYY-MM-DDTHH:MM:00Z"
    let padded;
    let s = if iso.len() == 17 && iso.ends_with('Z') && iso.as_bytes()[13] == b':' {
        padded = format!("{}:00Z", &iso[..16]);
        padded.as_str()
    } else {
        iso
    };
    OffsetDateTime::parse(s, &Rfc3339).ok().map(|t| t.to_offset(offset))
}

/// Start-time label relative to `now` (same offset as `start`):
/// today → `9:38 PM`; within the next six days → `THU 8:20 PM`; otherwise
/// `SEP 13 1:00 PM`. Six days keeps a bare weekday unambiguous.
pub fn fmt_start(start: OffsetDateTime, now: OffsetDateTime) -> String {
    let clock = hm12(start);
    let days = (start.date() - now.date()).whole_days();
    if days == 0 {
        clock
    } else if (1..=6).contains(&days) {
        format!("{} {clock}", &format!("{:?}", start.weekday()).to_uppercase()[..3])
    } else {
        format!(
            "{} {} {clock}",
            &format!("{:?}", start.month()).to_uppercase()[..3],
            start.day()
        )
    }
}

/// Header wall clock: `9:30:01 PM`.
pub fn fmt_clock12(t: OffsetDateTime) -> String {
    let (h, ampm) = h12(t.hour());
    format!("{h}:{:02}:{:02} {ampm}", t.minute(), t.second())
}

fn hm12(t: OffsetDateTime) -> String {
    let (h, ampm) = h12(t.hour());
    format!("{h}:{:02} {ampm}", t.minute())
}

fn h12(hour: u8) -> (u8, &'static str) {
    match hour {
        0 => (12, "AM"),
        h if h < 12 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    }
}

/// The local UTC offset, read ONCE on the main thread before any other thread
/// exists: `time` refuses to read the TZ database from a multi-threaded
/// process on Unix (it returns Err), and the old per-call
/// `now_local().unwrap_or_else(now_utc)` silently printed UTC in that case.
pub fn startup_offset() -> UtcOffset {
    match UtcOffset::current_local_offset() {
        Ok(off) => off,
        Err(e) => {
            eprintln!("gameday: local UTC offset unavailable ({e}); times will show in UTC — set TZ to fix");
            UtcOffset::UTC
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib text:: 2>&1 | tail -5`
Expected: all `text::` tests PASS (existing truncate/surname + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/text.rs
git commit -m "feat(v3.1): one place for time — local_time, fmt_start, fmt_clock12, startup_offset"
```

---

### Task 2: Domain fields, `Default for Game`, and every literal site (spec §1)

**Files:**
- Modify: `src/domain.rs`
- Modify (literal sites, mechanical): `src/alerts.rs`, `src/ticker.rs`, `src/demo.rs`, `src/main.rs`, `src/provider/memory.rs`, `src/input.rs`, `src/app.rs`, `src/tiles/mod.rs`, `src/provider/map.rs`, `src/views/board.rs`, `tests/packer.rs`, `tests/theme.rs`, `tests/home.rs`, `tests/draw.rs`, `tests/tile_snap.rs`, `tests/poll.rs`
- Test: `src/domain.rs`

**Interfaces:**
- Produces (exact):

```rust
pub struct Team { /* existing */ pub rank: Option<u8> }
pub struct Play { pub clock: String, pub period: String, pub team: String, pub text: String, pub scoring: bool }
pub struct Situation { /* existing */ pub pitcher: Option<String>, pub batter: Option<String>, pub due_up: Vec<String> }
pub enum EventKind { Goal, OwnGoal, Penalty, Yellow, Red, Sub }
pub struct MatchEvent { pub minute: String, pub kind: EventKind, pub team: String, pub player: String }
pub enum Extras {
    None,
    Football { drive: Option<String> },
    Baseball { hits: Option<(u16, u16)>, errors: Option<(u16, u16)> },
    Hockey { shots: Option<(u16, u16)> },
    Soccer { events: Vec<MatchEvent> },
}
pub struct Game {
    /* existing minus start_time */
    pub start: Option<time::OffsetDateTime>,
    pub scoring_plays: Vec<Play>,
    pub linescore: Vec<(u16, u16)>,   // per period/inning, (away, home)
    pub timeouts: Option<(u8, u8)>,   // (away, home) remaining
    pub extras: Extras,
}
impl Default for Game  // id "", league Nfl, status Pre, everything empty/None
impl Default for Extras // Extras::None
```
- `start_time: Option<String>` is **removed**. Every place that printed it now calls `crate::text::fmt_start(start, now)`; where `now` isn't available yet (Task 9 threads it through), use `time::OffsetDateTime::now_utc().to_offset(start.offset())` as a temporary and leave a `// Task 9: use App::now()` comment.

- [ ] **Step 1: Write the failing test** — in `src/domain.rs` tests:

```rust
    #[test]
    fn game_default_is_an_empty_pregame() {
        let g = Game::default();
        assert_eq!(g.status, Status::Pre);
        assert_eq!(g.league, League::Nfl);
        assert!(g.scoring_plays.is_empty() && g.linescore.is_empty() && g.last_plays.is_empty());
        assert_eq!(g.start, None);
        assert_eq!(g.timeouts, None);
        assert_eq!(g.extras, Extras::None);
        assert_eq!(g.away.rank, None);
        // Literal sites use `..Game::default()`; a play carries its period.
        let p = Play { period: "B9".into(), ..Default::default() };
        assert_eq!(p.period, "B9");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib domain::tests::game_default 2>&1 | tail -3`
Expected: `no function or associated item named default found for struct Game`.

- [ ] **Step 3: Implement the domain changes** in `src/domain.rs`:

```rust
// Team: add after `pub logo_key: String,`
    /// AP/coaches rank for college sports (`competitors[].curatedRank.current`,
    /// verified `14` for USC 2026-08-31). None for pro leagues and unranked teams.
    pub rank: Option<u8>,

// Play: add after `pub clock: String,`
    /// Period label for sports without a play clock: baseball `B9`/`T7`;
    /// empty when the clock carries the moment. Renders where `[-:--]` did.
    pub period: String,

// Situation: add after `pub on_base: Option<[bool; 3]>,`
    /// Baseball matchup from `situation.pitcher/.batter` (athlete shortName).
    pub pitcher: Option<String>,
    pub batter: Option<String>,
    /// Baseball `situation.dueUp[]` as "A. Riley (2-3, HR)" strings, in order.
    pub due_up: Vec<String>,

// New types, after Meter:
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind { Goal, OwnGoal, Penalty, Yellow, Red, Sub }

/// One soccer match event from the scoreboard's `competition.details[]`
/// (goals, cards, substitutions), verified in fixtures/epl_scoreboard.json.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchEvent {
    pub minute: String,
    pub kind: EventKind,
    pub team: String,
    pub player: String,
}

/// Per-sport facts that don't fit the shared fields. One variant per sport
/// family; `None` for sports with nothing extra yet.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Extras {
    #[default]
    None,
    Football { drive: Option<String> },
    Baseball { hits: Option<(u16, u16)>, errors: Option<(u16, u16)> },
    Hockey { shots: Option<(u16, u16)> },
    Soccer { events: Vec<MatchEvent> },
}

// Game: replace `pub start_time: Option<String>,` with the block below
    /// Scheduled start, already in the user's local offset (mapper applies
    /// `text::local_time`). Rendered through `text::fmt_start` — never raw.
    pub start: Option<time::OffsetDateTime>,
    /// Scoring plays, oldest first. Filled two ways: a score delta between
    /// scoreboard polls captures that poll's `lastPlay` (every game); the
    /// summary's full list replaces it for the zoomed game.
    pub scoring_plays: Vec<Play>,
    /// Per period/inning (away, home) from `competitors[].linescores[]`.
    pub linescore: Vec<(u16, u16)>,
    /// (away, home) timeouts remaining, football/basketball only.
    pub timeouts: Option<(u8, u8)>,
    pub extras: Extras,

impl Default for Game {
    fn default() -> Self {
        Game {
            id: String::new(),
            league: League::Nfl,
            home: Team::default(),
            away: Team::default(),
            home_score: 0,
            away_score: 0,
            status: Status::Pre,
            period: String::new(),
            clock: String::new(),
            situation: None,
            last_plays: vec![],
            meter: None,
            start: None,
            broadcast: None,
            odds: None,
            scoring_plays: vec![],
            linescore: vec![],
            timeouts: None,
            extras: Extras::None,
        }
    }
}
```

- [ ] **Step 4: Fix every literal site.** `cargo build --all-targets 2>&1 | grep -c 'missing field'` lists them. For each `Game { … }` literal in tests and helpers, delete the `start_time: …, broadcast: …, odds: …` tail lines that are `None` and end the literal with `..Game::default()`. For the mapper (`map.rs:313-316`) fill the new fields explicitly: `start: None, scoring_plays: vec![], linescore: vec![], timeouts: None, extras: Extras::None` (Task 3 populates them). For `src/demo.rs:171` replace `start_time: Some("8:20 PM".into())` with `start: Some(time::macros::datetime!(2026-09-13 20:20 -4))` (the demo's fixed instant) and add `time = { …, features = [… "macros"] }`? — **no**: the `macros` feature is already implied by `formatting`+`parsing` in time 0.3.36+; if `datetime!` fails to resolve, add `"macros"` to the existing `time` features line in `Cargo.toml` (no new crate). Every `Play { … }` literal gains `period: String::new(),` or `..Default::default()`.

Where `start_time` was *read*:
- `src/tiles/mod.rs:165` (`situation_summary`): `let mut s = game.start.map(|t| crate::text::fmt_start(t, time::OffsetDateTime::now_utc().to_offset(t.offset()))).unwrap_or_default(); // Task 9: App::now()`
- `src/tiles/mod.rs:892`: same shape.
- `src/views/board.rs:312` (`slate_line`): `game.start.map(|t| crate::text::fmt_start(t, time::OffsetDateTime::now_utc().to_offset(t.offset()))).unwrap_or_else(|| "--:--".into())`
- `tests/draw.rs:42`: `start: Some(time::macros::datetime!(2026-09-13 20:20 -4)),`

- [ ] **Step 5: Build and run everything**

Run: `cargo test 2>&1 | tail -4 && cargo clippy --all-targets 2>&1 | grep -c '^warning' `
Expected: all tests pass (294 + 1); warning count ≤ 7.

- [ ] **Step 6: Commit**

```bash
git add -A src tests Cargo.toml
git commit -m "feat(v3.1): domain carries start, scoring_plays, linescore, timeouts, extras; Game::default"
```

---

### Task 3: Scoreboard mapper — per-event fallibility, local start, ranks, linescores, timeouts, soccer events, MLB matchup (spec §2)

**Files:**
- Modify: `src/provider/map.rs` (`map_scoreboard`, `team_from`, new `map_event`, `details_from`, `mlb_extras`)
- Modify: callers of `map_scoreboard` (`src/provider/espn.rs:140,158`, `src/main.rs` probe, `src/dump.rs` if any, `tests/map_espn.rs`) — signature gains `offset: time::UtcOffset`
- Test: `tests/map_espn.rs`

**Interfaces:**
- Produces: `pub fn map_scoreboard(league: League, json: &str, offset: time::UtcOffset) -> Result<Vec<Game>, MapError>`; `pub fn map_event(league: League, ev: &Value, offset: UtcOffset) -> Result<Game, MapError>`; `MapError::Missing(&'static str)` unchanged; new `MapError::Event { id: String, path: &'static str }` used only for the stderr skip line.

- [ ] **Step 1: Write the failing tests** — append to `tests/map_espn.rs`:

```rust
use time::UtcOffset;
fn et() -> UtcOffset { UtcOffset::from_hms(-4, 0, 0).unwrap() }

#[test]
fn a_malformed_event_is_skipped_not_fatal() {
    // Second event has no competitors (ESPN ships placeholder rows in preseason).
    let json = r#"{"events":[
      {"id":"1","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}},"competitors":[
        {"homeAway":"away","score":"0","team":{"id":"1","abbreviation":"NE"}},
        {"homeAway":"home","score":"0","team":{"id":"2","abbreviation":"SEA"}}]}]},
      {"id":"2","date":"2026-09-10T00:20Z","competitions":[{"status":{"type":{"state":"pre"}}}]}
    ]}"#;
    let games = map_scoreboard(League::Nfl, json, et()).unwrap();
    assert_eq!(games.len(), 1, "the good event survives the bad one");
    assert_eq!(games[0].id, "1");
}

#[test]
fn start_is_local_and_never_a_raw_string() {
    let games = map_scoreboard(League::Wnba, include_str!("../fixtures/wnba_scoreboard.json"), et()).unwrap();
    let pre = games.iter().find(|g| g.status == Status::Pre).expect("fixture has a pre game");
    let start = pre.start.expect("start parsed");
    assert_eq!(start.offset(), et());
}

#[test]
fn mlb_maps_linescore_hits_errors_matchup_and_play_period() {
    let games = map_scoreboard(League::Mlb, include_str!("../fixtures/mlb_scoreboard.json"), et()).unwrap();
    let g = games.iter().find(|g| g.id == "401816718").unwrap();
    assert!(g.linescore.len() >= 7, "per-inning linescore, got {:?}", g.linescore);
    match &g.extras {
        gameday::domain::Extras::Baseball { hits, errors } => {
            assert!(hits.is_some() && errors.is_some(), "hits/errors are on every MLB competitor");
        }
        other => panic!("expected Baseball extras, got {other:?}"),
    }
    let sit = g.situation.as_ref().unwrap();
    assert!(sit.pitcher.is_some() && sit.batter.is_some(), "situation.pitcher/batter present live");
    // The scoreboard lastPlay is a pitch ("Pitch 6 : Ball 3"); the tile wants
    // the human label plus the batter, and the inning where the clock would be.
    let p = &g.last_plays[0];
    assert_eq!(p.text, "Walk — A. Riley");
    assert_eq!(p.period, "B7");
    assert_eq!(p.clock, "");
}

#[test]
fn soccer_details_become_match_events() {
    let games = map_scoreboard(League::Epl, include_str!("../fixtures/epl_scoreboard.json"), et()).unwrap();
    let g = games.iter().find(|g| g.id == "401879314").unwrap();
    let gameday::domain::Extras::Soccer { events } = &g.extras else { panic!("soccer extras") };
    assert!(!events.is_empty());
    let goal = events.iter().find(|e| e.kind == gameday::domain::EventKind::Goal).unwrap();
    assert!(goal.minute.ends_with('\''), "{}", goal.minute);
    assert!(!goal.player.is_empty());
}

#[test]
fn cfb_rank_comes_from_curated_rank() {
    let json = r#"{"events":[{"id":"1","date":"2026-08-29T23:30Z","competitions":[{"status":{"type":{"state":"post"},"period":4},"competitors":[
      {"homeAway":"away","score":"26","curatedRank":{"current":99},"team":{"id":"1","abbreviation":"SJSU"}},
      {"homeAway":"home","score":"42","curatedRank":{"current":14},"team":{"id":"2","abbreviation":"USC"}}]}]}]}"#;
    let g = &map_scoreboard(League::Cfb, json, et()).unwrap()[0];
    assert_eq!(g.home.rank, Some(14));
    assert_eq!(g.away.rank, None, "ESPN uses 99 for unranked");
}

#[test]
fn football_timeouts_map_from_situation() {
    let json = r#"{"events":[{"id":"1","date":"2026-09-13T17:00Z","competitions":[{"status":{"type":{"state":"in"},"period":4,"displayClock":"1:27"},
      "situation":{"awayTimeouts":1,"homeTimeouts":3,"downDistanceText":"1st & Goal","possessionText":"TB 3","possession":"1"},
      "competitors":[{"homeAway":"away","score":"27","team":{"id":"1","abbreviation":"KC"}},{"homeAway":"home","score":"24","team":{"id":"2","abbreviation":"TB"}}]}]}]}"#;
    let g = &map_scoreboard(League::Nfl, json, et()).unwrap()[0];
    assert_eq!(g.timeouts, Some((1, 3)));
}
```

Also update the five existing `map_scoreboard(League::X, json)` calls in this file to pass `et()`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test map_espn 2>&1 | tail -5`
Expected: compile error on arity (`map_scoreboard` takes 2 arguments).

- [ ] **Step 3: Implement.** In `src/provider/map.rs`:

```rust
use time::UtcOffset;

// MapError: add a variant
    #[error("event {id}: missing {path}")]
    Event { id: String, path: &'static str },

// team_from: add rank (99 = unranked in ESPN's curatedRank)
        rank: v.get("curatedRank").and_then(|r| r["current"].as_u64()).filter(|&n| (1..=25).contains(&n)).map(|n| n as u8),
```

Note `curatedRank` sits on the **competitor**, not on `team`; pass it in: change `team_from(league, &c["team"])` to `team_from(league, &c["team"], &c["curatedRank"])` and add a `rank: &Value` parameter.

Replace the body of `map_scoreboard` with a loop over `map_event`:

```rust
pub fn map_scoreboard(league: League, json: &str, offset: UtcOffset) -> Result<Vec<Game>, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let events = v.get("events").and_then(|e| e.as_array()).ok_or(MapError::Missing("events"))?;
    let mut out = Vec::with_capacity(events.len());
    for ev in events {
        match map_event(league, ev, offset) {
            Ok(g) => out.push(g),
            Err(e) => {
                // One placeholder row must not erase the league (spec §2).
                eprintln!("gameday: {} scoreboard: skipped {e}", league.slug());
            }
        }
    }
    Ok(out)
}

pub fn map_event(league: League, ev: &Value, offset: UtcOffset) -> Result<Game, MapError> {
    let id = ev.get("id").and_then(|x| x.as_str()).ok_or(MapError::Missing("id"))?.to_string();
    let miss = |path: &'static str| MapError::Event { id: id.clone(), path };
    let start = ev.get("date").and_then(|x| x.as_str()).and_then(|s| crate::text::local_time(s, offset));
    let comp = ev.get("competitions").and_then(|c| c.as_array()).and_then(|a| a.first()).ok_or_else(|| miss("competitions[0]"))?;
    // … existing status/period/clock code unchanged …
    let comps = comp.get("competitors").and_then(|c| c.as_array()).ok_or_else(|| miss("competitors"))?;
    let mut linescore_away: Vec<u16> = vec![];
    let mut linescore_home: Vec<u16> = vec![];
    let mut hits = (None, None);
    let mut errors = (None, None);
    for c in comps {
        let mut team = team_from(league, &c["team"], &c["curatedRank"]).ok_or_else(|| miss("competitors[].team"))?;
        team.record = record_from(c);
        let score = c["score"].as_str().unwrap_or("0").parse().unwrap_or(0);
        let ls: Vec<u16> = c["linescores"].as_array().map(|a| a.iter().map(|p| p["value"].as_f64().unwrap_or(0.0) as u16).collect()).unwrap_or_default();
        let h = c["hits"].as_u64().map(|n| n as u16);
        let e = c["errors"].as_u64().map(|n| n as u16);
        match c["homeAway"].as_str() {
            Some("home") => { home_score = score; home = Some(team); linescore_home = ls; hits.1 = h; errors.1 = e; }
            _ => { away_score = score; away = Some(team); linescore_away = ls; hits.0 = h; errors.0 = e; }
        }
    }
    let home = home.ok_or_else(|| miss("competitors[homeAway=home]"))?;
    let away = away.ok_or_else(|| miss("competitors[homeAway=away]"))?;
    let n = linescore_away.len().min(linescore_home.len());
    let linescore: Vec<(u16, u16)> = (0..n).map(|i| (linescore_away[i], linescore_home[i])).collect();
    // … existing situation block; inside the MLB branch add:
    //     sit.pitcher = sit_v["pitcher"]["athlete"]["shortName"].as_str().map(str::to_string);
    //     sit.batter  = sit_v["batter"]["athlete"]["shortName"].as_str().map(str::to_string);
    //     sit.due_up  = sit_v["dueUp"].as_array().map(|a| a.iter().filter_map(due_up_line).collect()).unwrap_or_default();
    let timeouts = match (sit_v["awayTimeouts"].as_u64(), sit_v["homeTimeouts"].as_u64()) {
        (Some(a), Some(h)) => Some((a.min(9) as u8, h.min(9) as u8)),
        _ => None,
    };
    // lastPlay: MLB pitch rows get the human label + batter; the inning goes where the clock would.
    let mut last_plays = Vec::new();
    if let Some(text) = sit_v["lastPlay"]["text"].as_str() {
        let team = abbr_for_id(sit_v["lastPlay"]["team"]["id"].as_str())
            .or_else(|| situation.as_ref().and_then(|s| s.possession.clone()))
            .unwrap_or_default();
        let text = if league == League::Mlb {
            mlb_last_play_text(&sit_v["lastPlay"]).unwrap_or_else(|| text.to_string())
        } else {
            text.to_string()
        };
        last_plays.push(Play {
            clock: if league == League::Mlb { String::new() } else { sit_v["lastPlay"]["clock"]["displayValue"].as_str().unwrap_or(&clock).to_string() },
            period: if league == League::Mlb { mlb_inning_tag(&period) } else { String::new() },
            team,
            text,
            scoring: false,
        });
    }
    let extras = match league {
        League::Mlb => Extras::Baseball {
            hits: hits.0.zip(hits.1),
            errors: errors.0.zip(errors.1),
        },
        League::Epl | League::Mls => Extras::Soccer { events: details_from(&comp["details"], &abbr_for_id) },
        League::Nfl | League::Cfb => Extras::Football { drive: None },
        League::Nhl => Extras::Hockey { shots: None },
        _ => Extras::None,
    };
    // … odds/broadcast/meter unchanged …
    Ok(Game { id, league, home, away, home_score, away_score, status, period, clock, situation,
              last_plays, meter, start, broadcast, odds, scoring_plays: vec![], linescore, timeouts, extras })
}

/// "BOT 7TH" -> "B7", "TOP 9TH" -> "T9", "MID 5TH"/"END 8TH" -> "M5"/"E8".
fn mlb_inning_tag(period: &str) -> String {
    let mut it = period.split_whitespace();
    let (Some(half), Some(num)) = (it.next(), it.next()) else { return String::new() };
    let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
    format!("{}{digits}", &half[..1])
}

/// `lastPlay.type.alternativeText` ("Walk", "Strikeout") + the batter — the
/// `text` field is the pitch ("Pitch 6 : Ball 3"), which nobody wants.
fn mlb_last_play_text(lp: &Value) -> Option<String> {
    let label = lp["type"]["alternativeText"].as_str().or(lp["type"]["text"].as_str())?;
    let batter = lp["athletesInvolved"].as_array().and_then(|a| a.first()).and_then(|a| a["shortName"].as_str());
    Some(match batter {
        Some(b) => format!("{label} — {b}"),
        None => label.to_string(),
    })
}

fn due_up_line(v: &Value) -> Option<String> {
    let name = v["athlete"]["shortName"].as_str()?;
    Some(match v["summary"].as_str() {
        Some(s) if !s.is_empty() => format!("{name} ({s})"),
        _ => name.to_string(),
    })
}

/// Soccer `competition.details[]`: goals/cards/subs with minute and player.
fn details_from(details: &Value, abbr_for_id: &dyn Fn(Option<&str>) -> Option<String>) -> Vec<MatchEvent> {
    let Some(arr) = details.as_array() else { return vec![] };
    arr.iter().filter_map(|d| {
        let kind = if d["scoringPlay"].as_bool() == Some(true) {
            if d["ownGoal"].as_bool() == Some(true) { EventKind::OwnGoal }
            else if d["penaltyKick"].as_bool() == Some(true) { EventKind::Penalty }
            else { EventKind::Goal }
        } else if d["redCard"].as_bool() == Some(true) { EventKind::Red }
        else if d["yellowCard"].as_bool() == Some(true) { EventKind::Yellow }
        else if d["type"]["text"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("Substitution")) { EventKind::Sub }
        else { return None };
        Some(MatchEvent {
            minute: d["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
            kind,
            team: abbr_for_id(d["team"]["id"].as_str()).unwrap_or_default(),
            player: d["athletesInvolved"].as_array().and_then(|a| a.first()).and_then(|a| a["shortName"].as_str()).unwrap_or("").to_string(),
        })
    }).collect()
}
```

`abbr_for_id` is currently a closure capturing `home`/`away`; keep it a closure and pass `&abbr_for_id` (it is `Fn`). Update `espn.rs` calls to `map_scoreboard(league, body, self.offset)` — add `pub offset: UtcOffset` to `EspnProvider` (set in `main.rs` from `text::startup_offset()`; tests use `UtcOffset::UTC`). Update `probe` in `main.rs` and any `dump.rs` use.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS, including the six new tests. If `soccer_details_become_match_events` fails because the fixture's finished match has an empty `details`, pick another event id from `fixtures/epl_scoreboard.json` whose `details` is non-empty (`jq '.events[] | select(.competitions[0].details|length>0) | .id' fixtures/epl_scoreboard.json`) and fix the id in the test — never weaken the assertion.

- [ ] **Step 5: Commit**

```bash
git add src/provider src/main.rs src/dump.rs tests/map_espn.rs
git commit -m "feat(v3.1): scoreboard mapper — per-event skip, local start, rank, linescore, timeouts, soccer events, MLB matchup"
```

---

### Task 4: Summary mapper — no truncation, MLB at-bat feed, grouped box score, NHL PP tag, play periods (spec §2 MLB)

**Files:**
- Modify: `src/provider/map.rs` (`map_summary`, `map_stats`)
- Test: `tests/map_espn.rs`, new fixture `fixtures/mlb_summary.json` (Task 18 captures the real one; this task ships a **hand-built minimal** fixture that exercises the shapes)

**Interfaces:**
- `map_summary(json) -> Result<Summary, MapError>` unchanged signature. `Summary.last_plays` is now the **full** list, newest first (no `split_off(len-8)`); display truncation moves to the tile (Task 9). `Play.period` set for MLB from each play's `period.type`+`period.number` (`T9`/`B9`).
- `map_stats` handles both `statistics[{name, displayValue}]` (flat) and `statistics[{name, stats:[{name, displayValue}]}]` (grouped, MLB). Missing `leaders` → empty `leaders`, not an error.

- [ ] **Step 1: Write the fixture** `fixtures/mlb_summary_min.json` (hand-built, minimal, mirrors verified shapes):

```json
{"header":{"competitions":[{"competitors":[
  {"homeAway":"away","team":{"id":"12","abbreviation":"SEA"}},
  {"homeAway":"home","team":{"id":"2","abbreviation":"BOS"}}]}]},
 "plays":[
  {"text":"Devers homered to right (24)","summaryType":"S","scoringPlay":true,"team":{"id":"2"},"period":{"type":"Bottom","number":3}},
  {"text":"Pitch 1 : Ball 1","summaryType":"P","scoringPlay":false,"team":{"id":"12"},"period":{"type":"Top","number":4}},
  {"text":"Pitch 2 : Strike 1 Looking","summaryType":"P","scoringPlay":false,"team":{"id":"12"},"period":{"type":"Top","number":4}},
  {"text":"Raleigh struck out swinging.","summaryType":"N","scoringPlay":false,"team":{"id":"12"},"period":{"type":"Top","number":4}},
  {"text":"Top of the 5th inning","summaryType":"I","scoringPlay":false,"period":{"type":"Top","number":5}},
  {"text":"Rodríguez singles to right, Crawford to third","summaryType":"N","scoringPlay":false,"team":{"id":"12"},"period":{"type":"Top","number":9}},
  {"text":"Pitch 1 : Strike 1 Foul","summaryType":"P","scoringPlay":false,"team":{"id":"12"},"period":{"type":"Top","number":9}}
 ],
 "boxscore":{"teams":[
  {"homeAway":"away","team":{"abbreviation":"SEA"},"statistics":[{"name":"batting","stats":[{"name":"hits","label":"H","displayValue":"11"},{"name":"runs","label":"R","displayValue":"8"}]}]},
  {"homeAway":"home","team":{"abbreviation":"BOS"},"statistics":[{"name":"batting","stats":[{"name":"hits","label":"H","displayValue":"9"},{"name":"runs","label":"R","displayValue":"7"}]}]}
 ]}}
```

- [ ] **Step 2: Write the failing tests** — append to `tests/map_espn.rs`:

```rust
use gameday::provider::map::map_stats;

#[test]
fn mlb_summary_keeps_only_at_bat_results_and_scoring_and_tags_the_inning() {
    let s = map_summary(include_str!("../fixtures/mlb_summary_min.json")).unwrap();
    let texts: Vec<&str> = s.last_plays.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, vec![
        "Rodríguez singles to right, Crawford to third",
        "Raleigh struck out swinging.",
        "Devers homered to right (24)",
    ], "pitches (P) and inning markers (I) are dropped; newest first");
    assert_eq!(s.last_plays[0].period, "T9");
    assert_eq!(s.last_plays[2].period, "B3");
    assert_eq!(s.scoring_plays.len(), 1);
    assert_eq!(s.scoring_plays[0].team, "BOS");
}

#[test]
fn summary_is_not_truncated_to_eight() {
    // 20 flat plays with the scoring play at index 3 — the old split_off(len-8)
    // dropped it and every scoring surface went blank (review finding #2).
    let mut plays = String::new();
    for i in 0..20 {
        if i > 0 { plays.push(','); }
        let scoring = i == 3;
        plays.push_str(&format!(r#"{{"text":"play {i}","scoringPlay":{scoring},"team":{{"id":"1"}},"clock":{{"displayValue":"{}:00"}}}}"#, 12 - (i % 12)));
    }
    let json = format!(r#"{{"header":{{"competitions":[{{"competitors":[{{"team":{{"id":"1","abbreviation":"DEN"}}}}]}}]}},"plays":[{plays}]}}"#);
    let s = map_summary(&json).unwrap();
    assert_eq!(s.last_plays.len(), 20);
    assert_eq!(s.scoring_plays.len(), 1);
    assert_eq!(s.scoring_plays[0].text, "play 3");
    assert!(s.last_plays.iter().any(|p| p.scoring && p.text == "play 3"));
}

#[test]
fn grouped_box_score_maps_and_missing_leaders_is_empty_not_error() {
    let stats = map_stats(include_str!("../fixtures/mlb_summary_min.json")).unwrap();
    let hits = stats.rows.iter().find(|r| r.label == "H").expect("grouped stats flattened");
    assert_eq!((hits.away.as_str(), hits.home.as_str()), ("11", "9"));
    assert!(stats.leaders.is_empty());
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test map_espn mlb_summary grouped summary_is_not 2>&1 | tail -6`
Expected: three failures (truncation to 8, pitch rows present, `H` row missing).

- [ ] **Step 4: Implement** in `map_summary`:

```rust
    // Flat-array branch: replace the inner push with
    for p in events {
        let Some(text) = p["text"].as_str().filter(|t| !t.is_empty()) else { continue };
        // MLB tags rows: P pitch, N at-bat narrative, S scoring, I inning
        // marker, A batter/pitcher start, C substitution. Only N and S are
        // the feed a fan reads (verified 2026-08-31: 285 P vs 74 N + 11 S).
        if matches!(p["summaryType"].as_str(), Some("P") | Some("I") | Some("A") | Some("C")) {
            continue;
        }
        plays.push(Play {
            clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
            period: inning_tag(&p["period"]),
            team: team_of(p),
            text: text.to_string(),
            scoring: p["scoringPlay"].as_bool().unwrap_or(false),
        });
    }
    // DELETE:
    //   if plays.len() > 8 { plays = plays.split_off(plays.len() - 8); }
    // keep: plays.reverse();

/// `{"type":"Top","number":9}` -> "T9"; empty when the play has no period.
fn inning_tag(period: &Value) -> String {
    match (period["type"].as_str(), period["number"].as_u64()) {
        (Some(t), Some(n)) if !t.is_empty() => format!("{}{n}", &t[..1].to_uppercase()),
        _ => String::new(),
    }
}
```

Football drive plays also get `period: String::new()` (their clock is real). NHL: after building `plays`, tag power-play goals — `if p["strength"]["id"].as_str() == Some("702") && scoring { text = format!("PP · {text}") }` (verified id; see Global Constraints deviation note).

In `map_stats`, replace the per-side `statistics` read with a flattener:

```rust
/// Flat (`[{name, displayValue}]`) or grouped (`[{name, stats:[{name, displayValue}]}]`,
/// MLB) — both become (name, label, displayValue) triples.
fn stat_rows(side: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for s in side["statistics"].as_array().cloned().unwrap_or_default() {
        if let Some(group) = s["stats"].as_array() {
            for g in group {
                if let (Some(n), Some(v)) = (g["name"].as_str(), g["displayValue"].as_str()) {
                    out.push((n.to_string(), g["label"].as_str().unwrap_or(n).to_string(), v.to_string()));
                }
            }
        } else if let (Some(n), Some(v)) = (s["name"].as_str(), s["displayValue"].as_str()) {
            out.push((n.to_string(), s["label"].as_str().unwrap_or(n).to_string(), v.to_string()));
        }
    }
    out
}
```

and pair away/home by `name` as today. `leaders` already tolerates absence (`unwrap_or_default`) — confirm the test passes without change.

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS. `maps_summary_scoring_plays_newest_first` (existing) still passes — the NFL fixture has < 8 plays.

- [ ] **Step 6: Commit**

```bash
git add fixtures/mlb_summary_min.json src/provider/map.rs tests/map_espn.rs
git commit -m "feat(v3.1): summary mapper — full play list, MLB at-bat feed, grouped box score, inning tags, PP goal tag"
```

---

### Task 5: Provider — map-then-cache, fallback on map failure, timeouts, structured errors, ETag (spec §3)

**Files:**
- Modify: `src/provider/mod.rs` (`ProviderError`), `src/provider/espn.rs`
- Test: `src/provider/espn.rs` unit tests

**Interfaces:**
- Produces:

```rust
pub enum ProviderError {
    Http { status: u16, url: String, detail: String },   // status 0 = transport/timeout
    Map { key: String, source: map::MapError },
    Io(std::io::Error),
}
impl ProviderError { pub fn short(&self) -> String }  // "ESPN 403 nfl scoreboard" / "ESPN timeout mlb summary" / "ESPN bad body nba scoreboard"
pub struct EspnProvider { pub cache_dir: PathBuf, pub offset: UtcOffset, agent: ureq::Agent }
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const USER_AGENT: &str = concat!("gameday/", env!("CARGO_PKG_VERSION"), " (+https://github.com/WallyMagill/game-day)");
enum Fetched { Body { body: String, etag: Option<String> }, NotModified }
fn fetch_with<T>(&self, key: &str, http: impl FnOnce(Option<&str>) -> Result<Fetched, ProviderError>, map: impl Fn(&str) -> Result<T, ProviderError>) -> Result<(T, bool), ProviderError>
```

- [ ] **Step 1: Write the failing tests** — replace the three `http_or_cache` tests in `src/provider/espn.rs` with:

```rust
    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gd-espn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn provider(dir: &Path) -> EspnProvider { EspnProvider::new(dir.to_path_buf(), time::UtcOffset::UTC) }
    fn ok_map(s: &str) -> Result<String, ProviderError> {
        if s.starts_with('{') { Ok(s.to_string()) } else {
            Err(ProviderError::Map { key: "k".into(), source: crate::provider::map::MapError::Missing("events") })
        }
    }

    #[test]
    fn a_200_with_a_bad_body_serves_the_cache_and_does_not_poison_it() {
        let dir = tmp("poison");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"good\":1}").unwrap();
        let got = p.fetch_with("k", |_| Ok(Fetched::Body { body: "<html>blocked</html>".into(), etag: None }), ok_map).unwrap();
        assert_eq!(got, ("{\"good\":1}".to_string(), true), "cached payload, marked stale");
        assert_eq!(cache_read(&dir, "k").unwrap(), "{\"good\":1}", "disk untouched by the bad body");
    }

    #[test]
    fn a_good_body_is_cached_after_it_maps_and_is_fresh() {
        let dir = tmp("fresh");
        let p = provider(&dir);
        let got = p.fetch_with("k", |_| Ok(Fetched::Body { body: "{\"v\":2}".into(), etag: Some("\"abc\"".into()) }), ok_map).unwrap();
        assert_eq!(got, ("{\"v\":2}".to_string(), false));
        assert_eq!(cache_read(&dir, "k").unwrap(), "{\"v\":2}");
        assert_eq!(std::fs::read_to_string(dir.join("k.etag")).unwrap(), "\"abc\"");
    }

    #[test]
    fn not_modified_serves_the_cache_fresh_and_sends_the_stored_etag() {
        let dir = tmp("etag");
        let p = provider(&dir);
        cache_write(&dir, "k", "{\"v\":1}").unwrap();
        std::fs::write(dir.join("k.etag"), "\"abc\"").unwrap();
        let mut seen = None;
        let got = p.fetch_with("k", |etag| { seen = etag.map(str::to_string); Ok(Fetched::NotModified) }, ok_map).unwrap();
        assert_eq!(seen.as_deref(), Some("\"abc\""));
        assert_eq!(got, ("{\"v\":1}".to_string(), false), "304 = the cache IS current");
    }

    #[test]
    fn transport_error_without_cache_is_the_error_with_status_and_url() {
        let dir = tmp("noc");
        let p = provider(&dir);
        let err = p.fetch_with("k", |_| Err(ProviderError::Http { status: 403, url: "u".into(), detail: String::new() }), ok_map).unwrap_err();
        assert!(matches!(err, ProviderError::Http { status: 403, .. }));
        assert_eq!(err.short(), "ESPN 403 k");
    }

    #[test]
    fn user_agent_names_the_project_and_a_contact() {
        assert!(USER_AGENT.starts_with("gameday/"));
        assert!(USER_AGENT.contains("+https://"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib provider::espn 2>&1 | tail -5`
Expected: compile errors (`Fetched`, `fetch_with`, `ProviderError::Http { .. }` do not exist).

- [ ] **Step 3: Implement.** `src/provider/mod.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// `status` 0 means the request never got a response (DNS, TCP, timeout).
    #[error("http status={status} url={url} {detail}")]
    Http { status: u16, url: String, detail: String },
    #[error("map {key}: {source}")]
    Map { key: String, #[source] source: map::MapError },
    #[error("io {0}")]
    Io(#[from] std::io::Error),
}

impl ProviderError {
    /// One footer-sized phrase: what failed and where. `key` is the cache key
    /// (`nfl-scoreboard`), which reads as "league resource".
    pub fn short(&self) -> String {
        match self {
            ProviderError::Http { status: 0, url, detail } if detail.contains("timed out") || detail.contains("timeout") => {
                format!("ESPN timeout {}", key_of(url))
            }
            ProviderError::Http { status: 0, url, .. } => format!("ESPN unreachable {}", key_of(url)),
            ProviderError::Http { status, url, .. } => format!("ESPN {status} {}", key_of(url)),
            ProviderError::Map { key, .. } => format!("ESPN bad body {key}"),
            ProviderError::Io(e) => format!("disk {e}"),
        }
    }
}

/// "…/sports/football/nfl/scoreboard?x" -> "nfl scoreboard"; a bare key passes through.
fn key_of(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    match parts.as_slice() {
        [res, league] if !league.is_empty() && !url.contains("://") => format!("{league} {res}"),
        [res, league] => format!("{league} {res}"),
        _ => url.to_string(),
    }
}
```

`src/provider/espn.rs`:

```rust
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10); // guess: ESPN answers in <1s normally; 10s is "the socket is dead", not "slow"
pub const USER_AGENT: &str = concat!("gameday/", env!("CARGO_PKG_VERSION"), " (+https://github.com/WallyMagill/game-day)");

pub struct EspnProvider { pub cache_dir: PathBuf, pub offset: time::UtcOffset, agent: ureq::Agent }

pub(crate) enum Fetched { Body { body: String, etag: Option<String> }, NotModified }

impl EspnProvider {
    pub fn new(cache_dir: PathBuf, offset: time::UtcOffset) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(HTTP_TIMEOUT)
            .timeout_read(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build();
        Self { cache_dir, offset, agent }
    }

    fn http(&self, url: &str, etag: Option<&str>) -> Result<Fetched, ProviderError> {
        let mut req = self.agent.get(url).set("Accept", "application/json");
        if let Some(tag) = etag { req = req.set("If-None-Match", tag); }
        match req.call() {
            Ok(r) => {
                let etag = r.header("etag").map(str::to_string);
                let body = r.into_string().map_err(|e| ProviderError::Http { status: 0, url: url.into(), detail: format!("body read: {e}") })?;
                Ok(Fetched::Body { body, etag })
            }
            Err(ureq::Error::Status(304, _)) => Ok(Fetched::NotModified),
            Err(ureq::Error::Status(code, _)) => Err(ProviderError::Http { status: code, url: url.into(), detail: String::new() }),
            Err(e) => Err(ProviderError::Http { status: 0, url: url.into(), detail: e.to_string() }),
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
        let etag = std::fs::read_to_string(self.cache_dir.join(format!("{key}.etag"))).ok();
        let fresh_err = match http(etag.as_deref().filter(|_| cached.is_some())) {
            Ok(Fetched::Body { body, etag }) => match map(&body) {
                Ok(v) => {
                    cache_write(&self.cache_dir, key, &body)?;
                    match etag {
                        Some(t) => std::fs::write(self.cache_dir.join(format!("{key}.etag")), t)?,
                        None => { let _ = std::fs::remove_file(self.cache_dir.join(format!("{key}.etag"))); }
                    }
                    return Ok((v, false));
                }
                Err(e) => e,
            },
            Ok(Fetched::NotModified) => match &cached {
                Some(body) => return map(body).map(|v| (v, false)),
                None => ProviderError::Http { status: 304, url: key.into(), detail: "304 with no cache".into() },
            },
            Err(e) => e,
        };
        match cached {
            Some(body) => map(&body).map(|v| (v, true)).map_err(|_| fresh_err),
            None => Err(fresh_err),
        }
    }

    fn fetch<T>(&self, url: &str, key: &str, map: impl Fn(&str) -> Result<T, ProviderError>) -> Result<(T, bool), ProviderError> {
        self.fetch_with(key, |etag| self.http(url, etag), map)
    }
}
```

Wrap each `map_*` closure's error as `ProviderError::Map { key: key.to_string(), source }` instead of `.map_err(Into::into)` (remove the `#[from]` on `Map`). Delete `http_or_cache` and `http_get`. `backoff_secs` moves to `poll.rs` in Task 6 — leave it for now.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib provider 2>&1 | tail -4 && cargo build --all-targets 2>&1 | grep -E '^error' | head`
Expected: provider tests PASS; fix any caller of `EspnProvider::new` (main.rs ×2) to pass an offset; fix `ProviderError::Http(String)` matchers in `memory.rs` (use the struct form with `status: 0`).

- [ ] **Step 5: Commit**

```bash
git add src/provider src/main.rs
git commit -m "feat(v3.1): provider maps before it caches, falls back on bad bodies, 10s timeouts, ETag, honest UA and errors"
```

---

### Task 6: `Scheduler` in `poll.rs` replaces the inline cadence in `main.rs` (spec §3 budget)

**Files:**
- Modify: `src/poll.rs` (rewrite; keep `PollPlan` only as the input snapshot), `src/main.rs` (`poll_loop`), `src/provider/espn.rs` (delete `backoff_secs`)
- Test: `tests/poll.rs` (rewrite), `src/poll.rs` unit tests

**Interfaces:**
- Produces:

```rust
pub const SCOREBOARD_LIVE: Duration = Duration::from_secs(15);   // ESPN max-age=5 measured 2026-08-31; 15s = 3× their cache window
pub const SCOREBOARD_IDLE: Duration = Duration::from_secs(60);
pub const SUMMARY_EVERY: Duration = Duration::from_secs(15);     // zoomed game only
pub const STATS_EVERY: Duration = Duration::from_secs(30);
pub const STANDINGS_TTL: Duration = Duration::from_secs(600);
pub const DATED_RETRY: Duration = Duration::from_secs(15);
pub const BACKOFF_BASE: Duration = Duration::from_secs(5);       // guess (no Retry-After from ESPN), doubles per failure, capped
pub const BACKOFF_CAP: Duration = Duration::from_secs(300);
pub const JITTER_PCT: u64 = 20;                                  // ±20%: enough to de-synchronize many clients, small enough to keep the cadence readable

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request { Scoreboard(League), Summary(League, String), Stats(League, String), Dated(League, time::Date), Standings(League) }

/// What the UI wants right now — a snapshot the UI thread publishes.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Wants {
    pub leagues: Vec<League>,
    pub any_live: bool,
    pub zoomed: Option<(League, String)>,
    pub dated: Option<(League, time::Date)>,
    pub standings: Option<League>,
    pub refresh_now: bool,
}

pub struct Scheduler { /* per-league next_due + attempt, last summary/stats/standings/dated, seed */ }
impl Scheduler {
    pub fn new(seed: u64) -> Self;
    /// Requests due at `now`, staggered so N leagues never fire in one burst.
    pub fn due(&mut self, wants: &Wants, now: Instant) -> Vec<Request>;
    /// Record an outcome; failures back the league off with jitter.
    pub fn report(&mut self, req: &Request, ok: bool, now: Instant);
    pub fn next_retry(&self, league: League, now: Instant) -> Option<Duration>;
}
```

- [ ] **Step 1: Write the failing tests** — replace `tests/poll.rs` with:

```rust
use gameday::domain::League;
use gameday::poll::*;
use std::time::{Duration, Instant};

fn wants(leagues: &[League], any_live: bool) -> Wants {
    Wants { leagues: leagues.to_vec(), any_live, ..Default::default() }
}

#[test]
fn scoreboards_are_staggered_not_burst() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let w = wants(&League::ALL, true);
    let first = s.due(&w, t0);
    assert_eq!(first.len(), 1, "one league per tick on a cold start, got {first:?}");
    // Across one live window every league gets exactly one scoreboard.
    let mut count = std::collections::HashMap::new();
    for i in 0..(SCOREBOARD_LIVE.as_millis() / 200) {
        for r in s.due(&w, t0 + Duration::from_millis(200 * i as u64)) {
            if let Request::Scoreboard(l) = &r { *count.entry(*l).or_insert(0) += 1; }
            s.report(&r, true, t0 + Duration::from_millis(200 * i as u64));
        }
    }
    for l in League::ALL { assert_eq!(count.get(&l), Some(&1), "{} fetched once per window", l.slug()); }
}

#[test]
fn summary_and_stats_only_for_the_zoomed_game() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Mlb], true);
    // No zoom: never a summary, whatever is live.
    let reqs: Vec<Request> = (0..200).flat_map(|i| s.due(&w, t0 + Duration::from_millis(200 * i))).collect();
    assert!(!reqs.iter().any(|r| matches!(r, Request::Summary(..) | Request::Stats(..))), "{reqs:?}");
    w.zoomed = Some((League::Mlb, "401".into()));
    let reqs = s.due(&w, t0 + Duration::from_secs(60));
    assert!(reqs.contains(&Request::Summary(League::Mlb, "401".into())));
    assert!(reqs.contains(&Request::Stats(League::Mlb, "401".into())), "a fresh zoom fetches stats at once");
}

#[test]
fn ten_minutes_of_nine_live_leagues_and_one_zoom_stays_under_budget() {
    let mut s = Scheduler::new(7);
    let t0 = Instant::now();
    let mut w = wants(&League::ALL, true);
    w.zoomed = Some((League::Nfl, "1".into()));
    let mut n = 0usize;
    for i in 0..(600 * 5) {
        let now = t0 + Duration::from_millis(200 * i);
        for r in s.due(&w, now) { n += 1; s.report(&r, true, now); }
    }
    // 9 leagues / 15s = 36/min, summary 4/min, stats 2/min => 42/min => 420 in 10 min (+jitter slack).
    assert!(n <= 440, "{n} requests in 10 minutes, budget 420 (+5% jitter slack)");
    assert!(n >= 380, "{n} — suspiciously few; the scheduler is starving something");
}

#[test]
fn a_failing_league_backs_off_alone_with_jitter_and_caps() {
    let mut s = Scheduler::new(3);
    let t0 = Instant::now();
    let w = wants(&[League::Nfl, League::Nba], true);
    let _ = s.due(&w, t0);
    s.report(&Request::Scoreboard(League::Nfl), false, t0);
    let r1 = s.next_retry(League::Nfl, t0).unwrap();
    assert!(r1 >= BACKOFF_BASE * 80 / 100 && r1 <= BACKOFF_BASE * 120 / 100, "first retry ≈5s ±20%, got {r1:?}");
    assert!(s.next_retry(League::Nba, t0).is_none(), "NBA is not backed off by NFL's failure");
    for _ in 0..10 { s.report(&Request::Scoreboard(League::Nfl), false, t0); }
    assert!(s.next_retry(League::Nfl, t0).unwrap() <= BACKOFF_CAP * 120 / 100);
    s.report(&Request::Scoreboard(League::Nfl), true, t0);
    assert!(s.next_retry(League::Nfl, t0).is_none(), "success clears the backoff");
}

#[test]
fn refresh_now_makes_every_scoreboard_due_at_once() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Nfl, League::Nba, League::Mlb], false);
    let _ = s.due(&w, t0);
    w.refresh_now = true;
    let reqs = s.due(&w, t0 + Duration::from_millis(200));
    assert_eq!(reqs.iter().filter(|r| matches!(r, Request::Scoreboard(_))).count(), 3);
}

#[test]
fn idle_cadence_is_slow_and_dated_and_standings_fire_on_target_change() {
    let mut s = Scheduler::new(1);
    let t0 = Instant::now();
    let mut w = wants(&[League::Cfb], false);
    let _ = s.due(&w, t0);
    assert!(s.due(&w, t0 + Duration::from_secs(30)).is_empty(), "idle: nothing due at 30s");
    assert_eq!(s.due(&w, t0 + Duration::from_secs(61)), vec![Request::Scoreboard(League::Cfb)]);
    let d = time::Date::from_calendar_date(2026, time::Month::August, 29).unwrap();
    w.dated = Some((League::Cfb, d));
    w.standings = Some(League::Cfb);
    let reqs = s.due(&w, t0 + Duration::from_secs(62));
    assert!(reqs.contains(&Request::Dated(League::Cfb, d)));
    assert!(reqs.contains(&Request::Standings(League::Cfb)));
    assert!(s.due(&w, t0 + Duration::from_secs(63)).is_empty(), "same targets don't refire inside their windows");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test poll 2>&1 | tail -3`
Expected: compile errors (`Scheduler`, `Wants`, `Request` missing).

- [ ] **Step 3: Implement `src/poll.rs`** (replace the file):

```rust
//! Request scheduler for the poll thread. Pure: `due` and `report` take the
//! clock as a parameter, so the budget is testable without sleeping.
use crate::domain::League;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const SCOREBOARD_LIVE: Duration = Duration::from_secs(15);
pub const SCOREBOARD_IDLE: Duration = Duration::from_secs(60);
pub const SUMMARY_EVERY: Duration = Duration::from_secs(15);
pub const STATS_EVERY: Duration = Duration::from_secs(30);
pub const STANDINGS_TTL: Duration = Duration::from_secs(600);
pub const DATED_RETRY: Duration = Duration::from_secs(15);
pub const BACKOFF_BASE: Duration = Duration::from_secs(5);
pub const BACKOFF_CAP: Duration = Duration::from_secs(300);
pub const JITTER_PCT: u64 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Scoreboard(League),
    Summary(League, String),
    Stats(League, String),
    Dated(League, time::Date),
    Standings(League),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Wants {
    pub leagues: Vec<League>,
    pub any_live: bool,
    pub zoomed: Option<(League, String)>,
    pub dated: Option<(League, time::Date)>,
    pub standings: Option<League>,
    pub refresh_now: bool,
}

#[derive(Default)]
struct LeagueState {
    next_due: Option<Instant>,
    attempt: u32,
}

pub struct Scheduler {
    leagues: HashMap<League, LeagueState>,
    last_summary: Option<(String, Instant)>,
    last_stats: Option<(String, Instant)>,
    last_dated: Option<((League, time::Date), Instant)>,
    last_standings: Option<(League, Instant)>,
    rng: u64,
}

impl Scheduler {
    pub fn new(seed: u64) -> Self {
        Self { leagues: HashMap::new(), last_summary: None, last_stats: None, last_dated: None, last_standings: None, rng: seed.max(1) }
    }

    /// xorshift64 — deterministic jitter so tests can pin the budget.
    fn jitter(&mut self, d: Duration) -> Duration {
        self.rng ^= self.rng << 13; self.rng ^= self.rng >> 7; self.rng ^= self.rng << 17;
        let span = d.as_millis() as u64 * JITTER_PCT / 100;
        let off = (self.rng % (2 * span + 1)) as i64 - span as i64;
        Duration::from_millis((d.as_millis() as i64 + off).max(0) as u64)
    }

    pub fn due(&mut self, wants: &Wants, now: Instant) -> Vec<Request> {
        let mut out = Vec::new();
        let every = if wants.any_live { SCOREBOARD_LIVE } else { SCOREBOARD_IDLE };
        // Stagger: a league that has never been scheduled gets its first slot
        // i*every/n after `now`, so a cold start (or :config enabling nine
        // leagues) spreads across the window instead of firing nine at once.
        let n = wants.leagues.len().max(1) as u32;
        for (i, league) in wants.leagues.iter().enumerate() {
            let st = self.leagues.entry(*league).or_default();
            let slot = st.next_due.get_or_insert_with(|| now + every * i as u32 / n);
            if wants.refresh_now || *slot <= now {
                out.push(Request::Scoreboard(*league));
                st.next_due = Some(now + every); // provisional; report() re-jitters
            }
        }
        self.leagues.retain(|l, _| wants.leagues.contains(l));
        if let Some((league, id)) = &wants.zoomed {
            let fresh = |last: &Option<(String, Instant)>, every: Duration| {
                last.as_ref().is_some_and(|(lid, t)| lid == id && now.duration_since(*t) < every)
            };
            if !fresh(&self.last_summary, SUMMARY_EVERY) {
                out.push(Request::Summary(*league, id.clone()));
                self.last_summary = Some((id.clone(), now));
            }
            if !fresh(&self.last_stats, STATS_EVERY) {
                out.push(Request::Stats(*league, id.clone()));
                self.last_stats = Some((id.clone(), now));
            }
        }
        if let Some(target) = wants.dated {
            let fresh = self.last_dated.is_some_and(|(t, at)| t == target && now.duration_since(at) < DATED_RETRY);
            if !fresh { out.push(Request::Dated(target.0, target.1)); self.last_dated = Some((target, now)); }
        }
        if let Some(league) = wants.standings {
            let fresh = self.last_standings.is_some_and(|(l, at)| l == league && now.duration_since(at) < STANDINGS_TTL);
            if !fresh { out.push(Request::Standings(league)); self.last_standings = Some((league, now)); }
        }
        out
    }

    pub fn report(&mut self, req: &Request, ok: bool, now: Instant) {
        let Request::Scoreboard(league) = req else { return };
        let base = if ok { None } else {
            let st = self.leagues.entry(*league).or_default();
            st.attempt = st.attempt.saturating_add(1);
            Some((BACKOFF_BASE * 2u32.saturating_pow(st.attempt - 1)).min(BACKOFF_CAP))
        };
        match base {
            Some(b) => { let j = self.jitter(b); if let Some(st) = self.leagues.get_mut(league) { st.next_due = Some(now + j); } }
            None => if let Some(st) = self.leagues.get_mut(league) {
                st.attempt = 0;
                // re-jitter the provisional slot set in due()
                if let Some(slot) = st.next_due { let every = slot.saturating_duration_since(now); st.next_due = Some(now + self.jitter(every)); }
            },
        }
    }

    pub fn next_retry(&self, league: League, now: Instant) -> Option<Duration> {
        let st = self.leagues.get(&league)?;
        (st.attempt > 0).then(|| st.next_due.map(|d| d.saturating_duration_since(now)).unwrap_or_default())
    }
}
```

Note the borrow in `report`'s success arm: compute `every` first, then call `self.jitter`, then write back (split into two statements to satisfy the borrow checker).

- [ ] **Step 4: Rewrite `poll_loop` in `src/main.rs`** to consume the scheduler. Replace the five `Arc<Mutex<…>>` targets + `LeaguesShared` + `refresh` flag with one `WantsShared = Arc<Mutex<Wants>>` the UI publishes:

```rust
type WantsShared = Arc<Mutex<gameday::poll::Wants>>;

fn poll_loop(provider: EspnProvider, tx: mpsc::Sender<Msg>, wants: WantsShared) {
    use gameday::poll::{Request, Scheduler};
    let mut sched = Scheduler::new(std::process::id() as u64);
    loop {
        let w = wants.lock().map(|w| w.clone()).unwrap_or_default();
        let now = Instant::now();
        for req in sched.due(&w, now) {
            let ok = match &req {
                Request::Scoreboard(league) => match provider.scoreboard(*league) {
                    Ok((games, stale)) => { let _ = tx.send(Msg::Boards { league: *league, games, stale }); true }
                    Err(e) => { let _ = tx.send(Msg::Failed { league: *league, error: e.short(), retry_in: sched.next_retry(*league, now) }); false }
                },
                Request::Summary(league, id) => match provider.summary(*league, id) {
                    Ok((s, _)) => { let _ = tx.send(Msg::Summary { id: id.clone(), summary: s }); true }
                    Err(_) => false,
                },
                Request::Stats(league, id) => match provider.stats(*league, id) {
                    Ok((s, _)) => { let _ = tx.send(Msg::Stats { id: id.clone(), stats: s }); true }
                    Err(_) => false,
                },
                Request::Dated(league, date) => match provider.scoreboard_on(*league, *date) {
                    Ok((games, _)) => { let _ = tx.send(Msg::DatedBoards { league: *league, date: *date, games }); true }
                    Err(_) => false,
                },
                Request::Standings(league) => match provider.standings(*league) {
                    Ok((t, _)) => { let _ = tx.send(Msg::Standings(t)); true }
                    Err(_) => false,
                },
            };
            sched.report(&req, ok, Instant::now());
        }
        if w.refresh_now { if let Ok(mut w) = wants.lock() { w.refresh_now = false; } }
        thread::sleep(Duration::from_millis(200));
    }
}
```

Add `Msg::Failed { league: League, error: String, retry_in: Option<Duration> }` (Task 10 consumes it; until then `run_ui` matches it with `app.note_failure(league, error, retry_in)` — add that method now as a stub that stores into a new `pub last_failure: Option<(League, String, Option<Duration>)>` field on `App`). In `run_ui`, replace the four "handshake" blocks with one: build `Wants { leagues: app.config.enabled_tabs.clone(), any_live: app.any_live(), zoomed: app.stats_target(), dated: app.dated_target(), standings: app.standings_target(), refresh_now: app.refresh_now }`; if it differs from the last published value, write it under the lock; clear `app.refresh_now` after publishing. Delete `merge_live_ids`, `prune_live_ids`, `leagues_changed`, `board_due`, `dated_due`, `DATED_RETRY`, the `StatsTarget/StandingsTarget/DatedTarget/LeaguesShared` types and their tests; delete `App::poll_plan` and `visible_for_poll`; delete `backoff_secs` from `espn.rs` and move `STANDINGS_TTL` to `poll.rs` (the provider's fresh-cache short-circuit imports it from there).

- [ ] **Step 5: Run everything**

Run: `cargo test 2>&1 | tail -4 && cargo clippy --all-targets 2>&1 | grep -c '^warning'`
Expected: PASS; no new warnings. Then a 60-second real run: `timeout 60 cargo run --release 2>/tmp/gd.err; grep -c . /tmp/gd.err` — stderr must be empty (no skipped-event lines on a healthy feed).

- [ ] **Step 6: Commit**

```bash
git add src/poll.rs src/main.rs src/app.rs src/provider/espn.rs tests/poll.rs
git commit -m "feat(v3.1): Scheduler — staggered per-league cadence, jittered backoff, summary/stats for the zoomed game only, 42 req/min budget"
```

---

### Task 7: Home shows everything live; pins and favorites sort first (spec §4, Decision 1)

**Files:**
- Modify: `src/home.rs`, `src/views/board.rs:183-191` (empty message), `src/app.rs` test `home_shows_only_pinned` (rename/re-assert)
- Test: `tests/home.rs`

**Interfaces:**
- `pub fn home_games<'a>(pins, favorites, boards, now) -> Vec<&'a Game>` — same signature, new contract: pinned (surviving prune) → favorites' games → every other **live** game, each band in `boards` order (which is `enabled_tabs` order). Pre/Final games appear only via pin or favorite.

- [ ] **Step 1: Write the failing tests** — append to `tests/home.rs`:

```rust
#[test]
fn home_shows_every_live_game_when_nothing_is_pinned() {
    let boards = vec![game("1", "SEA", Status::Live), game("2", "NYY", Status::Pre), game("3", "SD", Status::Live), game("4", "SF", Status::Final)];
    let out = home_games(&[], &[], &boards, OffsetDateTime::now_utc());
    let ids: Vec<&str> = out.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, vec!["1", "3"], "live games in board order; pre/final stay off Home unless pinned/favorited");
}

#[test]
fn pins_then_favorites_then_the_rest_of_the_live_slate() {
    let boards = vec![game("1", "SEA", Status::Live), game("2", "NYY", Status::Pre), game("3", "SD", Status::Live), game("4", "KC", Status::Live)];
    let pins = [Pin { game_id: "3".into(), league: League::Nfl, final_at: None }];
    let favs = [Favorite { league: League::Nfl, team_abbr: "NYY".into() }];
    let out = home_games(&pins, &favs, &boards, OffsetDateTime::now_utc());
    let ids: Vec<&str> = out.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, vec!["3", "2", "1", "4"], "pin, then favorite (even pre-game), then live in board order");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test home 2>&1 | tail -4`
Expected: `home_shows_every_live_game_when_nothing_is_pinned` fails (empty vec).

- [ ] **Step 3: Implement** — at the end of `home_games`, before `out`:

```rust
    // Everything else that is live: Home is the room with every TV on;
    // pins and favorites only decide which TV is in front (spec §4).
    for g in boards {
        if g.status == Status::Live && !out.iter().any(|x| x.id == g.id) {
            out.push(g);
        }
    }
```

(import `crate::domain::Status`). In `src/views/board.rs:183-191` the Home-empty branch now means *nothing live anywhere*; change the copy to name the next start across enabled boards:

```rust
        Tab::Home if games.is_empty() => {
            let next = app.next_start(); // Option<(Game)> — smallest future `start` across boards
            let msg = match next {
                Some(g) => format!("nothing live · next: {} @ {} {}", g.away.abbr, g.home.abbr,
                    crate::text::fmt_start(g.start.unwrap(), app.now())),
                None => "nothing live on the enabled boards · :config to add leagues".to_string(),
            };
```

Add `App::next_start(&self) -> Option<Game>` (min by `start` over `concat_boards()` where `status == Pre && start > now`) and `App::now()` (Task 9 adds the override; for now `OffsetDateTime::now_utc().to_offset(self.offset)` with `pub offset: UtcOffset` added to `App::new`'s callers — `App::new(config, pins, dir, offset)`). Rename the app test `home_shows_only_pinned` → `home_shows_pinned_first_then_live` and update its assertion to the new order.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/home.rs src/views/board.rs src/app.rs src/main.rs src/dump.rs tests/home.rs
git commit -m "feat(v3.1): Home is every live game; pins and favorites sort first; empty Home names the next start"
```

---

### Task 8: Scoring plays flow end to end (spec §1 `scoring_plays`, review finding #2)

**Files:**
- Modify: `src/app.rs` (`apply_boards`, `merge_summary`, `scoring_events`), `src/tiles/mod.rs:644` (zoom SCORING reads `scoring_plays`), `src/views/plays_feed.rs` (reads `scoring_events`, verify), `src/ticker.rs` (unchanged; consumes `scoring_events`)
- Test: `src/app.rs` unit tests

**Interfaces:**
- `App::apply_boards` carries `scoring_plays` across board replacements and appends the incoming game's `last_plays[0]` (marked `scoring: true`) when `(away_score, home_score)` changed vs `last_scores`.
- `App::merge_summary(id, summary)`: sets `game.last_plays = summary.last_plays` (full list) **and** `game.scoring_plays = summary.scoring_plays` when non-empty; an empty summary leaves both alone.
- `App::scoring_events() -> Vec<(Game, Play)>` reads `game.scoring_plays` (newest first) for every game on every enabled board, live **or final** (a final's scoring plays are still that game's story until it leaves the board).

- [ ] **Step 1: Write the failing tests** — in `src/app.rs` `mod tests`:

```rust
    #[test]
    fn a_score_delta_captures_the_scoreboard_last_play_as_a_scoring_play() {
        let mut app = app_with(vec![], vec![]);
        let mut g1 = g("1", "SEA", "BOS", true);
        g1.away_score = 7; g1.home_score = 7;
        g1.last_plays = vec![Play { text: "Raleigh flies out".into(), team: "SEA".into(), ..Default::default() }];
        app.apply_boards(League::Nfl, vec![g1.clone()], false);
        assert!(app.scoring_events().is_empty(), "first sighting seeds silently");
        let mut g2 = g1.clone();
        g2.away_score = 8;
        g2.last_plays = vec![Play { text: "Rodríguez homers to left (18)".into(), team: "SEA".into(), ..Default::default() }];
        app.apply_boards(League::Nfl, vec![g2], false);
        let ev = app.scoring_events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].1.text, "Rodríguez homers to left (18)");
        assert!(ev[0].1.scoring);
        // The next poll (no delta) keeps it — boards are replaced wholesale.
        let mut g3 = g1.clone(); g3.away_score = 8;
        app.apply_boards(League::Nfl, vec![g3], false);
        assert_eq!(app.scoring_events().len(), 1, "carried across the board replacement");
    }

    #[test]
    fn summary_scoring_plays_replace_the_delta_derived_list_and_survive_truncation() {
        let mut app = app_with(vec![g("1", "SEA", "BOS", true)], vec![]);
        let plays: Vec<Play> = (0..20).map(|i| Play { text: format!("play {i}"), team: "SEA".into(), scoring: i == 3, ..Default::default() }).collect();
        let summary = Summary { last_plays: plays.clone(), scoring_plays: vec![plays[3].clone()], meter: None };
        app.merge_summary("1", summary);
        let ev = app.scoring_events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].1.text, "play 3");
        assert_eq!(app.game_by_id("1").unwrap().last_plays.len(), 20, "no 8-row truncation in the model");
    }

    #[test]
    fn final_games_keep_their_scoring_plays_on_the_board() {
        let mut app = app_with(vec![], vec![]);
        let mut g1 = g("1", "SEA", "BOS", true);
        g1.away_score = 0;
        app.apply_boards(League::Nfl, vec![g1.clone()], false);
        let mut g2 = g1.clone(); g2.away_score = 7;
        g2.last_plays = vec![Play { text: "TD".into(), team: "SEA".into(), ..Default::default() }];
        app.apply_boards(League::Nfl, vec![g2.clone()], false);
        let mut g3 = g2.clone(); g3.status = Status::Final;
        app.apply_boards(League::Nfl, vec![g3], false);
        assert_eq!(app.scoring_events().len(), 1, "a final's TD is still on the ticker");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::a_score_delta app::tests::summary_scoring app::tests::final_games 2>&1 | tail -6`
Expected: three failures.

- [ ] **Step 3: Implement** in `src/app.rs`:

```rust
    pub fn apply_boards(&mut self, league: League, mut games: Vec<Game>, stale: bool) {
        let now = OffsetDateTime::now_utc();
        let prev_board = self.boards.get(&league).cloned().unwrap_or_default();
        for g in &mut games {
            // Carry the accumulated scoring plays across the wholesale replace.
            if let Some(prev) = prev_board.iter().find(|p| p.id == g.id) {
                if g.scoring_plays.is_empty() { g.scoring_plays = prev.scoring_plays.clone(); }
            }
            let score = (g.away_score, g.home_score);
            if let Some(prev) = self.last_scores.get(&g.id) {
                if *prev != score {
                    self.flashes.insert(g.id.clone(), self.tick);
                    // The scoreboard's lastPlay at the moment the score moved
                    // IS the scoring play (spec §1); dedupe on text.
                    if let Some(p) = g.last_plays.first() {
                        if !g.scoring_plays.iter().any(|s| s.text == p.text) {
                            let mut p = p.clone();
                            p.scoring = true;
                            g.scoring_plays.push(p);
                        }
                    }
                }
            }
            self.last_scores.insert(g.id.clone(), score);
        }
        // … rest unchanged …
    }

    pub fn merge_summary(&mut self, game_id: &str, summary: Summary) {
        if summary.last_plays.is_empty() && summary.scoring_plays.is_empty() {
            return; // nothing to merge; keep the scoreboard's lastPlay
        }
        for board in self.boards.values_mut() {
            if let Some(game) = board.iter_mut().find(|g| g.id == game_id) {
                if !summary.last_plays.is_empty() {
                    let mut last_plays = summary.last_plays;
                    for play in &mut last_plays {
                        if summary.scoring_plays.iter().any(|s| s.text == play.text) { play.scoring = true; }
                    }
                    game.last_plays = last_plays;
                }
                if !summary.scoring_plays.is_empty() {
                    // Summary order is oldest-first for football, newest-first
                    // when derived; normalize to oldest-first here.
                    let mut sp = summary.scoring_plays.clone();
                    if sp.len() > 1 && summary.last_plays.first().is_some_and(|f| f.text == sp[0].text) { sp.reverse(); }
                    game.scoring_plays = sp;
                }
                return;
            }
        }
    }

    /// Scoring plays across every enabled board, newest first per game,
    /// games in board order. Finals keep theirs until they leave the board.
    pub(crate) fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let mut out = Vec::new();
        for game in self.concat_boards() {
            for play in game.scoring_plays.iter().rev() {
                out.push((game.clone(), play.clone()));
            }
        }
        out
    }
```

In `src/tiles/mod.rs:644` change `game.last_plays.iter().filter(|p| p.scoring)` to `game.scoring_plays.iter().rev()`. Check `src/views/plays_feed.rs` and `src/views/board.rs:52` — both already call `scoring_events()`; no change. The tile's LAST PLAYS list must cap what it *draws* (it used to rely on the 8-cap): in `render_lower_left`/`render_focus_body` take `.iter().take(rows_available)` — confirm by reading those functions; they already truncate lines to the area height.

- [ ] **Step 4: Run tests + look**

Run: `cargo test 2>&1 | tail -4 && GAMEDAY_DUMP_FONT=… cargo run --release -- dump --tick 15 >/dev/null && open out/board-broadcast.png`
Expected: PASS; the dump's GLOBAL ALERTS / TOP PLAYS / ticker ALERTS still show the demo's scoring plays (the demo seeds `scoring: true` plays in `last_plays` — if the sidebar went empty, make `demo.rs` seed `scoring_plays` too, same rows).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/tiles/mod.rs src/demo.rs
git commit -m "fix(v3.1): scoring plays live on Game — delta-captured for every game, summary-replaced for the zoomed one; alerts/top plays/ticker/zoom read them"
```

---

### Task 9: Render the new fields honestly — local times, inning tags, `2 OUT · 1-2`, record fallback, possession, pin/fav glyphs, `App::now` (spec §2, §4)

**Files:**
- Modify: `src/app.rs` (`now_override`, `now()`, `tile_fx`), `src/tiles/mod.rs` (`TileFx`, `situation_summary`, `play_line`, `render_name_row`, compact row, title), `src/views/board.rs` (`slate_line` takes `now`), `src/views/zoom.rs` (linescore row), `src/domain.rs` (`mlb_count_headline` text), `src/dump.rs` (`demo_app` sets `now_override`)
- Test: `tests/draw.rs`, `src/domain.rs`, `src/tiles/mod.rs` tests

**Interfaces:**
- `pub struct TileFx { pub flash: bool, pub live_bright: bool, pub pinned: bool, pub favorite: bool, pub now: OffsetDateTime }` — every tile render gets the frame's `now` through `TileFx` (no globals, dumps stay deterministic).
- `App { pub now_override: Option<OffsetDateTime>, pub offset: UtcOffset }`, `pub fn now(&self) -> OffsetDateTime`.
- `Situation::mlb_count_headline()` returns `"2 OUT · 1-2"` (singular/plural: `1 OUT`, `2 OUT` — the reference board style; the `·` is what separates count from score visually).

- [ ] **Step 1: Write the failing tests** — `src/domain.rs`:

```rust
        assert_eq!(sit(1, 2, 2).mlb_count_headline().as_deref(), Some("2 OUT · 1-2"));
        assert_eq!(sit(3, 2, 1).mlb_count_headline().as_deref(), Some("1 OUT · 3-2"));
        assert_eq!(sit(0, 0, 0).mlb_count_headline().as_deref(), Some("0 OUT · 0-0"));
```

`tests/draw.rs` (uses the file's `g` helper and `buf_text`):

```rust
#[test]
fn pregame_tile_and_slate_show_local_start_never_iso() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::from_hms(-4,0,0).unwrap());
    app.now_override = Some(time::macros::datetime!(2026-09-10 12:00 -4));
    let mut pre = g("1", "NE", "SEA", false);
    pre.start = Some(time::macros::datetime!(2026-09-10 20:20 -4));
    app.apply_boards(League::Nfl, vec![pre], false);
    app.tab = Tab::League(League::Nfl);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("8:20 PM"), "{text}");
    assert!(!text.contains("2026-"), "raw ISO leaked: {text}");
}

#[test]
fn pinned_and_favorited_tiles_carry_a_glyph_in_the_title() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(crossterm::event::KeyCode::Char(' '), crossterm::event::KeyModifiers::NONE);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("⚑"), "pin glyph missing: {text}");
    assert!(text.contains("pinned KC@TB"), "toast missing: {text}");
    app.on_key(crossterm::event::KeyCode::Char('t'), crossterm::event::KeyModifiers::NONE);
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("★"), "favorite glyph missing: {text}");
}

#[test]
fn baseball_play_rows_show_the_inning_not_a_dash_clock() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    let mut mlb = g("1", "SEA", "BOS", true);
    mlb.league = League::Mlb; mlb.period = "BOT 9TH".into(); mlb.clock = String::new();
    mlb.last_plays = vec![Play { period: "B9".into(), team: "SEA".into(), text: "Rodríguez singles".into(), ..Default::default() }];
    app.apply_boards(League::Mlb, vec![mlb], false);
    app.tab = Tab::League(League::Mlb);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("[B9]"), "{text}");
    assert!(!text.contains("[-:--]"), "{text}");
}

#[test]
fn long_names_keep_their_record_as_the_abbr_form() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    let mut game = g("1", "SEA", "BOS", true);
    game.away.name = "Mariners".into(); game.away.record = "64-73".into();
    game.home.name = "Red Sox".into(); game.home.record = "74-63".into();
    app.apply_boards(League::Nfl, vec![game; 4], false); // 2x2 => narrow tiles
    app.tab = Tab::League(League::Nfl);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("64-73") && text.contains("74-63"), "records dropped: {text}");
}
```

(Give the four cloned games distinct ids in the last test — `id: format!("{i}")` — or `last_scores` dedupes them.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test draw 2>&1 | tail -8`
Expected: compile error on `App::new` arity / `now_override`, then assertion failures.

- [ ] **Step 3: Implement.**
  1. `App`: add `pub now_override: Option<OffsetDateTime>` and `pub offset: UtcOffset`; `pub fn now(&self) -> OffsetDateTime { self.now_override.unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(self.offset)) }`. Header clock uses `crate::text::fmt_clock12(self.now())` and `date_label(self.now().date())`; `today_local()` becomes `self.now().date()`. `dump::demo_app` sets `now_override = Some(datetime!(2026-08-31 21:30:01 -4))` so captures are stable.
  2. `TileFx` gains `pinned`, `favorite`, `now`; `App::tile_fx(game)` fills them (`self.pins.iter().any(|p| p.game_id == game.id)`; favorite = either team in `config.favorites` for `game.league`).
  3. `tiles/mod.rs`: `situation_summary(game, now)` uses `fmt_start`; the tile title prepends `⚑ ` when `fx.pinned` and `★ ` when `fx.favorite` (after the `[LEAGUE]` chip, before `LIVE`); `play_line` uses `if p.clock.is_empty() { if p.period.is_empty() { "-:--" } else { &p.period } } else { &p.clock }` (same at line 650); `render_name_row` falls back per side to `format!("{} {}", t.abbr, t.record)` when the full name+record doesn't fit but abbr+record does, and only drops the record when neither fits; when `game.situation.possession == Some(abbr)` prefix that side's label with `▸ ` (away) / suffix `◂` (home).
  4. `views/board.rs::slate_line(game, now)`.
  5. `views/zoom.rs::draw_overview`: when `!game.linescore.is_empty()`, render one row under the tile: `   1  2  3  4  5  6  7  8  9   R` header + two rows `SEA 1 0 0 3 …  8` / `BOS …  7` using `game.away.abbr`/`home.abbr` and the per-period pairs (period headers are `1..=n`; for MLB also append `H E` from `Extras::Baseball`). Keep it a `Paragraph` of three `Line`s placed at the bottom of the identity block's rect; if `area.height < 20` skip it.
  6. `toggle_pin`/`toggle_favorite`: set `self.status_line = Some(format!("pinned {}@{}", …))` / `"unpinned …"` / `"favorited {league} {abbr}"` / `"unfavorited …"`. `cycle_theme`: `status_line = Some(format!("theme {next}"))`.
  7. `domain.rs::mlb_count_headline`: `format!("{o} OUT · {b}-{s}")`; update the map test at `tests/map_espn.rs:95` to `"2 OUT · 4-2"`.

- [ ] **Step 4: Run tests + look at the gallery**

Run: `cargo test 2>&1 | tail -4 && GAMEDAY_DUMP_FONT=… cargo run --release -- dump >/dev/null && open out/tab-nfl.png out/focus.png`
Expected: PASS; slate shows `8:20 PM`, zoom shows a linescore row, no `[-:--]` anywhere in `out/*.txt`-equivalent (`grep -l -- '-:--' out/*.ansi` returns nothing).

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat(v3.1): local start times everywhere, inning tags, count that isn't a score, record fallback, possession mark, pin/fav glyphs + toasts, deterministic App::now"
```

---

### Task 10: Failure visibility — `NetStatus` chip, frozen UPD, offline board message (spec §5)

**Files:**
- Create: `src/app/net.rs` (if Task 17 hasn't split `app.rs` yet, create `src/net.rs` and re-export; Task 17 moves it)
- Modify: `src/app.rs` (state, `apply_boards`, `note_failure`, header, footer), `src/views/board.rs` (offline message), `src/main.rs` (`Msg::Failed` → `note_failure`)
- Test: `src/net.rs` unit tests; `tests/draw.rs`

**Interfaces:**

```rust
pub enum NetChip { NoDataYet, Live, Stale { age: Duration }, Offline { retry_in: Option<Duration>, error: String } }
#[derive(Default)]
pub struct NetStatus { last_ok: Option<Instant>, last_err: Option<(Instant, String, Option<Duration>)>, ever_ok: bool }
impl NetStatus {
    pub fn ok(&mut self, now: Instant, stale: bool);           // a board applied (stale=true means served from cache)
    pub fn failed(&mut self, now: Instant, error: String, retry_in: Option<Duration>);
    pub fn chip(&self, now: Instant) -> NetChip;
    pub fn upd_label(&self, now: Instant) -> Option<String>;   // "UPD 12s" while Live; frozen "UPD 4m ·" while Stale/Offline; None before first data
}
pub const STALE_AFTER: Duration = Duration::from_secs(45);     // 3× the live scoreboard cadence: one missed poll is noise, three is a problem
```

Rules: `chip` = `NoDataYet` until the first `ok`; `Live` when the last event was an `ok(stale=false)` within `STALE_AFTER`; `Stale{age}` when the last fresh `ok` is older than `STALE_AFTER` or the last `ok` was `stale=true`; `Offline{..}` when a failure is more recent than the last `ok` **and** no fresh `ok` within `STALE_AFTER`.

- [ ] **Step 1: Write the failing tests** — `src/net.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn chip_walks_no_data_live_stale_offline_and_back() {
        let t0 = Instant::now();
        let mut n = NetStatus::default();
        assert!(matches!(n.chip(t0), NetChip::NoDataYet));
        assert_eq!(n.upd_label(t0), None);
        n.ok(t0, false);
        assert!(matches!(n.chip(t0 + Duration::from_secs(10)), NetChip::Live));
        assert_eq!(n.upd_label(t0 + Duration::from_secs(10)).as_deref(), Some("UPD 10s"));
        let later = t0 + STALE_AFTER + Duration::from_secs(1);
        assert!(matches!(n.chip(later), NetChip::Stale { .. }));
        assert_eq!(n.upd_label(later).as_deref(), Some("UPD 46s ·"), "frozen marker while stale");
        n.failed(later, "ESPN 403 nfl scoreboard".into(), Some(Duration::from_secs(40)));
        match n.chip(later) {
            NetChip::Offline { retry_in, error } => { assert_eq!(retry_in, Some(Duration::from_secs(40))); assert!(error.contains("403")); }
            other => panic!("{other:?}"),
        }
        n.ok(later + Duration::from_secs(5), false);
        assert!(matches!(n.chip(later + Duration::from_secs(6)), NetChip::Live), "recovery clears offline");
    }
    #[test]
    fn a_cached_apply_is_stale_immediately() {
        let t0 = Instant::now();
        let mut n = NetStatus::default();
        n.ok(t0, true);
        assert!(matches!(n.chip(t0), NetChip::Stale { .. }));
    }
}
```

`tests/draw.rs`:

```rust
#[test]
fn header_chip_is_its_own_cell_and_offline_names_the_error() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("NO DATA YET"), "{text}");
    app.note_failure(League::Nfl, "ESPN 403 nfl scoreboard".into(), Some(std::time::Duration::from_secs(40)));
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("OFFLINE"), "{text}");
    assert!(text.contains("ESPN 403 nfl scoreboard"), "board area names the error: {text}");
    assert!(text.contains("retry 40s"), "{text}");
    assert!(!text.contains("OFFLINEMON") && !text.contains("STALEMON"), "chip glued to the date: {text}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test net:: 2>&1 | tail -3; cargo test --test draw header_chip 2>&1 | tail -3`
Expected: compile errors.

- [ ] **Step 3: Implement** `src/net.rs` per the interface (straightforward state; `upd_label` uses the existing `age_label` logic moved here, appending ` ·` when not `Live`). Wire it: `App { pub net: NetStatus }`; `apply_boards` calls `self.net.ok(Instant::now(), stale)` and drops the old `self.stale` field and `last_update` (footer reads `net.upd_label`); `note_failure(league, error, retry_in)` calls `self.net.failed(...)` and sets `status_line` only if the board is empty. Header: replace the `if self.stale { " STALE" }` span with one chip span rendered **before** the date with two spaces of padding on both sides: `NO DATA YET` in `th.muted`, `LIVE` omitted (the tiles say it), `STALE 4m` in `th.star`, `OFFLINE · retry 40s` in `th.live`. Board: in `draw_mosaic`, when `games.is_empty()` and `matches!(app.net.chip(now), NetChip::Offline{..} | NetChip::NoDataYet)` render two centered lines: the chip text and `last error: <error>` / `waiting for the first scoreboard…`. Also in `apply_boards`, when `stale`, skip the flash/scoring-delta logic (a cached payload can't have a fresh delta).

- [ ] **Step 4: Run tests + a real offline check**

Run: `cargo test 2>&1 | tail -4`; then `HOME=$(mktemp -d) cargo run --release` with Wi-Fi off (or `sudo route add -host site.web.api.espn.com 127.0.0.1` if you'd rather not toggle Wi-Fi; remove it after) for 30 s, then `tmux capture-pane -p` — the header must read `OFFLINE · retry …` and the board must name the error. Then reconnect and confirm it flips to live within one cadence.
Expected: PASS; capture shows the chip.

- [ ] **Step 5: Commit**

```bash
git add src/net.rs src/lib.rs src/app.rs src/views/board.rs src/main.rs tests/draw.rs
git commit -m "feat(v3.1): NetStatus — NO DATA YET / STALE / OFFLINE chip, frozen UPD, offline board names the error and the retry"
```

---

### Task 11: CLI — `--help`, `--version`, `--config-dir`, unknown flags, no-TTY guard, panic hook (spec §5, §6)

**Files:**
- Modify: `src/main.rs` (`Args`, `parse_args`, `main`, `RestoreTerminal`)
- Test: `src/main.rs` unit tests

**Interfaces:**

```rust
struct Args { demo: bool, dump: bool, tick: u64, probe: Option<String>, help: bool, version: bool, config_dir: Option<PathBuf> }
fn parse_args(args: &[String]) -> Result<Args, String>;   // Err names the bad flag AND the valid set
const HELP: &str = "…";                                    // full text below
fn install_panic_hook();                                   // restores terminal, then prints
fn require_tty() -> Result<(), String>;                    // "gameday needs a terminal (stdout is not a tty); try --help"
```

- [ ] **Step 1: Write the failing tests** — in `src/main.rs` `mod tests`:

```rust
    #[test]
    fn help_and_version_flags_parse_and_unknown_flags_name_the_valid_set() {
        assert!(parsed(&["gameday", "--help"]).help);
        assert!(parsed(&["gameday", "-h"]).help);
        assert!(parsed(&["gameday", "--version"]).version);
        assert!(parsed(&["gameday", "-V"]).version);
        assert_eq!(parsed(&["gameday", "--config-dir", "/tmp/x"]).config_dir.as_deref(), Some(std::path::Path::new("/tmp/x")));
        let owned: Vec<String> = ["gameday", "--nonsense"].iter().map(|s| s.to_string()).collect();
        let err = parse_args(&owned).unwrap_err();
        assert!(err.contains("--nonsense") && err.contains("--demo") && err.contains("--help"), "{err}");
        let owned: Vec<String> = ["gameday", "nonsense"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&owned).is_err(), "bare unknown words are errors too");
        let owned: Vec<String> = ["gameday", "--config-dir"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&owned).unwrap_err().contains("--config-dir expects a path"));
    }

    #[test]
    fn help_text_lists_every_flag_and_the_dev_commands_under_their_own_heading() {
        for needle in ["--demo", "--help", "--version", "--config-dir", "dev:", "dump", "probe", "--tick", "~/.config/gameday"] {
            assert!(super::HELP.contains(needle), "HELP missing {needle}");
        }
        assert!(super::HELP.lines().count() < 30, "help must fit a small terminal");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --bin gameday help_and_version 2>&1 | tail -3`
Expected: compile error (`help` field missing).

- [ ] **Step 3: Implement.**

```rust
const HELP: &str = "\
gameday — terminal sports board. Pin games, they tile.

USAGE
  gameday                 live board (needs a terminal)
  gameday --demo          scripted demo slate, no network
  gameday --config-dir P  use P instead of ~/.config/gameday
  gameday -h, --help      this text
  gameday -V, --version   version

KEYS  space pin · enter/z zoom · j/k move · tab league · [ ] date · / filter · : command · ? all keys · q quit
CONFIG  ~/.config/gameday/config.toml (or $XDG_CONFIG_HOME/gameday); pins.json, cache/ and themes/ beside it
DATA  unofficial ESPN JSON, polled; the last good payload is kept on disk and shown as STALE when the network fails

dev:
  gameday dump [--tick N]   write the capture gallery to out/ (no network)
  gameday probe <league>    fetch + map one live scoreboard and print it
";

fn parse_args(args: &[String]) -> Result<Args, String> {
    const VALID: &str = "--demo|--help|-h|--version|-V|--config-dir <path>|dump [--tick N]|probe <league>";
    let mut a = Args { demo: false, dump: false, tick: 0, probe: None, help: false, version: false, config_dir: None };
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--demo" => a.demo = true,
            "--help" | "-h" => a.help = true,
            "--version" | "-V" => a.version = true,
            "dump" | "--dump" => a.dump = true,
            "--tick" => {
                let raw = it.next().map(String::as_str).unwrap_or("");
                a.tick = raw.parse().map_err(|_| format!("--tick expects a non-negative integer, got {raw:?}"))?;
            }
            "probe" => a.probe = Some(it.next().cloned().unwrap_or_default()),
            "--config-dir" => {
                let p = it.next().ok_or_else(|| "--config-dir expects a path".to_string())?;
                a.config_dir = Some(PathBuf::from(p));
            }
            other => return Err(format!("unknown argument {other:?}, valid: {VALID}")),
        }
    }
    Ok(a)
}
```

In `main`: after parsing, `if args.help { print!("{HELP}"); return Ok(()); }`, `if args.version { println!("gameday {}", env!("CARGO_PKG_VERSION")); return Ok(()); }`; before `run_ui` for the live/demo paths: `if let Err(e) = require_tty() { eprintln!("gameday: {e}"); std::process::exit(1); }` using `std::io::IsTerminal` (`std::io::stdout().is_terminal()`). Panic hook, installed at the top of `run_ui`:

```rust
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        eprintln!("\ngameday {} crashed — please file this with the lines below:", env!("CARGO_PKG_VERSION"));
        default(info);
    }));
}
```

Thread `args.config_dir` into Task 12's `config::resolve_dir`.

- [ ] **Step 4: Verify by hand**

Run: `cargo run --release -- --help | head -3; cargo run --release -- --version; cargo run --release -- --bogus; echo "exit=$?"; cargo run --release 2>&1 | head -1`
Expected: usage text; `gameday 0.1.0`; `unknown argument "--bogus", valid: …` with exit 2; `gameday needs a terminal (stdout is not a tty); try --help`.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(v3.1): --help/--version/--config-dir, unknown flags name the valid set, TTY guard, panic hook that restores the terminal first"
```

---

### Task 12: Config dir at `~/.config/gameday`, legacy fallback, parse errors never overwrite (spec §6, Decision 2)

**Files:**
- Modify: `src/config.rs`, `src/main.rs` (dir resolution + load), `src/app.rs` (`persist_config`/`persist_pins` helpers replacing every direct `save_to`/`save_pins`), `src/input.rs:188,194,198`, `src/views/config_view.rs` (any direct save), `README.md:24`
- Test: `tests/config.rs`, `src/app.rs`

**Interfaces:**

```rust
pub struct DirResolution { pub dir: PathBuf, pub legacy_read_from: Option<PathBuf> }
pub fn resolve_dir(override_dir: Option<PathBuf>, home: &Path, xdg: Option<&Path>, legacy: Option<&Path>) -> DirResolution;
pub struct LoadOutcome<T> { pub value: T, pub error: Option<String> }   // error: "config.toml:7: unknown league `NFLL`, valid: nfl|…"
pub fn load_config(dir: &DirResolution) -> LoadOutcome<Config>;
pub fn load_pins_outcome(dir: &DirResolution) -> LoadOutcome<Vec<Pin>>;
// App
pub config_error: Option<String>;   // Some => every persist_* is a no-op that re-arms the status line
fn persist_config(&mut self); fn persist_pins(&mut self);
```

- [ ] **Step 1: Write the failing tests** — `tests/config.rs`:

```rust
use gameday::config::{load_config, resolve_dir};
use std::path::Path;

#[test]
fn xdg_wins_then_dot_config_then_legacy_is_read_only() {
    let home = tmp("home");
    let legacy = home.join("Library/Application Support/gameday");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("config.toml"), "enabled_tabs = [\"Nfl\"]\nlayout = \"Auto\"\nfavorites = []\n").unwrap();
    // No ~/.config/gameday yet: dir is the new path, reads fall back to legacy.
    let r = resolve_dir(None, &home, None, Some(&legacy));
    assert_eq!(r.dir, home.join(".config/gameday"));
    assert_eq!(r.legacy_read_from.as_deref(), Some(legacy.as_path()));
    let out = load_config(&r);
    assert_eq!(out.value.enabled_tabs, vec![League::Nfl]);
    assert!(out.error.is_none());
    // XDG_CONFIG_HOME set: it wins outright.
    let xdg = home.join("xdg");
    let r = resolve_dir(None, &home, Some(&xdg), Some(&legacy));
    assert_eq!(r.dir, xdg.join("gameday"));
    // --config-dir beats everything and never consults legacy.
    let r = resolve_dir(Some(home.join("custom")), &home, Some(&xdg), Some(&legacy));
    assert_eq!(r.dir, home.join("custom"));
    assert!(r.legacy_read_from.is_none());
    fs::remove_dir_all(&home).ok();
}

#[test]
fn a_broken_config_loads_defaults_reports_the_line_and_is_never_overwritten() {
    let dir = tmp("broken");
    fs::write(dir.join("config.toml"), "enabled_tabs = [\"NFLL\"]\nlayout = \"Auto\"\nfavorites = []\n").unwrap();
    let r = resolve_dir(Some(dir.clone()), &dir, None, None);
    let out = load_config(&r);
    assert_eq!(out.value, Config::default_all(), "defaults in memory");
    let err = out.error.expect("error reported");
    assert!(err.contains("config.toml") && err.contains("NFLL") && err.contains("nfl|cfb"), "{err}");
    let before = fs::read_to_string(dir.join("config.toml")).unwrap();
    let mut app = gameday::app::App::new(out.value, vec![], dir.clone(), time::UtcOffset::UTC);
    app.config_error = out.error;
    app.on_key(crossterm::event::KeyCode::Char('2'), crossterm::event::KeyModifiers::NONE); // would save layout
    assert_eq!(fs::read_to_string(dir.join("config.toml")).unwrap(), before, "file untouched");
    assert!(app.status_line.as_deref().unwrap_or("").contains("not saving"), "{:?}", app.status_line);
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test config 2>&1 | tail -3`
Expected: compile errors.

- [ ] **Step 3: Implement** `src/config.rs`:

```rust
pub struct DirResolution { pub dir: PathBuf, pub legacy_read_from: Option<PathBuf> }

/// `--config-dir` > `$XDG_CONFIG_HOME/gameday` > `~/.config/gameday`. When
/// the chosen dir has no config.toml but the pre-v3 macOS location does,
/// reads come from there (once) and a note says where writes now go.
pub fn resolve_dir(override_dir: Option<PathBuf>, home: &Path, xdg: Option<&Path>, legacy: Option<&Path>) -> DirResolution {
    if let Some(dir) = override_dir {
        return DirResolution { dir, legacy_read_from: None };
    }
    let dir = xdg.map(|x| x.join("gameday")).unwrap_or_else(|| home.join(".config").join("gameday"));
    let legacy_read_from = match legacy {
        Some(l) if !dir.join("config.toml").exists() && l.join("config.toml").exists() => Some(l.to_path_buf()),
        _ => None,
    };
    DirResolution { dir, legacy_read_from }
}

pub struct LoadOutcome<T> { pub value: T, pub error: Option<String> }

pub fn load_config(r: &DirResolution) -> LoadOutcome<Config> {
    let read_dir = r.legacy_read_from.as_deref().unwrap_or(&r.dir);
    match Config::load_from(read_dir) {
        Ok(c) => LoadOutcome { value: c, error: None },
        Err(e) => LoadOutcome { value: Config::default_all(), error: Some(describe_toml_error(&read_dir.join("config.toml"), &e)) },
    }
}

fn describe_toml_error(path: &Path, e: &ConfigError) -> String {
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("config.toml");
    match e {
        ConfigError::TomlDe(te) => {
            let line = te.span().and_then(|s| std::fs::read_to_string(path).ok().map(|t| t[..s.start.min(t.len())].matches('\n').count() + 1));
            let msg = te.message();
            let hint = if msg.contains("League") || msg.contains("variant") {
                format!(" — valid leagues: {}", League::ALL.map(League::slug).join("|"))
            } else { String::new() };
            match line { Some(l) => format!("{file}:{l}: {msg}{hint}"), None => format!("{file}: {msg}{hint}") }
        }
        other => format!("{file}: {other}"),
    }
}
```

(`toml::de::Error::span()` and `.message()` exist in toml 0.8.) `load_pins_outcome` is the same shape over `load_pins`. In `main.rs`: `let r = resolve_dir(args.config_dir.clone(), &dirs::home_dir().unwrap_or_default(), std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).as_deref(), dirs::config_dir().map(|d| d.join("gameday")).as_deref());` then `if let Some(l) = &r.legacy_read_from { eprintln!("gameday: reading config from {} — gameday now writes to {}; move the folder to keep one copy", l.display(), r.dir.display()); }`. `App` gains `pub config_error: Option<String>`; add:

```rust
    fn persist_config(&mut self) {
        if let Some(err) = &self.config_error {
            self.status_line = Some(format!("not saving: {err}"));
            return;
        }
        if let Err(e) = self.config.save_to(&self.config_dir) {
            self.status_line = Some(format!("config save failed: {e}"));
        }
    }
```

and `persist_pins` likewise. Replace every `let _ = self.config.save_to(&self.config_dir)` / `save_pins(...)` in `app.rs`, `input.rs`, `views/config_view.rs` with these. Fix `README.md:24` to `~/.config/gameday/config.toml` (already says that — leave; the code now matches) and add one line: `Older installs on macOS: the app reads ~/Library/Application Support/gameday until you move it.`

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs src/app.rs src/input.rs src/views/config_view.rs tests/config.rs README.md
git commit -m "feat(v3.1): ~/.config/gameday on every platform with legacy read-fallback; a broken config is reported and never overwritten"
```

---

### Task 13: Keys and feedback — `q` in help, `[`/`]` in the keymap, handler→keymap coverage, stable tab order, layout keeps the game, completion list, scoped filter message, header clock never clipped (spec §4)

**Files:**
- Modify: `src/app.rs` (`on_key`, `config_toggle_tab`, `set_layout`, `draw_header`, `draw_footer`), `src/keymap.rs` (binding + test), `src/input.rs` (completion candidates), `src/views/board.rs:173-181` (filter message)
- Test: `src/keymap.rs`, `src/app.rs`, `tests/draw.rs`

- [ ] **Step 1: Write the failing tests.** `src/keymap.rs`:

```rust
    /// Every key the Board handler reacts to must be advertised somewhere.
    /// Drives a fresh App with one live game and diffs observable state.
    #[test]
    fn every_handled_board_key_is_a_binding() {
        use crate::app::App;
        use crate::config::Config;
        use crate::domain::*;
        use crossterm::event::{KeyCode, KeyModifiers};
        let mk = || {
            let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir().join(format!("gd-km-{}", std::process::id())), time::UtcOffset::UTC);
            let g = |id: &str| Game { id: id.into(), status: Status::Live, away: Team { abbr: "KC".into(), ..Default::default() }, home: Team { abbr: "TB".into(), ..Default::default() }, ..Default::default() };
            app.apply_boards(League::Nfl, vec![g("1"), g("2"), g("3"), g("4"), g("5")], false);
            app.tab = crate::app::Tab::League(League::Nfl);
            app
        };
        let snapshot = |a: &App| format!("{:?}|{}|{}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{:?}|{}|{:?}",
            a.tab, a.page, a.selected, a.pins.len(), a.view, a.help_open, a.should_quit, a.refresh_now,
            a.filter, a.config.layout, a.config.theme, a.config.favorites.len(), a.viewed_date_offset);
        let chord_for = |c: char| -> String { match c {
            ' ' => "SPC".into(), '[' | ']' => "[/]".into(), '?' => "?".into(), ':' => ":".into(), '/' => "/".into(),
            '1' | '2' | '4' => "1/2/4/S".into(), 's' => "1/2/4/S".into(),
            other => other.to_ascii_uppercase().to_string(),
        }};
        let mut unadvertised = vec![];
        for c in (b' '..=b'~').map(char::from) {
            let mut app = mk();
            let before = snapshot(&app);
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
            if snapshot(&app) == before { continue; }
            let chord = chord_for(c);
            let advertised = KEYMAP.iter().any(|b| b.keys.iter().any(|k| k.split('/').any(|part| part == chord) || *k == chord));
            if !advertised { unadvertised.push(c); }
        }
        assert!(unadvertised.is_empty(), "keys that change state but appear in no Binding: {unadvertised:?}");
    }
```

`src/app.rs` tests:

```rust
    #[test]
    fn q_inside_help_closes_help_not_the_app() {
        let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
        app.on_key(KeyCode::Char('?'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!app.help_open && !app.should_quit);
    }
    #[test]
    fn toggling_a_league_off_and_on_keeps_canonical_tab_order() {
        let mut app = app_with(vec![], vec![]);
        app.config_toggle_tab(League::Nfl);
        app.config_toggle_tab(League::Nfl);
        assert_eq!(app.config.enabled_tabs, League::ALL.to_vec());
    }
    #[test]
    fn layout_change_keeps_the_selected_game() {
        let mut app = app_with(six_live(), vec![]);
        app.tab = Tab::League(League::Nfl);
        app.selected = 4;
        app.set_layout(LayoutPref::One);
        assert_eq!(app.selected, 4);
        assert_eq!(app.page, 4, "page follows the selection under the new layout");
    }
    #[test]
    fn filter_miss_names_its_scope_and_the_ticker_match() {
        let mut app = app_with(vec![], vec![]);
        let mut sea = g("9", "SEA", "BOS", true); sea.league = League::Mlb;
        app.apply_boards(League::Mlb, vec![sea], false);
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.tab = Tab::League(League::Nfl);
        app.filter = Some("sea".into());
        assert_eq!(app.filter_miss_message(), "no games match \"sea\" on NFL · ticker matches SEA@BOS · esc clears");
    }
```

`tests/draw.rs`:

```rust
#[test]
fn header_keeps_the_clock_with_ten_chips_at_120_columns() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let first = buf_text(&term).lines().next().unwrap().to_string();
    assert!(first.contains("9:37:05 PM"), "clock clipped: {first}");
}

#[test]
fn command_completion_shows_the_candidates_in_the_footer() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    for c in [':', 'n'] { gameday::input::handle_key(&mut app, crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE); }
    gameday::input::handle_key(&mut app, crossterm::event::KeyCode::Tab, crossterm::event::KeyModifiers::NONE);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let last = buf_text(&term).lines().last().unwrap().to_string();
    assert!(last.contains(":nfl") && last.contains("nba") && last.contains("nhl"), "{last}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test keymap::tests::every_handled q_inside_help toggling_a_league layout_change_keeps filter_miss header_keeps command_completion 2>&1 | grep -E 'test .* (ok|FAILED)|error'`
Expected: failures / compile errors (`config_toggle_tab` is private — make it `pub(crate)`; `filter_miss_message` missing).

- [ ] **Step 3: Implement.**
  1. `on_key` help branch: `KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.help_open = false`.
  2. `keymap.rs`: add `Binding { keys: &["[/]"], label: "DATE", group: Group::Navigation, footer: FooterSlot::Never }` after PAGE; the KEYMAP tests count updates automatically.
  3. `config_toggle_tab`: after insert/remove, `self.config.enabled_tabs.sort_by_key(|l| League::ALL.iter().position(|x| x == l))`.
  4. `set_layout`: remember `let sel = self.selected_game().map(|g| g.id)`; after setting the layout, restore `self.selected` from the id in the new `selection_list()` and `self.page = self.selected / self.page_len()`.
  5. `filter_miss_message(&self) -> String`: scope = `match self.tab { Tab::Home => "all boards", Tab::League(l) => l.slug().to_uppercase() }`; ticker matches = `self.ticker_live()` first 2 as `A@B` joined by `, `; format as in the test; `views/board.rs:176` uses it.
  6. Footer completion: when `self.mode` is `Command` and `self.completion.is_some()`, append after the prompt: `  ▸ nfl  nba  nhl` (all matches of the stem, current one bright) — `command::complete(&stem)` already returns them.
  7. Header: compute the right side first (chip + date + clock); if the chips don't fit the remaining width, drop the `FILTER:` label, then render chips as bare 3-letter labels without brackets until they fit; the clock is never truncated.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/keymap.rs src/input.rs src/views/board.rs tests/draw.rs
git commit -m "fix(v3.1): q closes help, [/] advertised, handler↔keymap test, stable tab order, layout keeps the game, completion list, scoped filter message, clock never clipped"
```

---

### Task 14: Standings — sorted, labeled, honest columns (spec §7)

**Files:**
- Modify: `src/provider/map.rs` (`map_standings`: sort + `season` label + division sub-groups when present), `src/domain.rs` (`StandingsTable.season: Option<String>`, `fetched_at`), `src/views/standings.rs` (header line, third column per sport), `src/provider/espn.rs` (CFB group probe)
- Test: `tests/map_espn.rs`, `src/views/standings.rs`

- [ ] **Step 1: Write the failing tests** — `tests/map_espn.rs`:

```rust
use gameday::provider::map::map_standings;

#[test]
fn standings_rows_are_sorted_by_win_pct_then_wins_then_name() {
    let t = map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json")).unwrap();
    for g in &t.groups {
        let pct = |r: &gameday::domain::StandingRow| { let gp = r.wins + r.losses + r.third.unwrap_or(0); if gp == 0 { 0.0 } else { (r.wins as f64 + 0.5 * r.third.unwrap_or(0) as f64) / gp as f64 } };
        for w in g.rows.windows(2) {
            assert!(pct(&w[0]) >= pct(&w[1]) - 1e-9, "{} before {} in {}", w[0].abbr, w[1].abbr, g.name);
        }
    }
}

#[test]
fn standings_label_is_the_season_when_present_else_none() {
    let t = map_standings(League::Nfl, include_str!("../fixtures/nfl_standings.json")).unwrap();
    assert_eq!(t.season, None, "this fixture carries no season key");
    let json = r#"{"name":"X","season":{"displayName":"2025-26"},"children":[{"name":"East","standings":{"entries":[{"team":{"abbreviation":"BOS","name":"Celtics"},"stats":[{"type":"wins","value":58},{"type":"losses","value":24}]}]}}]}"#;
    let t = map_standings(League::Nba, json).unwrap();
    assert_eq!(t.season.as_deref(), Some("2025-26"));
}

#[test]
fn ties_column_only_when_the_sport_has_one() {
    let json = r#"{"name":"MLB","children":[{"name":"AL","standings":{"entries":[{"team":{"abbreviation":"TB","name":"Rays"},"stats":[{"type":"wins","value":82},{"type":"losses","value":55},{"type":"ties","value":0}]}]}}]}"#;
    let t = map_standings(League::Mlb, json).unwrap();
    assert_eq!(t.groups[0].rows[0].third, None, "MLB sends ties=0 for every team; drop the column");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test map_espn standings 2>&1 | tail -4`
Expected: compile error (`season` field) then sort failure.

- [ ] **Step 3: Implement.** `StandingsTable { league, season: Option<String>, groups }`. In `map_standings`: read `v["season"]["displayName"].as_str()` (or `v["seasonDisplayName"]`); after building each group, `rows.sort_by(|a, b| pct(b).partial_cmp(&pct(a)).unwrap().then(b.wins.cmp(&a.wins)).then(a.name.cmp(&b.name)))`; the third column: keep `ties` only for `League::Nfl | League::Cfb | League::Epl | League::Mls` and `otlosses` for `League::Nhl`; everything else `third: None`. Division sub-groups: if a child has its own `children[]` (verified absent for NFL, present for some leagues), emit one group per grandchild named `"{conference} · {division}"`. View header: `STANDINGS  MLB  ·  2025-26` when `season` is Some, else `STANDINGS  MLB  ·  updated 9:41 PM` using the provider's cache age (add `fetched_at: Option<OffsetDateTime>` set in `merge_standings` from `App::now()`). CFB: in `standings_url`, when `league == Cfb` append `?group=80`; if the mapped table is empty the view shows `ESPN offers no FBS-wide standings table · try :standings <conf> (coming in v3.3)` instead of `no standings yet` — the message string is the deliverable; conference tables are sub-project 3.

- [ ] **Step 4: Run tests + look**

Run: `cargo test 2>&1 | tail -4 && cargo run --release -- dump >/dev/null && open out/standings.png`
Expected: PASS; standings sorted, header carries the label.

- [ ] **Step 5: Commit**

```bash
git add src/provider src/domain.rs src/views/standings.rs src/app.rs tests/map_espn.rs
git commit -m "feat(v3.1): standings sorted by pct, season/updated label, ties/OTL only where the sport has them, honest CFB message"
```

---

### Task 15: CFB date semantics — one fetch path, header and slate agree (spec §2 CFB)

**Files:**
- Modify: `src/provider/espn.rs` (`scoreboard_url`, `scoreboard_on_url`), `src/main.rs`/`poll` (CFB "today" uses the dated URL), `src/provider/espn.rs` tests

- [ ] **Step 1: Verify the endpoint by hand (allowed: it's a probe, not a test).** ESPN's CFB scoreboard without `dates` returns the current *week* (that's why Monday showed Saturday's 100 finals). Run:

```bash
for u in "groups=80" "groups=80&dates=20260829" "groups=80&dates=20260829&limit=300"; do
  printf '%s -> ' "$u"; curl -s -A "gameday/dev (+https://github.com/WallyMagill/game-day)" "https://site.web.api.espn.com/apis/site/v2/sports/football/college-football/scoreboard?$u" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))'; done
```

Record the three counts in the commit message. The expected shape: bare = whole week; `dates=` = that day, possibly capped; `limit=300` lifts the cap.

- [ ] **Step 2: Write the failing test** — `src/provider/espn.rs`:

```rust
    #[test]
    fn cfb_today_is_dated_and_uncapped() {
        let today = time::Date::from_calendar_date(2026, 8, 31).unwrap();
        let u = scoreboard_url_for(League::Cfb, Some(today));
        assert!(u.contains("groups=80") && u.contains("dates=20260831") && u.contains("limit=300"), "{u}");
        assert!(!scoreboard_url_for(League::Nfl, Some(today)).contains("dates="), "NFL today stays undated (ESPN returns the current week, which is what an NFL board wants)");
    }
```

- [ ] **Step 3: Implement.** `pub fn scoreboard_url_for(league: League, today: Option<time::Date>) -> String`: for `Cfb` always `?groups=80&limit=300` plus `&dates=YYYYMMDD` of `today` when given; `scoreboard_url(league)` calls it with `None` for non-CFB and the caller passes today for CFB. `EspnProvider::scoreboard(league)` computes today from `self.offset` (`OffsetDateTime::now_utc().to_offset(self.offset).date()`) and uses the dated URL for CFB; the cache key stays `cfb-scoreboard` (it's "today"). `scoreboard_on_url` gains `limit=300` for CFB too.

- [ ] **Step 4: Run tests + probe**

Run: `cargo test --lib provider::espn 2>&1 | tail -3 && cargo run --release -- probe cfb | head -3`
Expected: PASS; probe prints today's CFB count (0–8 on a Tuesday, not ~100).

- [ ] **Step 5: Commit**

```bash
git add src/provider/espn.rs
git commit -m "fix(v3.1): CFB today is dated and uncapped (bare=<week count>, dated=<n>, dated+limit=<n>) so the header and slate agree"
```

---

### Task 16: `App` split, per-frame `Derived`, memoized logo art (spec §4 App split)

**Files:**
- Move: `src/app.rs` → `src/app/mod.rs` (`git mv`)
- Create: `src/app/chrome.rs` (`draw_header`, `draw_footer`, `draw_help`, `age_label`), `src/app/derive.rs` (`Derived`), move `src/net.rs` → `src/app/net.rs`
- Modify: `src/views/board.rs`, `src/views/*.rs` (read `app.derived()`), `src/tiles/logo.rs` (`OnceLock<HashMap<&'static str, AnsiArt>>`)
- Test: existing suites (behavior-preserving), plus one clone-count test

**Interfaces:**

```rust
pub struct Derived { pub visible: Vec<Game>, pub live: Vec<Game>, pub slate: Vec<Game>, pub mosaic: Vec<Game>, pub selection: Vec<Game>, pub scoring: Vec<(Game, Play)>, pub ticker_live: Vec<Game>, pub ticker_events: Vec<(Game, Play)> }
impl App {
    pub(crate) fn derive(&self) -> Derived;        // the existing fns, evaluated once
    pub(crate) fn derived(&self) -> &Derived;      // valid only inside draw(); panics with a named message outside
}
```

- [ ] **Step 1: Mechanical move.** `git mv src/app.rs src/app/mod.rs`; cut `draw_header`/`draw_footer`/`draw_help`/`age_label` into `src/app/chrome.rs` as `impl App { … }` in a child module (`mod chrome;` in `mod.rs`; methods stay `pub(crate)`/private as before via `pub(super)`); cut the list-derivation fns (`visible_games`, `live_games`, `slate_games`, `mosaic_games`, `selection_list`, `scoring_events`, `ticker_live`, `ticker_events`, `concat_boards`) into `src/app/derive.rs` — **keep them as methods** (key handlers still call them) and add `derive()`/`derived()`. `git mv src/net.rs src/app/net.rs`. Run `cargo test` — everything must pass unchanged before Step 2.

- [ ] **Step 2: Write the failing test** — `src/app/derive.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draw_derives_once_per_frame() {
        // Instrumented: Derived::new increments a thread-local counter.
        let mut app = crate::app::tests::app_with(crate::app::tests::six_live(), vec![]);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        DERIVE_COUNT.with(|c| c.set(0));
        term.draw(|f| app.draw(f)).unwrap();
        assert_eq!(DERIVE_COUNT.with(|c| c.get()), 1, "one derivation per draw, not one per widget");
    }
}
```

(add `thread_local! { pub(crate) static DERIVE_COUNT: Cell<u32> = Cell::new(0); }` incremented in `derive()`; make the app test helpers `pub(crate)`.)

- [ ] **Step 3: Implement.** In `App::draw`, first line: `self.frame_cache = Some(self.derive());` and last line `self.frame_cache = None;` (`frame_cache: Option<Derived>` field, `derived()` returns `self.frame_cache.as_ref().expect("App::derived() outside draw — use the list fns")`). Change `views/board.rs`, `views/zoom.rs`, `views/plays_feed.rs`, and `chrome.rs` to read `app.derived().mosaic` etc. instead of calling the fns. `ticker::draw` receives `&d.ticker_live, &d.ticker_events`. In `tiles/logo.rs::load_logo`, replace the per-call `parse_ansi_art(include_str!(…))` with a `static ART: OnceLock<HashMap<&'static str, AnsiArt>>` filled on first use.

- [ ] **Step 4: Run tests + a CPU check**

Run: `cargo test 2>&1 | tail -4`; then `cargo run --release -- --demo` in tmux for 30 s and `ps -o %cpu -p $(pgrep -f 'gameday --demo')` three times.
Expected: PASS; live-demo CPU noticeably below the pre-task number (record both in the commit message).

- [ ] **Step 5: Commit**

```bash
git add -A src
git commit -m "refactor(v3.1): app/{mod,chrome,derive,net}; one Derived per frame (was ~15 board clones); logo art memoized — demo CPU <before>% → <after>%"
```

---

### Task 17: Real fixtures for all nine leagues + per-league mapping tests + gallery captures (spec §8)

**Files:**
- Create: `scripts/capture-fixtures.sh`, `fixtures/{nfl,cfb,cbb,nba,wnba,nhl,mlb,epl,mls}_scoreboard_full.json`, `fixtures/{…}_summary_full.json` (jq-trimmed: keep `header, plays[0:80], scoringPlays, drives, keyEvents, boxscore, leaders`)
- Modify: `tests/map_espn.rs`, `src/dump.rs` (gallery gains `home-live`, `offline`, `stale`, `config-error`)

- [ ] **Step 1: Capture script** (run once; the JSON is committed; tests never call it):

```bash
#!/usr/bin/env bash
# Captures one scoreboard + one summary per league into fixtures/. Off-season
# leagues use a known past date so linescores/plays exist. Re-run deliberately;
# fixtures are frozen inputs, not live mirrors.
set -euo pipefail
UA="gameday/fixtures (+https://github.com/WallyMagill/game-day)"
B="https://site.web.api.espn.com/apis/site/v2/sports"
declare -A P=( [nfl]="football/nfl" [cfb]="football/college-football" [cbb]="basketball/mens-college-basketball" [nba]="basketball/nba" [wnba]="basketball/wnba" [nhl]="hockey/nhl" [mlb]="baseball/mlb" [epl]="soccer/eng.1" [mls]="soccer/usa.1" )
declare -A D=( [nfl]="20260913" [cfb]="20260829" [cbb]="20260307" [nba]="20260415" [wnba]="20260831" [nhl]="20260412" [mlb]="20260831" [epl]="20260830" [mls]="20260830" )
for l in "${!P[@]}"; do
  q="dates=${D[$l]}"; [[ $l == cfb ]] && q="$q&groups=80&limit=300"
  curl -sf -A "$UA" "$B/${P[$l]}/scoreboard?$q" > "fixtures/${l}_scoreboard_full.json"
  id=$(python3 -c 'import json,sys; e=[x for x in json.load(sys.stdin)["events"] if x["status"]["type"]["state"]=="post"]; print(e[0]["id"] if e else "")' < "fixtures/${l}_scoreboard_full.json")
  [[ -n $id ]] && curl -sf -A "$UA" "$B/${P[$l]}/summary?event=$id" \
    | python3 -c 'import json,sys; v=json.load(sys.stdin); k={x:v[x] for x in ("header","scoringPlays","drives","keyEvents","boxscore","leaders") if x in v}; k["plays"]=v.get("plays",[])[:80]; json.dump(k,sys.stdout)' > "fixtures/${l}_summary_full.json"
  sleep 1
done
ls -la fixtures/*_full.json
```

Dates are the review week (2026-08-29..31) for in-season leagues and the last week of the 2025-26 seasons for off-season ones; adjust any that return zero `post` events and note the final dates in the commit.

- [ ] **Step 2: Write the tests** — `tests/map_espn.rs`:

```rust
#[test]
fn every_league_maps_its_full_scoreboard_and_summary_with_no_skips() {
    let cases: [(League, &str, &str); 9] = [
        (League::Nfl, include_str!("../fixtures/nfl_scoreboard_full.json"), include_str!("../fixtures/nfl_summary_full.json")),
        (League::Cfb, include_str!("../fixtures/cfb_scoreboard_full.json"), include_str!("../fixtures/cfb_summary_full.json")),
        (League::Cbb, include_str!("../fixtures/cbb_scoreboard_full.json"), include_str!("../fixtures/cbb_summary_full.json")),
        (League::Nba, include_str!("../fixtures/nba_scoreboard_full.json"), include_str!("../fixtures/nba_summary_full.json")),
        (League::Wnba, include_str!("../fixtures/wnba_scoreboard_full.json"), include_str!("../fixtures/wnba_summary_full.json")),
        (League::Nhl, include_str!("../fixtures/nhl_scoreboard_full.json"), include_str!("../fixtures/nhl_summary_full.json")),
        (League::Mlb, include_str!("../fixtures/mlb_scoreboard_full.json"), include_str!("../fixtures/mlb_summary_full.json")),
        (League::Epl, include_str!("../fixtures/epl_scoreboard_full.json"), include_str!("../fixtures/epl_summary_full.json")),
        (League::Mls, include_str!("../fixtures/mls_scoreboard_full.json"), include_str!("../fixtures/mls_summary_full.json")),
    ];
    for (league, sb, sm) in cases {
        let events = serde_json::from_str::<serde_json::Value>(sb).unwrap()["events"].as_array().unwrap().len();
        let games = map_scoreboard(league, sb, et()).unwrap();
        assert_eq!(games.len(), events, "{}: every event maps (none skipped)", league.slug());
        for g in &games {
            assert!(g.start.is_some(), "{}: {} has no start", league.slug(), g.id);
            if g.status != Status::Pre { assert!(!g.linescore.is_empty() || matches!(league, League::Epl | League::Mls), "{}: {} linescore", league.slug(), g.id); }
        }
        let s = map_summary(sm).unwrap();
        assert!(!s.last_plays.is_empty(), "{}: summary plays", league.slug());
        if league == League::Mlb { assert!(s.last_plays.iter().all(|p| !p.text.starts_with("Pitch ")), "MLB pitch rows leaked"); }
        if !s.scoring_plays.is_empty() { assert!(s.last_plays.iter().any(|p| p.scoring), "{}: scoring flag lost", league.slug()); }
        let stats = map_stats(sm).unwrap();
        assert!(!stats.rows.is_empty(), "{}: box score rows (grouped or flat)", league.slug());
    }
}
```

- [ ] **Step 3: Gallery captures** in `src/dump.rs::gallery()`:

```rust
    fn home_live(app: &mut App) { app.tab = Tab::Home; } // Home now shows every live demo game — the first-boot frame
    fn offline(app: &mut App) {
        app.boards.clear();
        app.note_failure(League::Nfl, "ESPN unreachable nfl scoreboard".into(), Some(Duration::from_secs(40)));
    }
    fn stale(app: &mut App) { app.net.ok(Instant::now() - Duration::from_secs(4 * 60), true); }
    fn config_error(app: &mut App) {
        app.config_error = Some("config.toml:7: unknown variant `NFLL` — valid leagues: nfl|cfb|cbb|nba|wnba|nhl|mlb|epl|mls".into());
        app.status_line = Some("not saving: config.toml:7: unknown variant `NFLL` — valid leagues: nfl|cfb|…".into());
    }
    // …and in the list:
    full("home-live", "broadcast", ScoreStyle::Big, home_live),
    full("offline", "broadcast", ScoreStyle::Big, offline),
    full("stale", "broadcast", ScoreStyle::Big, stale),
    full("config-error", "broadcast", ScoreStyle::Big, config_error),
```

Update the README's `dump` paragraph with the four new stems.

- [ ] **Step 4: Run everything and look**

Run: `bash scripts/capture-fixtures.sh && cargo test 2>&1 | tail -4 && GAMEDAY_DUMP_FONT=… cargo run --release -- dump >/dev/null && open out/home-live.png out/offline.png out/stale.png out/config-error.png`
Expected: PASS; the four captures show what their names say; `du -sh fixtures` under 6 MB.

- [ ] **Step 5: Commit**

```bash
git add scripts fixtures tests/map_espn.rs src/dump.rs README.md
git commit -m "test(v3.1): full real fixtures for all nine leagues, no-skip mapping test; gallery adds home-live/offline/stale/config-error"
```

---

### Task 18: Definition-of-done sweep (spec §10)

**Files:** none new — this task produces evidence, then one docs commit.

- [ ] **Step 1: Test + lint receipts**

Run: `cargo test 2>&1 | grep -E '^test result' ; cargo clippy --all-targets 2>&1 | grep -c '^warning'`
Expected: every suite `ok`; `0` warnings.

- [ ] **Step 2: Live capture on a real night** — in tmux at 120×40 with the real binary and a scratch HOME, wait 30 s, then `tmux capture-pane -p > /tmp/home-live.txt`. Assert by eye and by grep: `grep -c 'PM\|AM' /tmp/home-live.txt` > 0 (local times), `grep -c '2026-' /tmp/home-live.txt` == 0 (no ISO), `grep -c -- '-:--' /tmp/home-live.txt` == 0, Home shows live games (or the `nothing live · next:` line if nothing is live), and TOP PLAYS/ticker ALERTS show at least one scoring play within ten minutes of a live game scoring.

- [ ] **Step 3: Offline + stale capture** — the Task 10 Step 4 procedure; save both captures.

- [ ] **Step 4: Budget receipt** — run 10 minutes with all nine leagues enabled and one game zoomed while logging requests (add `GAMEDAY_LOG_REQUESTS=1` support in `poll_loop`: one `eprintln!` per request with a timestamp, gated by the env var — 5 lines), then `grep -c . /tmp/reqs.log` ≤ 440.

- [ ] **Step 5: CLI receipts** — `gameday --help | head -3`, `gameday --version`, `gameday --bogus; echo $?` (2), `gameday | cat` (TTY message, exit 1).

- [ ] **Step 6: Record** the receipts (commands + outputs, abridged) at the bottom of the spec under a `## Verification (2026-MM-DD)` heading and commit:

```bash
git add docs/superpowers/specs/2026-08-31-gameday-v3-1-truth-design.md src/main.rs
git commit -m "docs(v3.1): verification receipts — tests, clippy, live/offline/stale captures, 10-min request budget, CLI"
```

---

## Self-review

**Spec coverage.** §1 domain → Tasks 2, 3, 4 (Extras, MatchEvent, rank, period, linescore, timeouts, scoring_plays; `Meter::Penalty` deferred per the Global Constraints note). §2 mapper → Tasks 1, 3, 4, 15 (time, per-event skip, MLB, records fallback in Task 9, CFB dates, soccer details). §3 provider → Tasks 5, 6 (map-then-cache, fallback, timeouts, per-league backoff+jitter, stagger, ETag, UA, error type, budget, `Scheduler` replaces inline loop). §4 app → Tasks 7, 9, 13, 16 (Home, glyphs+toasts, q/[]/coverage, tab order, layout selection, completion, filter scope, App split, Derived, logo memo). §5 failure visibility → Tasks 10, 11 (chip states, frozen UPD, offline board line, panic hook, error strings). §6 CLI/config → Tasks 11, 12. §7 standings/date travel → Tasks 14, 15. §8 tests → each task + Task 17 (fixtures, no-skip, truncation regression in Task 4, local_time boundaries in Task 1, cache poison in Task 5, budget in Task 6, config error in Task 12, handler→keymap in Task 13, Home order in Task 7, standings sort in Task 14, gallery in Task 17). §10 DoD → Task 18.

**Gaps found and closed while reviewing:** the `:` completion-candidates footer had no home → Task 13 step 6; `apply_boards` on a stale (cached) payload must not fire deltas → Task 10 step 3; `Situation.due_up` was mapped but nothing rendered it — it is data for the sub-project 2 zoom rebuild and is intentionally not drawn here (spec §1 lists it as mapped, not shown).

**Type consistency.** `App::new(config, pins, dir, offset)` from Task 7 onward — Tasks 9, 10, 12, 13 tests use that arity. `TileFx { flash, live_bright, pinned, favorite, now }` (Task 9) is what Task 16's `derive` passes through. `ProviderError::Http { status, url, detail }` (Task 5) is what `Msg::Failed` (Task 6) shortens via `.short()` and `NetStatus::failed` (Task 10) stores. `Wants`/`Request`/`Scheduler` (Task 6) are the only poll-thread interface after Task 6; `App::poll_plan` is deleted there. `Game::default()` (Task 2) is used by every later test literal. `text::fmt_start(start, now)` (Task 1) is called from tiles/board/zoom with `fx.now`/`app.now()` after Task 9.

**Placeholders.** None: every step has code or an exact command; the two "verify by hand" steps (Tasks 10, 15) are probes with expected outputs, not TODOs.
