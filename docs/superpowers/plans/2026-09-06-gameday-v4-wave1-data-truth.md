# gameday v4 Wave 1 — Data Truth and Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The scoring cut names the actual scoring play every time, every text and meter defect the review found (D1–D10) is fixed with a test that would have caught it, and replay fixtures of consecutive real polls become the regression floor.

**Architecture:** A score delta on the scoreboard no longer trusts `lastPlay` unless the feed marks it as scoring; otherwise the game is queued for a one-shot summary fetch (a new `catchup` list on `poll::Wants`, emitted once per sequence number by the scheduler), and the summary's scoring plays fire the cut. Plays gain a stable `id` and a `period` in every league so dedupe and display stop depending on text. The mapper reads `shortDownDistanceText`, requires a possession id for the red zone, and reads the standings season type. A capture script records consecutive scoreboard polls into `fixtures/replay/`, and `tests/replay.rs` drives them through the real `apply_boards`/`merge_summary` path.

**Tech Stack:** Rust 2021 (ratatui 0.30, ureq 3), bash 3.2 + curl + jq for capture, the existing `MemoryProvider`, `TestBackend` draw tests.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` — §3 (Wave 1) is the authority; §1 D1–D10 are the findings; §9 receives the receipts.

## Global Constraints

- Work on branch `v4-wave1` from `main` (`a83f9c1`). Commit after every task with the trailer below; never push.
- Suite is 532 at the start; every task ends green with the count reported. `cargo clippy --all-targets --locked -- -D warnings` clean and `cargo fmt --check` clean before every commit.
- Structure over parsing (spec §0): every fix reads a field ESPN publishes. The one presentation-only rule (dropping a duplicated `(m:ss) ` prefix in a list) is labeled as such in code.
- Budget (spec §3.2): the catch-up fetch is at most one `Summary` request per score event; it is the only new request type. Measured in Task 14.
- Dedupe of plays is by `id` when both sides have one, else by `text` (demo and sim data carry no ids).
- Reorder discipline: nothing in this wave adds a reorder trigger; a summary never reorders.
- Feed pass-throughs `2ND & -4` and `(D. Klein KICK)` are left as ESPN sends them (D6).
- Names used across tasks (exact): `Play.id: String`, `poll::CatchupReq { league, game_id, seq }`, `Wants.catchup: Vec<CatchupReq>`, `Scheduler.last_catchup_seq: u64`, `App.catchup: Vec<CatchupEntry>`, `App.catchup_seq: u64`, `App::catchup_wants() -> Vec<CatchupReq>`, `App::cuts_fired() -> u32`, `merge::same_play(a, b) -> bool`, `provider::map::period_label(league, &Value) -> String`, `theme::select_or_default_noting(name) -> (String, Option<String>)`, `StandingsTable.season_type: Option<u8>`, `MemoryProvider::insert_summary(id, Summary)`, `CATCHUP_TTL_TICKS`.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `src/domain.rs` | `Play.id`; `StandingsTable.season_type` |
| `src/provider/map.rs` | `period_label`; ids on every mapped play; `scoring` from `scoringPlay`/`scoreValue` on the scoreboard's last play; `shortDownDistanceText`; possession-gated red zone; `season_type` |
| `src/poll.rs` | `CatchupReq`, `Wants.catchup`, one-shot emission by sequence number |
| `src/app/mod.rs` | `catchup`, `catchup_seq`, `cuts_fired_count` fields; `catchup_wants`, `cuts_fired` accessors |
| `src/app/merge.rs` | `same_play`; gated delta capture; catch-up firing in `merge_summary`; TTL prune |
| `src/main.rs` | publishes `catchup`; theme note into the footer |
| `src/rank.rs` | red zone chip requires possession |
| `src/tiles/mod.rs` | `play_stamp` prints period and clock; `play_line` drops a duplicated clock prefix (presentation only) |
| `src/board/hero.rs` | hides a `0-0` record on a live or final game |
| `src/theme.rs` | `select_or_default_noting` |
| `src/views/standings.rs` | `PRESEASON`/`POSTSEASON` label |
| `src/provider/memory.rs` | `insert_summary` |
| `scripts/capture-replay.sh` | consecutive-poll capture into `fixtures/replay/` |
| `fixtures/replay/<league>-<stamp>/{00..NN}.json, summary-<id>.json, PROVENANCE.md` | replay sequences |
| `tests/replay.rs` | the replay harness |
| `tests/map_espn.rs`, `tests/poll.rs`, `tests/draw.rs`, `src/app/tests.rs` | the teeth |

---

### Task 1: `Play.id` and `period_label`; every mapped play carries both

**Files:**
- Modify: `src/domain.rs` (`Play`), `src/provider/map.rs` (scoreboard last play, `scoringPlays`, drive plays, flat plays), `tests/map_espn.rs`
- Every `Play { … }` literal in `src/` and `tests/` that does not use `..Default::default()` gains `id: String::new()` (the compiler lists them).

**Interfaces:**
- Produces: `Play.id: String` (empty when the feed had none); `pub fn period_label(league: League, period: &serde_json::Value) -> String` in `src/provider/map.rs`.

- [ ] **Step 1: Write the failing mapper tests**

Append to `tests/map_espn.rs`:

```rust
#[test]
fn summary_plays_carry_ids_and_period_labels_in_every_league() {
    // Football: drives.previous[].plays[].period.number → "Q1".
    let s = map_summary(League::Cfb, include_str!("../fixtures/cfb_summary_full.json"), ).unwrap();
    let p = s.last_plays.iter().find(|p| !p.id.is_empty()).expect("football plays carry ids");
    assert!(p.id.chars().all(|c| c.is_ascii_digit()), "ESPN play ids are numeric: {}", p.id);
    assert!(s.last_plays.iter().all(|p| p.period.starts_with('Q') || p.period == "OT"), "{:?}", s.last_plays.iter().map(|p| &p.period).take(5).collect::<Vec<_>>());
    assert!(s.scoring_plays.iter().all(|p| !p.id.is_empty() && !p.period.is_empty()), "scoringPlays carry id + period");
    // Hoops: plays[].period.number → "Q1".."Q4"/"OT".
    let s = map_summary(League::Nba, include_str!("../fixtures/nba_summary_full.json")).unwrap();
    assert!(s.last_plays.iter().all(|p| !p.id.is_empty()));
    assert!(s.last_plays.iter().any(|p| p.period == "Q1"), "{:?}", s.last_plays.iter().map(|p| &p.period).take(5).collect::<Vec<_>>());
    // NHL: period.number → "P1".."P3"/"OT".
    let s = map_summary(League::Nhl, include_str!("../fixtures/nhl_summary_full.json")).unwrap();
    assert!(s.last_plays.iter().any(|p| p.period == "P1"), "{:?}", s.last_plays.iter().map(|p| &p.period).take(5).collect::<Vec<_>>());
    // MLB keeps the existing inning tag ("T1"/"B9").
    let s = map_summary(League::Mlb, include_str!("../fixtures/mlb_summary_full.json")).unwrap();
    assert!(s.last_plays.iter().any(|p| p.period == "T1" || p.period == "B1"), "{:?}", s.last_plays.iter().map(|p| &p.period).take(5).collect::<Vec<_>>());
    assert!(s.last_plays.iter().all(|p| !p.id.is_empty()));
}

#[test]
fn scoreboard_last_play_carries_its_id() {
    let games = map_scoreboard(League::Cfb, include_str!("../fixtures/live/cfb_scoreboard_live.json"), et()).unwrap();
    let live = games.iter().find(|g| g.status == Status::Live && !g.last_plays.is_empty()).expect("a live game with a last play");
    assert!(!live.last_plays[0].id.is_empty(), "situation.lastPlay.id is mapped");
}
```

Run: `cargo test --release --test map_espn summary_plays_carry 2>&1 | grep -E 'error\[|test result'`
Expected: a compile error (`no field id`).

- [ ] **Step 2: Add the field and the label helper**

In `src/domain.rs`, add as the first field of `Play`:

```rust
    /// ESPN's own play id (`situation.lastPlay.id`, `plays[].id`,
    /// `scoringPlays[].id`). Empty for demo and sim plays, which carry no
    /// feed identity; dedupe falls back to `text` then (`merge::same_play`).
    pub id: String,
```

In `src/provider/map.rs`, beside `inning_tag`:

```rust
/// The compact period tag a play row prints beside its clock, from the
/// play's own `period` object, in the grammar the scoreboard's period
/// labels already use: football and pro hoops `Q1`..`Q4` then `OT`;
/// college hoops `1H`/`2H` then `OT`; hockey `P1`..`P3` then `OT`;
/// baseball `T3`/`B9` (top/bottom + inning, via `inning_tag`); soccer plays
/// carry the minute in their clock and no period, so "".
pub fn period_label(league: League, period: &Value) -> String {
    let Some(n) = period["number"].as_u64() else {
        return String::new();
    };
    match league {
        League::Mlb => inning_tag(period),
        League::Epl | League::Mls => String::new(),
        League::Cbb => match n {
            1 => "1H".into(),
            2 => "2H".into(),
            _ => "OT".into(),
        },
        League::Nhl => match n {
            1..=3 => format!("P{n}"),
            _ => "OT".into(),
        },
        League::Nfl | League::Cfb | League::Nba | League::Wnba => match n {
            1..=4 => format!("Q{n}"),
            _ => "OT".into(),
        },
    }
}
```

Then fill `id` and `period` at every construction site in `map.rs`:
- scoreboard last play (the `last_plays.push(Play { … })` near `mlb_last_play_text`): `id: sit_v["lastPlay"]["id"].as_str().unwrap_or("").to_string(),` and leave `period` as it is (the scoreboard row's period comes from the game).
- `scoringPlays` mapping: `id: p["id"].as_str().unwrap_or("").to_string(), period: period_label(league, &p["period"]),`.
- drive plays: `id: p["id"].as_str().unwrap_or("").to_string(), period: period_label(league, &p["period"]),`.
- flat plays: `id: p["id"].as_str().unwrap_or("").to_string(),` and replace `period: inning_tag(&p["period"])` with `period: period_label(league, &p["period"])` (MLB routes back to `inning_tag`).
- soccer `details_from`/keyEvents rows if they build `Play`: `id` from `p["id"]` if present, else empty.

Build: `cargo build --all-targets 2>&1 | grep -E '^error' | head` and add `id: String::new(),` to each `Play { … }` literal the compiler names (demo, sim, tests).

- [ ] **Step 3: Run the tests**

Run: `cargo test --release --test map_espn 2>&1 | grep -E '^test result'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: map_espn `ok`; `534 tests` (two added).

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "feat(map): plays carry ESPN ids and a period label in every league

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 2: The scoreboard's last play is `scoring` only when the feed says so

**Files:**
- Modify: `src/provider/map.rs` (scoreboard last play `scoring:`), `tests/map_espn.rs`

**Interfaces:**
- Produces: `Play.scoring == true` on a scoreboard last play iff `lastPlay.scoringPlay == true` or `lastPlay.scoreValue > 0`. Task 4 gates the fast path on this.

- [ ] **Step 1: Write the failing tests**

Append to `tests/map_espn.rs`:

```rust
fn one_event_with_last_play(last_play: &str) -> String {
    format!(r#"{{"events":[{{"id":"9","competitions":[{{"status":{{"displayClock":"4:20","period":3,"type":{{"state":"in","completed":false}}}},"competitors":[
      {{"homeAway":"away","score":"13","team":{{"id":"1","abbreviation":"GB","displayName":"Packers","color":"203731","alternateColor":"ffb612"}}}},
      {{"homeAway":"home","score":"10","team":{{"id":"2","abbreviation":"CHI","displayName":"Bears","color":"0b162a","alternateColor":"c83803"}}}}
    ],"situation":{{"possession":"1","downDistanceText":"1st & 10 at CHI 41","shortDownDistanceText":"1st & 10","possessionText":"CHI 41","lastPlay":{last_play}}}}}]}}]}}"#)
}

#[test]
fn a_scoreboard_last_play_is_scoring_only_when_the_feed_marks_it() {
    let td = one_event_with_last_play(r#"{"id":"77","text":"Love pass to Reed for 41 yards, TOUCHDOWN","scoringPlay":true,"scoreValue":6,"team":{"id":"1"},"type":{"id":"67","text":"Passing Touchdown"}}"#);
    let g = &map_scoreboard(League::Nfl, &td, et()).unwrap()[0];
    assert!(g.last_plays[0].scoring, "scoringPlay:true marks it");
    assert_eq!(g.last_plays[0].id, "77");

    let by_value = one_event_with_last_play(r#"{"id":"78","text":"Rodgers 2 yard run","scoringPlay":null,"scoreValue":6,"team":{"id":"1"},"type":{"id":"68","text":"Rushing Touchdown"}}"#);
    let g = &map_scoreboard(League::Nfl, &by_value, et()).unwrap()[0];
    assert!(g.last_plays[0].scoring, "scoreValue > 0 marks it even with scoringPlay null");

    let plain = one_event_with_last_play(r#"{"id":"79","text":"Love scrambles for 6","scoringPlay":null,"scoreValue":0,"team":{"id":"1"},"type":{"id":"5","text":"Rush"}}"#);
    let g = &map_scoreboard(League::Nfl, &plain, et()).unwrap()[0];
    assert!(!g.last_plays[0].scoring, "a plain snap is not a scoring play");

    // The live captures never mark one (ESPN sends scoringPlay: null on
    // every observed scoreboard row): every last play maps as not scoring.
    let games = map_scoreboard(League::Cfb, include_str!("../fixtures/live/cfb_scoreboard_live.json"), et()).unwrap();
    assert!(games.iter().flat_map(|g| &g.last_plays).all(|p| !p.scoring));
}
```

Run: `cargo test --release --test map_espn a_scoreboard_last_play_is_scoring 2>&1 | grep -E 'test result|panicked' | head -3`
Expected: FAIL on the first assertion (`scoring` is hard-coded `false`).

- [ ] **Step 2: Map it**

In the scoreboard last-play construction in `src/provider/map.rs`, replace `scoring: false,` with:

```rust
            // ESPN's own verdict. The observed scoreboard rows carry
            // `scoringPlay: null` (every live capture in fixtures/live, and
            // every pitch and snap seen in the review's caches), so on real
            // data this is usually false even for the play that scored — which
            // is exactly why a score delta with a non-scoring last play asks
            // the summary instead (`app::merge`).
            scoring: sit_v["lastPlay"]["scoringPlay"].as_bool() == Some(true)
                || score_value.is_some_and(|v| v > 0),
```

- [ ] **Step 3: Run**

Run: `cargo test --release --test map_espn 2>&1 | grep -E '^test result'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `ok`; `535 tests`.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "feat(map): a scoreboard last play is scoring only when scoringPlay or scoreValue says so

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 3: Catch-up requests in the scheduler

**Files:**
- Modify: `src/poll.rs`, `tests/poll.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct CatchupReq { pub league: League, pub game_id: String, pub seq: u64 }
  ```
  `Wants.catchup: Vec<CatchupReq>`; `Scheduler::due` emits `Request::Summary(league, game_id)` once for every entry whose `seq` exceeds `last_catchup_seq`, then advances it. No freshness window applies; `report_aux` on a failed catch-up summary does nothing special (the entry expires on the App side, Task 4).

- [ ] **Step 1: Write the failing tests**

Append to `tests/poll.rs`:

```rust
fn catchup(league: League, id: &str, seq: u64) -> CatchupReq {
    CatchupReq { league, game_id: id.into(), seq }
}

#[test]
fn a_catchup_request_emits_one_summary_and_never_repeats() {
    let mut s = Scheduler::new(7);
    let t0 = Instant::now();
    let mut w = wants(&[League::Mlb], true);
    w.catchup = vec![catchup(League::Mlb, "401", 1)];
    let first: Vec<_> = s.due(&w, false, t0).into_iter().filter(|r| matches!(r, Request::Summary(..))).collect();
    assert_eq!(first, vec![Request::Summary(League::Mlb, "401".into())]);
    // The same snapshot republished on later ticks emits nothing more.
    for i in 1..20u64 {
        let again: Vec<_> = s.due(&w, false, t0 + Duration::from_millis(200 * i)).into_iter().filter(|r| matches!(r, Request::Summary(..))).collect();
        assert!(again.is_empty(), "tick {i} re-emitted {again:?}");
    }
}

#[test]
fn a_second_delta_on_the_same_game_is_a_new_sequence_and_emits_again() {
    let mut s = Scheduler::new(7);
    let t0 = Instant::now();
    let mut w = wants(&[League::Mlb], true);
    w.catchup = vec![catchup(League::Mlb, "401", 1)];
    let _ = s.due(&w, false, t0);
    w.catchup = vec![catchup(League::Mlb, "401", 2), catchup(League::Cfb, "555", 3)];
    let got: Vec<_> = s.due(&w, false, t0 + Duration::from_secs(1)).into_iter().filter(|r| matches!(r, Request::Summary(..))).collect();
    assert_eq!(got, vec![Request::Summary(League::Mlb, "401".into()), Request::Summary(League::Cfb, "555".into())]);
}

#[test]
fn catchups_do_not_disturb_the_zoomed_summary_cadence() {
    let mut s = Scheduler::new(7);
    let t0 = Instant::now();
    let mut w = wants(&[League::Nfl], true);
    w.zoomed = Some((League::Nfl, "1".into()));
    w.catchup = vec![catchup(League::Nfl, "2", 1)];
    let got: Vec<_> = s.due(&w, false, t0).into_iter().filter(|r| matches!(r, Request::Summary(..))).collect();
    assert_eq!(got, vec![Request::Summary(League::Nfl, "1".into()), Request::Summary(League::Nfl, "2".into())]);
    // 5s later: the zoomed game is inside SUMMARY_EVERY, the catch-up is spent.
    let got: Vec<_> = s.due(&w, false, t0 + Duration::from_secs(5)).into_iter().filter(|r| matches!(r, Request::Summary(..))).collect();
    assert!(got.is_empty(), "{got:?}");
}
```

Run: `cargo test --release --test poll catchup 2>&1 | grep -E 'error\[|test result' | head -3`
Expected: compile error (`CatchupReq` unknown).

- [ ] **Step 2: Implement**

In `src/poll.rs`, after `Request`:

```rust
/// One score delta the UI could not attribute to a scoring play: the
/// scoreboard's `lastPlay` at that poll was a later snap or pitch. The
/// summary is the authority, so the UI asks for it exactly once per
/// sequence number. `seq` is monotonic per App, so the same game scoring
/// twice is two requests and a republished snapshot is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchupReq {
    pub league: League,
    pub game_id: String,
    pub seq: u64,
}
```

Add `pub catchup: Vec<CatchupReq>,` to `Wants` (derives `Default` already). Add `last_catchup_seq: u64,` to `Scheduler`, initialized `0` in `new`. In `due`, after the zoomed block and before the dated block:

```rust
        // Catch-ups: one Summary per new sequence number, no freshness
        // window (spec §3.2). Budget is one request per score event by
        // construction; Task 14 of the wave-1 plan measured it live.
        for c in wants.catchup.iter().filter(|c| c.seq > self.last_catchup_seq) {
            out.push(Request::Summary(c.league, c.game_id.clone()));
        }
        if let Some(max) = wants.catchup.iter().map(|c| c.seq).max() {
            self.last_catchup_seq = self.last_catchup_seq.max(max);
        }
```

- [ ] **Step 3: Run**

Run: `cargo test --release --test poll 2>&1 | grep -E '^test result'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `ok`; `538 tests`.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "feat(poll): one-shot catch-up summary requests by sequence number

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 4: Gated delta capture, catch-up firing, id-based dedupe

**Files:**
- Modify: `src/app/mod.rs` (fields, accessors), `src/app/merge.rs` (`apply_boards`, `merge_summary`, `same_play`), `src/app/tests.rs`, `src/main.rs` (publish `catchup`)

**Interfaces:**
- Consumes: `Play.id`, `Play.scoring` from the mapper; `poll::CatchupReq`.
- Produces on `App`: `pub(crate) catchup: Vec<CatchupEntry>` where `pub(crate) struct CatchupEntry { league: League, game_id: String, seq: u64, queued_tick: u64 }` (private module type in `merge.rs`, re-exported to `app`); `catchup_seq: u64`; `pub fn catchup_wants(&self) -> Vec<poll::CatchupReq>`; `pub fn cuts_fired(&self) -> u32`; `pub const CATCHUP_TTL_TICKS: u64 = 60 * LIVE_TICKS_PER_SEC` (60 s: four polls; a guess — a summary that never lands must not hold a request forever); `pub(crate) fn same_play(a: &Play, b: &Play) -> bool` in `merge.rs`.
- `App::scoring_events` becomes `pub` (the replay harness in `tests/` reads it).

- [ ] **Step 1: Write the failing tests**

Append to `src/app/tests.rs` (reuse `app_with`, `g`, `team`):

```rust
fn snap(id: &str, text: &str, team: &str, scoring: bool) -> Play {
    Play { id: id.into(), text: text.into(), team: team.into(), scoring, ..Default::default() }
}

#[test]
fn a_delta_with_a_non_scoring_last_play_queues_a_catchup_and_fires_no_cut() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "CHC", "MIA", true);
    g1.league = League::Mlb;
    g1.last_plays = vec![snap("p1", "Pitch 1 : Ball", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g1.clone()], false);
    let mut g2 = g1.clone();
    g2.away_score = 1;
    g2.last_plays = vec![snap("p2", "Pitch 2 : Foul", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g2], false);
    assert_eq!(app.cuts_fired(), 0, "a later pitch is not the scoring play");
    assert!(app.scoring_events().is_empty(), "nothing captured from a non-scoring row");
    let wants = app.catchup_wants();
    assert_eq!(wants.len(), 1);
    assert_eq!((wants[0].league, wants[0].game_id.as_str(), wants[0].seq), (League::Mlb, "1", 1));
}

#[test]
fn a_delta_with_a_scoring_last_play_fires_at_once_and_queues_nothing() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "GB", "CHI", true);
    g1.last_plays = vec![snap("p1", "Love scrambles for 6", "GB", false)];
    app.apply_boards(League::Nfl, vec![g1.clone()], false);
    let mut g2 = g1.clone();
    g2.away_score = 7;
    g2.last_plays = vec![snap("p2", "Love pass to Reed, TOUCHDOWN", "GB", true)];
    app.apply_boards(League::Nfl, vec![g2], false);
    assert_eq!(app.cuts_fired(), 1);
    assert_eq!(app.scoring_events()[0].1.id, "p2");
    assert!(app.catchup_wants().is_empty());
}

#[test]
fn the_catchup_summary_fires_exactly_one_cut_on_the_newest_scoring_play_and_clears_the_queue() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "CHC", "MIA", true);
    g1.league = League::Mlb;
    g1.last_plays = vec![snap("p1", "Pitch 1 : Ball", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g1.clone()], false);
    let mut g2 = g1.clone();
    g2.away_score = 1;
    g2.last_plays = vec![snap("p2", "Pitch 2 : Foul", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g2], false);
    assert_eq!(app.catchup_wants().len(), 1);
    let summary = Summary {
        last_plays: vec![snap("p2", "Pitch 2 : Foul", "MIA", false), snap("s1", "Happ homered to right (12)", "CHC", true)],
        scoring_plays: vec![snap("s1", "Happ homered to right (12)", "CHC", true)],
        meter: None,
        extras: Extras::None,
    };
    app.merge_summary("1", summary);
    assert_eq!(app.cuts_fired(), 1, "the summary's newest scoring play is the cut");
    let ev = app.scoring_events();
    assert_eq!(ev.len(), 1);
    assert_eq!((ev[0].1.id.as_str(), ev[0].1.team.as_str()), ("s1", "CHC"));
    assert!(app.catchup_wants().is_empty(), "the entry is consumed");
    // The same summary again (the zoomed cadence) is history, not news.
    app.merge_summary("1", Summary { last_plays: vec![], scoring_plays: vec![snap("s1", "Happ homered to right (12)", "CHC", true)], meter: None, extras: Extras::None });
    assert_eq!(app.cuts_fired(), 1);
}

#[test]
fn a_second_delta_on_a_queued_game_does_not_add_a_second_entry() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "CHC", "MIA", true);
    g1.league = League::Mlb;
    g1.last_plays = vec![snap("p1", "Pitch 1 : Ball", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g1.clone()], false);
    for score in [1u16, 2] {
        let mut gn = g1.clone();
        gn.away_score = score;
        gn.last_plays = vec![snap(&format!("p{score}"), "Pitch : Foul", "MIA", false)];
        app.apply_boards(League::Mlb, vec![gn], false);
    }
    assert_eq!(app.catchup_wants().len(), 1, "one entry per game while it is pending");
    assert_eq!(app.catchup_wants()[0].seq, 1);
}

#[test]
fn a_catchup_that_never_lands_expires_after_the_ttl_without_a_cut() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "CHC", "MIA", true);
    g1.league = League::Mlb;
    g1.last_plays = vec![snap("p1", "Pitch 1 : Ball", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g1.clone()], false);
    let mut g2 = g1.clone();
    g2.away_score = 1;
    g2.last_plays = vec![snap("p2", "Pitch 2 : Foul", "MIA", false)];
    app.apply_boards(League::Mlb, vec![g2], false);
    for _ in 0..=gameday_ttl() {
        app.advance_tick();
    }
    assert!(app.catchup_wants().is_empty(), "expired");
    assert_eq!(app.cuts_fired(), 0);
}

fn gameday_ttl() -> u64 {
    crate::app::CATCHUP_TTL_TICKS
}

#[test]
fn a_stale_apply_never_queues_a_catchup() {
    let mut app = app_with(vec![], vec![]);
    let mut g1 = g("1", "CHC", "MIA", true);
    g1.league = League::Mlb;
    app.apply_boards(League::Mlb, vec![g1.clone()], false);
    let mut g2 = g1.clone();
    g2.away_score = 3;
    app.apply_boards(League::Mlb, vec![g2], true);
    assert!(app.catchup_wants().is_empty());
}

#[test]
fn scoring_plays_dedupe_by_id_then_by_text() {
    use crate::app::merge::same_play;
    assert!(same_play(&snap("a", "x", "", true), &snap("a", "y", "", true)), "same id, different text");
    assert!(!same_play(&snap("a", "x", "", true), &snap("b", "x", "", true)), "different ids, same text");
    assert!(same_play(&snap("", "x", "", true), &snap("", "x", "", true)), "no ids: text decides");
    assert!(same_play(&snap("a", "x", "", true), &snap("", "x", "", true)), "one side without an id: text decides");
}
```

Run: `cargo test --release app::tests::a_delta_with 2>&1 | grep -E 'error\[' | head -3`
Expected: compile errors (`cuts_fired`, `catchup_wants` unknown).

- [ ] **Step 2: Fields and accessors in `src/app/mod.rs`**

Add to `App`:

```rust
    /// Score deltas waiting for the summary that names their scoring play
    /// (spec §3.2). One entry per game; `seq` is `catchup_seq` at queue time.
    pub(crate) catchup: Vec<merge::CatchupEntry>,
    pub(crate) catchup_seq: u64,
    /// Every cut fired since start, both paths. The replay harness and the
    /// budget receipt count it; nothing on screen reads it.
    cuts_fired_count: u32,
```

initialized `catchup: Vec::new(), catchup_seq: 0, cuts_fired_count: 0,` in `new`. Add:

```rust
/// How long a queued catch-up waits for its summary: four polls. A guess —
/// a summary that never arrives must not hold a request forever, and a
/// score older than a minute is no longer a cut.
pub const CATCHUP_TTL_TICKS: u64 = 60 * LIVE_TICKS_PER_SEC;

impl App {
    /// The snapshot `main` publishes to the poll thread.
    pub fn catchup_wants(&self) -> Vec<crate::poll::CatchupReq> {
        self.catchup
            .iter()
            .map(|c| crate::poll::CatchupReq { league: c.league, game_id: c.game_id.clone(), seq: c.seq })
            .collect()
    }

    pub fn cuts_fired(&self) -> u32 {
        self.cuts_fired_count
    }
}
```

In `advance_tick` (in `mod.rs`), add the prune:

```rust
        let ttl = CATCHUP_TTL_TICKS;
        let tick = self.tick;
        self.catchup.retain(|c| tick.saturating_sub(c.queued_tick) <= ttl);
```

Make `scoring_events` in `src/app/derive.rs` `pub`.

- [ ] **Step 3: `merge.rs`**

At the top of `src/app/merge.rs`:

```rust
/// One pending catch-up (spec §3.2): a game whose score moved while the
/// scoreboard's last play was not the scoring play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CatchupEntry {
    pub league: League,
    pub game_id: String,
    pub seq: u64,
    pub queued_tick: u64,
}

/// Play identity: ESPN ids when both sides carry one, else the text (demo
/// and sim plays have no ids). Never compare a real id against an empty one.
pub(crate) fn same_play(a: &Play, b: &Play) -> bool {
    if !a.id.is_empty() && !b.id.is_empty() {
        a.id == b.id
    } else {
        a.text == b.text
    }
}
```

In `apply_boards`, replace the block from `// The scoreboard's lastPlay at the moment the score moved IS the scoring play; dedupe on text.` through the end of the `if let Some(p) = g.last_plays.first()` block with:

```rust
                    // The scoreboard's last play at the moment the score
                    // moved is the scoring play only when ESPN marks it so.
                    // At a 15s cadence it is routinely the next pitch or
                    // snap (the review saw `HOME RUN · MIA FOUL`), and a
                    // real feed marks almost nothing: so the unmarked case
                    // asks the summary, which is the authority, once.
                    match g.last_plays.first() {
                        Some(p) if p.scoring => {
                            if !g.scoring_plays.iter().any(|s| same_play(s, p)) {
                                let p = p.clone();
                                g.scoring_plays.push(p.clone());
                                self.fire_cut(&g.id, &p, self.cut_is_full(g));
                            }
                        }
                        _ => {
                            if !self.catchup.iter().any(|c| c.game_id == g.id) {
                                self.catchup_seq += 1;
                                self.catchup.push(CatchupEntry {
                                    league: g.league,
                                    game_id: g.id.clone(),
                                    seq: self.catchup_seq,
                                    queued_tick: self.tick,
                                });
                            }
                        }
                    }
```

Add a helper on `App` in `merge.rs` that both sites use (replacing the two inline `cuts.fire` blocks):

```rust
    /// Fire a cut unless the view suppresses them; a takeover rings the bell.
    fn fire_cut(&mut self, game_id: &str, play: &Play, full: bool) {
        if self.cut_suppressed() {
            return;
        }
        self.cuts.fire(game_id, play, full, self.tick);
        self.cuts_fired_count += 1;
        if full {
            self.bell_pending = true;
        }
    }
```

In `merge_summary`: replace every text comparison with `same_play` (the `known` vector becomes `Vec<Play>` and `known.iter().any(|k| same_play(k, p))`; the `newest_first` position closure compares with `same_play`; the `play.scoring = true` marking uses `same_play`). Then replace the `fresh` computation so a queued catch-up is honored even on the first summary:

```rust
                // Newest new scoring play, if any. A queued catch-up says a
                // score just happened, so on that path the newest scoring
                // play is news even when this is the game's first summary.
                // Without a catch-up, a first summary is history being
                // backfilled, never a cut.
                let queued = self.catchup.iter().position(|c| c.game_id == game_id);
                if queued.is_some() || !known.is_empty() {
                    fresh = game
                        .scoring_plays
                        .iter()
                        .rev()
                        .find(|p| !known.iter().any(|k| same_play(k, p)))
                        .map(|p| (game.id.clone(), p.clone()));
                }
                if let Some(i) = queued {
                    self.catchup.remove(i);
                }
```

(`self.catchup` is borrowed while `self.boards` is mutably borrowed in that loop; compute `queued` before the `for board in self.boards.values_mut()` loop and remove after it, as the compiler requires.) The tail's cut firing becomes `self.fire_cut(&id, &play, full)`.

- [ ] **Step 4: `main.rs` publishes it**

In the `Wants { … }` literal in `run_ui`, add `catchup: app.catchup_wants(),`.

- [ ] **Step 5: Run everything**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'`
Expected: `545 tests, 0 failed`. The existing `a_score_delta_captures_the_scoreboard_last_play_as_a_scoring_play` and `a_cached_board_never_writes_a_scoring_play` tests build plays without `scoring: true`; update them so the delta's last play is `scoring: true` (that is the fast path they test) and add a one-line comment saying so. Any other existing test that asserted a capture from an unmarked last play now asserts a queued catch-up instead; list each in the report.

- [ ] **Step 6: Commit**

```bash
cargo fmt && git add -A src && git commit -q -m "feat(app): score deltas fire only on a marked scoring play; otherwise a one-shot catch-up summary names it

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 5: D1 — down and distance without the duplicated field position

**Files:**
- Modify: `src/provider/map.rs` (situation mapping), `tests/map_espn.rs`, `tests/draw.rs`

**Interfaces:**
- Produces: `Situation.down_distance` is `shortDownDistanceText` when present, else `downDistanceText` with a trailing ` at <possessionText>` removed (case-insensitive, exact match).

- [ ] **Step 1: Failing tests**

`tests/map_espn.rs`:

```rust
#[test]
fn down_distance_never_repeats_the_ball_position() {
    // The live CFB capture carries both fields.
    let games = map_scoreboard(League::Cfb, include_str!("../fixtures/live/cfb_scoreboard_live.json"), et()).unwrap();
    let sit = games.iter().filter_map(|g| g.situation.as_ref()).find(|s| !s.down_distance.is_empty()).unwrap();
    assert!(!sit.down_distance.contains(" at "), "short form wins: {}", sit.down_distance);
    // A feed with only the long form loses its trailing " at <ball_on>".
    let long_only = one_event_with_last_play(r#"{"id":"1","text":"x","scoringPlay":null,"scoreValue":0,"team":{"id":"1"},"type":{"id":"5","text":"Rush"}}"#)
        .replace(r#""shortDownDistanceText":"1st & 10","#, "");
    let g = &map_scoreboard(League::Nfl, &long_only, et()).unwrap()[0];
    let sit = g.situation.as_ref().unwrap();
    assert_eq!(sit.down_distance, "1st & 10");
    assert_eq!(sit.ball_on.as_deref(), Some("CHI 41"));
}
```

`tests/draw.rs` (beside the other row tests; use `mk`, `g`, `buf_text`):

```rust
#[test]
fn a_college_row_prints_the_field_position_once() {
    let mut app = mk();
    let mut game = g("1", "BAY", "AUB", true);
    game.league = League::Cfb;
    game.period = "Q2".into();
    game.clock = "7:13".into();
    game.situation = Some(Situation {
        down_distance: "1st & 10".into(),
        possession: Some("BAY".into()),
        ball_on: Some("BAY 2".into()),
        ..Default::default()
    });
    app.apply_boards(League::Cfb, vec![game], false);
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("BAY 1ST & 10 AT BAY 2"), "{s}");
    assert!(!s.contains("AT BAY 2 AT BAY 2"), "the field position printed twice:\n{s}");
}
```

Run: `cargo test --release --test map_espn down_distance_never 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL (`down_distance` contains " at ").

- [ ] **Step 2: Map**

In `src/provider/map.rs` where `down_distance:` is built (both sites the earlier grep showed at the situation construction), replace with a call to a helper:

```rust
/// Down and distance without the ball position: ESPN's `shortDownDistanceText`
/// ("1st & 10") when it is there (college feeds carry it, verified in
/// fixtures/live/cfb_scoreboard_live.json), else `downDistanceText` with its
/// trailing " at <possessionText>" removed so the row, which prints the
/// position from `ball_on`, never says it twice.
fn down_distance_from(sit_v: &Value) -> String {
    if let Some(short) = sit_v["shortDownDistanceText"].as_str().filter(|s| !s.is_empty()) {
        return short.to_string();
    }
    let long = sit_v["downDistanceText"].as_str().unwrap_or("");
    match sit_v["possessionText"].as_str().filter(|s| !s.is_empty()) {
        Some(pos) => {
            let suffix = format!(" at {pos}");
            long.strip_suffix(suffix.as_str())
                .or_else(|| {
                    let lower = long.to_ascii_lowercase();
                    lower.strip_suffix(suffix.to_ascii_lowercase().as_str()).map(|_| &long[..long.len() - suffix.len()])
                })
                .unwrap_or(long)
                .to_string()
        }
        None => long.to_string(),
    }
}
```

and `down_distance: down_distance_from(sit_v),`.

- [ ] **Step 3: Run**

Run: `cargo test --release --test map_espn --test draw 2>&1 | grep -E '^test result'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `ok` ×2; `547 tests`. If an existing draw test asserted `PHI 3RD & 6 AT DAL 38` from a hand-built `down_distance: "3rd & 6"`, it still passes (the row appends `AT` from `ball_on`).

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "fix(map): down and distance never repeats the ball position (shortDownDistanceText)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 6: D3 — the red zone needs a possessing team

**Files:**
- Modify: `src/provider/map.rs` (red zone yards), `src/rank.rs` (chip gate + its tests), `tests/map_espn.rs`

- [ ] **Step 1: Failing tests**

`tests/map_espn.rs`:

```rust
#[test]
fn a_red_zone_flag_without_possession_makes_no_meter() {
    let base = one_event_with_last_play(r#"{"id":"1","text":"x","scoringPlay":null,"scoreValue":0,"team":{"id":"1"},"type":{"id":"5","text":"Rush"}}"#);
    // Add the flag and a yard line; the base fixture's possession is team 1 (away).
    let with = base.replace(r#""possession":"1","#, r#""possession":"1","isRedZone":true,"yardLine":15,"#);
    let g = &map_scoreboard(League::Nfl, &with, et()).unwrap()[0];
    assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 15 }), "away attacks 0: 15 to go");
    let none = base.replace(r#""possession":"1","#, r#""possession":null,"isRedZone":true,"yardLine":15,"#);
    let g = &map_scoreboard(League::Nfl, &none, et()).unwrap()[0];
    assert_eq!(g.meter, None, "no possessing team, no meter (the review saw 100 TO GOAL)");
    assert!(g.situation.as_ref().unwrap().possession.is_none());
    let stranger = base.replace(r#""possession":"1","#, r#""possession":"999","isRedZone":true,"yardLine":15,"#);
    let g = &map_scoreboard(League::Nfl, &stranger, et()).unwrap()[0];
    assert_eq!(g.meter, None, "an id that is neither team is not a side");
}
```

In `src/rank.rs` tests, the existing red-zone test at the `rz.situation = Some(Situation { is_red_zone: Some(true), ..` site gains `possession: Some("KC".into()),` and a new case:

```rust
    #[test]
    fn a_red_zone_flag_without_possession_earns_no_chip() {
        let mut g = g(League::Nfl, "Q3", "9:05", 21, 17);
        g.situation = Some(Situation { is_red_zone: Some(true), possession: None, ..Default::default() });
        let w = watchability(&g, OffsetDateTime::now_utc());
        assert_eq!(w.chip, None);
        assert!(!w.hot);
    }
```

Run: `cargo test --release --test map_espn a_red_zone_flag 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL on the `none` case (meter is `Some(RedZone { yards_to_goal: 15 })` today because null possession reads as away).

- [ ] **Step 2: Map and gate**

`src/provider/map.rs`, the `red_zone_yards` closure:

```rust
    let red_zone_yards = situation.as_ref().and_then(|s| {
        if s.is_red_zone != Some(true) {
            return None;
        }
        // Which goal is attacked comes from who has the ball. A missing or
        // foreign possession id is not a side: no meter, rather than the
        // "100 TO GOAL" the review saw when null read as away.
        let possession_is_home = match sit_v["possession"].as_str() {
            Some(id) if id == home.id => true,
            Some(id) if id == away.id => false,
            _ => return None,
        };
        s.yard_line.map(|yl| yards_to_goal(yl, possession_is_home))
    });
```

`src/rank.rs` red zone arm:

```rust
            if g.situation.as_ref().is_some_and(|s| s.is_red_zone == Some(true) && s.possession.is_some()) {
                bonus!(40, Some("RED ZONE"));
            }
```

- [ ] **Step 3: Run**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'`
Expected: `549 tests, 0 failed`.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "fix(map,rank): the red zone needs a possessing team

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 7: D4 — proof that MLB labels read the pitch outcome, never `alternativeText`

**Files:**
- Modify: `tests/map_espn.rs`

The mapper already reads `type.text` (v3.4's final wave closed the OPEN item); the review still saw `MIA FOUL` because of D2, not D4. This task pins the label so it cannot regress.

- [ ] **Step 1: Write the test**

```rust
#[test]
fn mlb_last_play_reads_the_pitch_outcome_not_the_projected_at_bat() {
    let json = r#"{"events":[{"id":"5","competitions":[{"status":{"displayClock":"0:00","period":3,"type":{"state":"in","completed":false,"shortDetail":"Bot 3rd"}},"competitors":[
      {"homeAway":"away","score":"1","team":{"id":"16","abbreviation":"CHC","displayName":"Cubs","color":"0e3386","alternateColor":"cc3433"}},
      {"homeAway":"home","score":"0","team":{"id":"28","abbreviation":"MIA","displayName":"Marlins","color":"00a3e0","alternateColor":"ef3340"}}
    ],"situation":{"balls":0,"strikes":2,"outs":1,"onFirst":false,"onSecond":false,"onThird":false,
      "lastPlay":{"id":"401","text":"Pitch 2 : Strike 2 Looking","scoreValue":0,"scoringPlay":null,"team":{"id":"28"},
        "type":{"id":"36","text":"Strike Looking","abbreviation":"SL","alternativeText":"Strikeout","type":"strike-looking"},
        "athletesInvolved":[{"id":"1","shortName":"J. Ortiz"}]}}}]}]}"#;
    let g = &map_scoreboard(League::Mlb, json, et()).unwrap()[0];
    assert_eq!(g.last_plays[0].text, "Strike Looking — J. Ortiz");
    assert!(!g.last_plays[0].text.contains("Strikeout"), "alternativeText is a projection, not what happened");
    assert!(!g.last_plays[0].scoring);
}
```

Run: `cargo test --release --test map_espn mlb_last_play_reads 2>&1 | grep -E '^test result'`
Expected: `ok` (the test passes against current code; it exists to keep it that way). If it fails, the mapper regressed: fix `mlb_last_play_text` to read `type.text` and report it.

- [ ] **Step 2: Commit**

```bash
cargo fmt && git add tests/map_espn.rs && git commit -q -m "test(map): MLB last-play label reads the pitch outcome, never alternativeText

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 8: D5 and D7 — play rows show period and clock, and never the clock twice

**Files:**
- Modify: `src/tiles/mod.rs` (`play_stamp`, `play_line`), `src/views/zoom.rs` (uses the new stamp), `src/views/plays_feed.rs` (uses the new stamp), `tests/draw.rs`

**Interfaces:**
- Produces: `pub(crate) fn play_stamp(p: &Play) -> String` returning `"Q2 11:09"` when both period and clock are present, the clock alone when only a clock, the period alone when only a period, `"-:--"` when neither. `play_line` drops a leading `(m:ss) ` or `(mm:ss) ` from the drawn text when the play carries a non-empty `clock` (presentation only; `Play.text` untouched).

- [ ] **Step 1: Failing draw tests**

In `tests/draw.rs`, near the zoom tests:

```rust
#[test]
fn zoom_scoring_rows_carry_the_period_and_the_clock() {
    let mut app = mk();
    let mut game = g("1", "TOW", "NAVY", true);
    game.league = League::Cfb;
    game.scoring_plays = vec![
        Play { id: "a".into(), period: "Q1".into(), clock: "9:32".into(), team: "NAVY".into(), text: "Gutierrez run for 3 yds".into(), scoring: true, ..Default::default() },
        Play { id: "b".into(), period: "Q2".into(), clock: "14:52".into(), team: "TOW".into(), text: "Indorf pass to Enterline for 48 yds".into(), scoring: true, ..Default::default() },
    ];
    app.apply_boards(League::Cfb, vec![game], false);
    key(&mut app, crossterm::event::KeyCode::Char('z'));
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[Q2 14:52] TOW"), "{s}");
    assert!(s.contains("[Q1 9:32] NAVY"), "{s}");
}

#[test]
fn zoom_last_plays_do_not_print_the_clock_twice() {
    let mut app = mk();
    let mut game = g("1", "TOW", "NAVY", true);
    game.league = League::Cfb;
    game.last_plays = vec![Play { id: "c".into(), period: "Q2".into(), clock: "3:31".into(), team: "NAVY".into(), text: "(03:39) #11 J.Carlson punt 42 yards to the Towson04".into(), ..Default::default() }];
    app.apply_boards(League::Cfb, vec![game.clone()], false);
    key(&mut app, crossterm::event::KeyCode::Char('z'));
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[Q2 3:31] NAVY #11 J.Carlson punt"), "{s}");
    assert!(!s.contains("(03:39)"), "the feed's own clock prefix is not drawn beside ours:\n{s}");
    // The play text itself is untouched (presentation only).
    assert!(app.game_by_id_pub("1").unwrap().last_plays[0].text.starts_with("(03:39)"));
}
```

If `App::game_by_id` is not reachable from `tests/`, use `app.scoring_events()` style access or make `game_by_id` `pub`; say which in the report.

Run: `cargo test --release --test draw zoom_scoring_rows_carry 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL (`[14:52] TOW` today).

- [ ] **Step 2: Implement**

`src/tiles/mod.rs`:

```rust
/// The bracket stamp on a play row: period and clock when the feed gave
/// both (`Q2 11:09`, so a scoring list reads in order across quarters),
/// the clock alone for a scoreboard row that carries no period, the period
/// alone for baseball (`B9`), `-:--` when neither.
pub(crate) fn play_stamp(p: &crate::domain::Play) -> String {
    match (p.period.is_empty(), p.clock.is_empty()) {
        (false, false) => format!("{} {}", p.period, p.clock),
        (true, false) => p.clock.clone(),
        (false, true) => p.period.clone(),
        (true, true) => "-:--".to_string(),
    }
}

/// Presentation only: college play text carries its own `(mm:ss) ` prefix,
/// and a row that already prints the clock in brackets would show it twice.
/// `Play.text` is never changed; only what this row draws.
fn without_leading_clock(text: &str, has_clock: bool) -> &str {
    if !has_clock {
        return text;
    }
    let Some(rest) = text.strip_prefix('(') else { return text };
    let Some(end) = rest.find(')') else { return text };
    let inside = &rest[..end];
    let looks_like_clock = inside.len() <= 5
        && inside.split_once(':').is_some_and(|(m, s)| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()) && s.len() == 2 && s.chars().all(|c| c.is_ascii_digit()));
    if looks_like_clock {
        rest[end + 1..].trim_start()
    } else {
        text
    }
}
```

In `play_line`: `let text = truncate(without_leading_clock(&p.text, !p.clock.is_empty()), width.saturating_sub(used + 1));` and the `clock` head uses the new `play_stamp` (a `String` now). Update `src/views/zoom.rs` and `src/views/plays_feed.rs` call sites for the `String` return (`tiles::play_stamp(p)` is used in `format!`, which needs no change).

Add a unit test in `src/tiles/mod.rs` tests:

```rust
    #[test]
    fn leading_clock_prefix_is_dropped_only_when_the_row_has_its_own_clock() {
        assert_eq!(without_leading_clock("(03:39) punt 42 yards", true), "punt 42 yards");
        assert_eq!(without_leading_clock("(03:39) punt 42 yards", false), "(03:39) punt 42 yards");
        assert_eq!(without_leading_clock("(D. Klein KICK)", true), "(D. Klein KICK)");
        assert_eq!(without_leading_clock("Timeout Navy, clock 02:00", true), "Timeout Navy, clock 02:00");
    }
```

- [ ] **Step 3: Run**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'`
Expected: `553 tests, 0 failed`. Existing draw tests that asserted `[1:27]`-style stamps on scoreboard rows (no period) still pass; any that built a play with both `period` and `clock` and asserted the old stamp are updated to the new form, listed in the report.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests && git commit -q -m "fix(zoom): play rows stamp period and clock, and never print the feed's clock twice

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 9: D8 — a `0-0` record on a live or final game is not shown

**Files:**
- Modify: `src/board/hero.rs` (the record span), `tests/draw.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn a_zero_record_hides_once_the_game_is_underway() {
    let mut app = mk();
    let mut game = g("1", "BOIS", "ORE", true);
    game.away.record = "0-0".into();
    game.home.record = "1-0".into();
    app.apply_boards(League::Nfl, vec![game.clone()], false);
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("1-0"), "{s}");
    assert!(!s.contains("BOIS 0-0") && !s.contains("0-0 BOIS"), "a 0-0 during play says nothing:\n{s}");
    // Pre-game keeps it: 0-0 before the opener is true.
    let mut pre = g("2", "NE", "SEA", false);
    pre.away.record = "0-0".into();
    let mut app2 = mk();
    app2.apply_boards(League::Nfl, vec![pre], false);
    key(&mut app2, crossterm::event::KeyCode::Char('z'));
    let mut t2 = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t2.draw(|f| app2.draw(f)).unwrap();
    assert!(buf_text(&t2).contains("0-0"), "{}", buf_text(&t2));
}
```

Run: `cargo test --release --test draw a_zero_record_hides 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL.

- [ ] **Step 2: Implement**

In `src/board/hero.rs` where `let record = (!team.record.is_empty()).then(…)`, the identity-line function needs the game's status; thread `status: Status` into it from the caller (the hero draws one game; pass `game.status`), and:

```rust
    // ESPN sends "0-0" for a team mid-game more often than it should (the
    // review saw Texas and Oregon at 0-0 in week two). A 0-0 during or
    // after a game is never information, so it is not drawn; pre-game it
    // is true and stays.
    let shows_record = !team.record.is_empty() && !(team.record == "0-0" && status != Status::Pre);
    let record = shows_record.then(|| Span::styled(team.record.clone(), Style::default().fg(r.dim)));
```

- [ ] **Step 3: Run and commit**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'` → `554 tests, 0 failed`.

```bash
cargo fmt && git add -A src tests && git commit -q -m "fix(hero): a 0-0 record on a game underway is not drawn

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 10: D9 — the unknown-theme note reaches the footer

**Files:**
- Modify: `src/theme.rs`, `src/main.rs`, `tests/theme.rs`, `tests/draw.rs`

**Interfaces:**
- Produces: `pub fn select_or_default_noting(name: &str) -> (String, Option<String>)`; `select_or_default` keeps its signature and calls it. The note text: `theme "<name>" not found · using broadcast · themes/ loads user files`.

- [ ] **Step 1: Failing tests**

`tests/theme.rs`:

```rust
#[test]
fn an_unknown_theme_name_falls_back_with_a_footer_sized_note() {
    let (name, note) = gameday::theme::select_or_default_noting("dracula");
    assert_eq!(name, "broadcast");
    let note = note.expect("a note for the footer");
    assert!(note.contains("\"dracula\"") && note.contains("broadcast") && note.contains("themes/"), "{note}");
    let (name, note) = gameday::theme::select_or_default_noting("studio");
    assert_eq!(name, "studio");
    assert_eq!(note, None);
}
```

`tests/draw.rs`:

```rust
#[test]
fn the_theme_note_is_in_the_footer_at_startup() {
    let mut app = mk();
    let (_, note) = gameday::theme::select_or_default_noting("dracula");
    app.status_line = note;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.lines().last().unwrap().contains("not found"), "{s}");
}
```

Run: `cargo test --release --test theme an_unknown_theme_name 2>&1 | grep -E 'error\[|test result' | head -2`
Expected: compile error.

- [ ] **Step 2: Implement**

`src/theme.rs`:

```rust
/// Lenient select for config values with the note the footer shows: an
/// unknown name falls back to broadcast and says so on screen, because the
/// stderr line `select_or_default` prints is swallowed the moment the
/// alternate screen opens (the review watched it vanish).
pub fn select_or_default_noting(name: &str) -> (String, Option<String>) {
    match set_current(name) {
        Ok(canonical) => (canonical, None),
        Err(_) => {
            let fallback = set_current("broadcast").expect("broadcast is always loaded");
            let note = format!("theme {name:?} not found · using {fallback} · themes/ loads user files");
            (fallback, Some(note))
        }
    }
}

pub fn select_or_default(name: &str) -> String {
    let (name, note) = select_or_default_noting(name);
    if let Some(note) = note {
        eprintln!("gameday: {note}");
    }
    name
}
```

`src/main.rs`: replace `config.theme = gameday::theme::select_or_default(&config.theme);` with

```rust
    let (theme_name, theme_note) = gameday::theme::select_or_default_noting(&config.theme);
    if let Some(note) = &theme_note {
        eprintln!("gameday: {note}");
    }
    config.theme = theme_name;
```

and after `app.set_config_error(config_error);` add `if app.status_line.is_none() { app.status_line = theme_note; }` (a config parse error keeps priority; it already lands in the footer).

- [ ] **Step 3: Run and commit**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'` → `556 tests, 0 failed`.

```bash
cargo fmt && git add -A src tests && git commit -q -m "fix(theme): an unknown theme name is reported in the footer, not only on the swallowed stderr

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 11: D10 — standings say PRESEASON or POSTSEASON when the feed does

**Files:**
- Modify: `src/domain.rs` (`StandingsTable.season_type`), `src/provider/map.rs` (`map_standings`), `src/views/standings.rs` (`label`), every `StandingsTable { … }` literal (`src/app/tests.rs`, `tests/draw.rs`, `src/provider/map.rs`), `tests/map_espn.rs`, `tests/draw.rs`

Probe receipt (2026-09-06, live `apis/v2/sports/football/nfl/standings`): the top level carries `season.displayName = "2026"`; each `children[].standings` carries `seasonType` (ESPN's convention: 1 preseason, 2 regular season, 3 postseason; the live value today was `2`). The review's preseason table on 2026-09-05 would have carried `1`.

- [ ] **Step 1: Failing tests**

`tests/map_espn.rs`:

```rust
#[test]
fn standings_carry_the_feeds_season_type() {
    let regular = include_str!("../fixtures/nfl_standings.json");
    let t = map_standings(League::Nfl, regular).unwrap();
    // The committed fixture may predate the field; either it says 2 or nothing.
    assert!(matches!(t.season_type, None | Some(2)), "{:?}", t.season_type);
    let pre = regular.replacen(r#""standings":{"#, r#""standings":{"seasonType":1,"#, 1);
    assert_ne!(pre, regular, "the fixture has a standings object to stamp");
    let t = map_standings(League::Nfl, &pre).unwrap();
    assert_eq!(t.season_type, Some(1));
}
```

`tests/draw.rs`, next to `standings_header_carries_the_season_else_when_it_was_fetched` (reuse `standings_table()`):

```rust
#[test]
fn a_preseason_table_says_so_in_the_header() {
    let mut app = mk();
    let mut table = standings_table();
    table.season = Some("2026".into());
    table.season_type = Some(1);
    app.merge_standings(table);
    key(&mut app, crossterm::event::KeyCode::Char(':'));
    for c in "standings".chars() { key(&mut app, crossterm::event::KeyCode::Char(c)); }
    key(&mut app, crossterm::event::KeyCode::Enter);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("2026 PRESEASON"), "{s}");
}
```

(If `standings_table()` builds the table with a struct literal, add `season_type: None` there and at the other literal sites the compiler names.)

Run: `cargo test --release --test map_espn standings_carry 2>&1 | grep -E 'error\[|test result' | head -2`
Expected: compile error (`season_type` unknown).

- [ ] **Step 2: Implement**

`src/domain.rs` `StandingsTable`: add

```rust
    /// ESPN's `children[].standings.seasonType`: 1 preseason, 2 regular
    /// season, 3 postseason (probed live 2026-09-06: 2). None when the feed
    /// carries none. The header prints PRESEASON/POSTSEASON from it so an
    /// August table never reads as this season's.
    pub season_type: Option<u8>,
```

`src/provider/map.rs` `map_standings`: read `v["children"][0]["standings"]["seasonType"].as_u64().map(|n| n.min(255) as u8)` into `season_type`, and set it in the `StandingsTable { … }` literal.

`src/views/standings.rs` `label`:

```rust
fn label(table: &StandingsTable) -> Option<String> {
    let phase = match table.season_type {
        Some(1) => " PRESEASON",
        Some(3) => " POSTSEASON",
        _ => "",
    };
    match &table.season {
        Some(season) => Some(format!("{season}{phase}")),
        None => table
            .fetched_at
            .map(|t| format!("updated {}{phase}", crate::text::fmt_hm12(t))),
    }
}
```

- [ ] **Step 3: Run and commit**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'` → `558 tests, 0 failed`.

```bash
cargo fmt && git add -A src tests && git commit -q -m "feat(standings): PRESEASON/POSTSEASON from the feed's seasonType

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 12: Replay capture tooling and the first MLB sequences

**Files:**
- Create: `scripts/capture-replay.sh`, `fixtures/replay/PROVENANCE.md`, `fixtures/replay/mlb-<stamp>/…` (at least two sequences containing a run)
- Modify: `src/provider/memory.rs` (`insert_summary`), `Cargo.toml` (`exclude` already lists `fixtures/replay/`; confirm)

**Interfaces:**
- Produces: a directory per capture, `fixtures/replay/<league>-<yyyymmdd-hhmm>/NN.json` (zero-padded, 15 s apart, each a full scoreboard payload filtered to one event when `event_id` is given), `summary-<event_id>.json` (fetched once at the end for every live event in the last payload, or the given one), and a `PROVENANCE.md` row.

- [ ] **Step 1: Write the script**

`scripts/capture-replay.sh`:

```bash
#!/usr/bin/env bash
# Capture consecutive scoreboard polls for replay tests.
#
#   scripts/capture-replay.sh <league> <minutes> [event_id]
#
# Every 15 s (the live cadence) the scoreboard is fetched and written as
# fixtures/replay/<league>-<UTC yyyymmdd-hhmm>/NN.json — the full payload,
# untouched, or filtered to `events[] | select(.id == event_id)` so a
# sequence stays small. When the loop ends, the summary for each live event
# in the last payload (or the given one) is written beside them as
# summary-<id>.json. The script prints which consecutive pairs carry a score
# delta; a sequence with no delta is not worth committing — delete it.
#
# Plain lists, no bash-4 features: macOS ships bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."
league="${1:?league slug (nfl cfb cbb nba wnba nhl mlb epl mls)}"
minutes="${2:?minutes to run}"
event="${3:-}"
UA="gameday/replay-capture (+https://github.com/WallyMagill/gameday)"
B="https://site.web.api.espn.com/apis/site/v2/sports"
case "$league" in
  nfl)  path=football/nfl ;;
  cfb)  path="football/college-football"; extra="&groups=80&limit=300" ;;
  cbb)  path=basketball/mens-college-basketball ;;
  nba)  path=basketball/nba ;;
  wnba) path=basketball/wnba ;;
  nhl)  path=hockey/nhl ;;
  mlb)  path=baseball/mlb ;;
  epl)  path=soccer/eng.1 ;;
  mls)  path=soccer/usa.1 ;;
  *) echo "unknown league $league" >&2; exit 2 ;;
esac
extra="${extra:-}"
stamp=$(date -u +%Y%m%d-%H%M)
dir="fixtures/replay/${league}-${stamp}"
mkdir -p "$dir"
polls=$(( minutes * 60 / 15 ))
echo "capturing $polls polls into $dir"
i=0
while [ "$i" -lt "$polls" ]; do
  n=$(printf '%02d' "$i")
  if [ -n "$event" ]; then
    curl -sf -A "$UA" "$B/$path/scoreboard?limit=300$extra" | jq --arg id "$event" '{events: [.events[] | select(.id == $id)]}' > "$dir/$n.json"
  else
    curl -sf -A "$UA" "$B/$path/scoreboard?limit=300$extra" | jq '.' > "$dir/$n.json"
  fi
  i=$(( i + 1 ))
  [ "$i" -lt "$polls" ] && sleep 15
done
last="$dir/$(printf '%02d' $(( polls - 1 ))).json"
if [ -n "$event" ]; then ids="$event"; else ids=$(jq -r '.events[] | select(.status.type.state=="in") | .id' "$last"); fi
for id in $ids; do
  curl -sf -A "$UA" "$B/$path/summary?event=$id" | jq '.' > "$dir/summary-$id.json"
  sleep 1
done
# Which consecutive pairs carry a score delta, per event.
prev=""
for f in "$dir"/[0-9][0-9].json; do
  if [ -n "$prev" ]; then
    jq -n --slurpfile a "$prev" --slurpfile b "$f" '
      [ $a[0].events[] as $e | ($b[0].events[] | select(.id == $e.id)) as $n
        | ($e.competitions[0].competitors | map(.score)) as $s0
        | ($n.competitions[0].competitors | map(.score)) as $s1
        | select($s0 != $s1) | "\($e.id) \($s0) -> \($s1)" ] | .[]' -r | sed "s|^|$(basename "$prev") -> $(basename "$f"): |"
  fi
  prev="$f"
done
echo "done: $dir"
```

`chmod +x scripts/capture-replay.sh`; `bash -n scripts/capture-replay.sh`.

- [ ] **Step 2: `MemoryProvider::insert_summary`**

```rust
    pub fn insert_summary(&mut self, game_id: &str, summary: Summary) {
        self.summaries.insert(game_id.to_string(), summary);
    }
```

- [ ] **Step 3: Capture two MLB sequences with a run**

MLB is live daily (evenings US; day games some days). Find a live game: `./target/release/gameday probe mlb | grep Live`. Run `scripts/capture-replay.sh mlb 20 <event_id>` on one live game (about 80 polls, filtered; about 4 MB before jq compaction; acceptable because `fixtures/replay/` is excluded from the crate). Repeat for a second game or a second window. Keep only sequences where the delta report printed at least one line; delete the rest (`rm -rf`). If no run lands in 20 minutes, run again; baseball averages a run every 15 minutes of game time across two teams, so two 20-minute windows usually suffice. Record each kept sequence in `fixtures/replay/PROVENANCE.md`:

```markdown
# fixtures/replay/ provenance

Consecutive scoreboard polls, 15 s apart, captured by `scripts/capture-replay.sh` — full payloads (filtered to one event where noted), then the summary for that event fetched once after the last poll. Used by `tests/replay.rs`, which feeds each sequence through the real apply/merge path and asserts the scoring cut names the actual scoring play.

| Directory | League / event | Captured (UTC) | Polls | Deltas (pair: scores) | Notes |
|---|---|---|---|---|---|
| `mlb-<stamp>` | MLB `<id>` AWAY @ HOME | <date time> | NN | `07 -> 08: ["1","0"] -> ["1","1"]` | <what the scoring play was, from the summary> |
```

- [ ] **Step 4: Commit**

```bash
git add scripts/capture-replay.sh src/provider/memory.rs fixtures/replay && git commit -q -m "test(replay): consecutive-poll capture script and the first MLB sequences

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 13: The replay harness

**Files:**
- Create: `tests/replay.rs`

**Interfaces:**
- Consumes: `fixtures/replay/*/NN.json` + `summary-<id>.json`; `map_scoreboard`, `map_summary`; `App::apply_boards`, `App::merge_summary`, `App::catchup_wants`, `App::cuts_fired`, `App::scoring_events`.

- [ ] **Step 1: Write the harness**

```rust
//! Replay of consecutive real scoreboard polls through the real apply path.
//! Every sequence under fixtures/replay/ is one live window captured by
//! scripts/capture-replay.sh. The assertion is the one the 2026-09-05 review
//! found broken on live data: when a score moves, the cut names the play
//! that scored — never the pitch or snap the poll happened to catch.
use gameday::app::App;
use gameday::config::Config;
use gameday::domain::*;
use gameday::provider::map::{map_scoreboard, map_summary};
use std::path::{Path, PathBuf};

fn app() -> App {
    let dir = std::env::temp_dir().join(format!("gd-replay-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
    // Ten seconds of ticks per poll keeps the catch-up TTL honest: a poll is
    // 15 s live, and the TTL is four polls.
    app.now_override = Some(time::OffsetDateTime::now_utc());
    app
}

fn sequences() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/replay");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("fixtures/replay must exist: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no replay sequences under {}: the harness must not pass vacuously", root.display());
    dirs
}

fn league_of(dir: &Path) -> League {
    let name = dir.file_name().unwrap().to_string_lossy();
    let slug = name.split('-').next().unwrap();
    League::from_slug(slug).unwrap_or_else(|| panic!("directory {name} does not start with a league slug"))
}

fn polls(dir: &Path) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().unwrap().to_string_lossy().chars().next().unwrap().is_ascii_digit()).collect();
    files.sort();
    files.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect()
}

fn summary_for(dir: &Path, id: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(format!("summary-{id}.json"))).ok()
}

#[test]
fn every_score_delta_in_every_sequence_yields_exactly_one_cut_naming_a_scoring_play() {
    for dir in sequences() {
        let league = league_of(&dir);
        let mut app = app();
        let mut deltas = 0u32;
        let mut prev: std::collections::HashMap<String, (u16, u16)> = Default::default();
        for (i, body) in polls(&dir).iter().enumerate() {
            let games = map_scoreboard(league, body, time::UtcOffset::UTC).unwrap_or_else(|e| panic!("{}: poll {i}: {e}", dir.display()));
            for g in &games {
                let score = (g.away_score, g.home_score);
                if let Some(p) = prev.get(&g.id) {
                    if *p != score { deltas += 1; }
                }
                prev.insert(g.id.clone(), score);
            }
            let before = app.cuts_fired();
            app.apply_boards(league, games, false);
            // Ten render ticks per poll so the catch-up TTL is measured in polls.
            for _ in 0..10 { app.advance_tick(); }
            // Serve every queued catch-up from the captured summary, as the
            // poll thread would.
            for c in app.catchup_wants() {
                let body = summary_for(&dir, &c.game_id).unwrap_or_else(|| panic!("{}: poll {i} queued a catch-up for {} but no summary-{}.json was captured", dir.display(), c.game_id, c.game_id));
                let s = map_summary(c.league, &body).unwrap();
                app.merge_summary(&c.game_id, s);
            }
            let fired = app.cuts_fired() - before;
            assert!(fired <= 1, "{}: poll {i} fired {fired} cuts", dir.display());
        }
        assert!(deltas > 0, "{}: no score delta in this sequence; it does not earn its place", dir.display());
        assert_eq!(app.cuts_fired(), deltas, "{}: {deltas} score deltas but {} cuts", dir.display(), app.cuts_fired());
        assert!(app.catchup_wants().is_empty(), "{}: a catch-up was left pending", dir.display());
        // Every captured scoring play is a real scoring play of the right
        // kind, credited to one of the two teams.
        for (game, play) in app.scoring_events() {
            assert!(play.scoring, "{}: {:?}", dir.display(), play.text);
            assert!(!play.id.is_empty(), "{}: a captured play without an id: {:?}", dir.display(), play.text);
            assert!(play.team == game.away.abbr || play.team == game.home.abbr, "{}: credited to {:?}, teams {} {}", dir.display(), play.team, game.away.abbr, game.home.abbr);
            let kinds_ok = match league {
                League::Mlb => matches!(play.kind, PlayKind::HomeRun | PlayKind::RunScoringPlay),
                League::Nfl | League::Cfb => matches!(play.kind, PlayKind::Touchdown | PlayKind::FieldGoal | PlayKind::Safety),
                League::Nhl | League::Epl | League::Mls => matches!(play.kind, PlayKind::Goal | PlayKind::OwnGoal | PlayKind::PenaltyGoal),
                _ => true,
            };
            assert!(kinds_ok, "{}: {:?} is not a scoring kind for {:?}: {:?}", dir.display(), play.kind, league, play.text);
        }
    }
}
```

If `App::now_override` is not `pub`, drop that line (it is a nicety, not a requirement). If `Config::default_all` is not `pub`, use whatever `tests/draw.rs`'s `mk()` uses.

- [ ] **Step 2: Run against the captured sequences**

Run: `cargo test --release --test replay 2>&1 | grep -E 'test result|panicked|assertion' | head -5`
Expected: `ok` for every MLB sequence. If a sequence fails because the summary's scoring play is credited to a team abbreviation that differs from the scoreboard's (ESPN sometimes abbreviates differently between endpoints), that is a mapper finding: fix `team_of` in `map_summary` to prefer the header's `abbreviation` and report it; do not loosen the assertion.

- [ ] **Step 3: Commit**

```bash
cargo fmt && git add tests/replay.rs && git commit -q -m "test(replay): consecutive real polls through the real apply path assert one cut per score, naming the scoring play

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 14: Budget receipt — catch-up requests in a live window

**Files:**
- Modify: `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§9), `CHANGELOG.md`

- [ ] **Step 1: Run the release binary for a live window with the request log**

```bash
cargo build --release
S=$(mktemp -d)
tmux kill-session -t w1 2>/dev/null; tmux new-session -d -s w1 -x 120 -y 36
tmux send-keys -t w1 "GAMEDAY_LOG_REQUESTS=1 ./target/release/gameday --config-dir $S 2>$S/requests.log" Enter
# Leave it 20 minutes during a window with MLB live (evenings US). Do not zoom.
sleep 1200
tmux send-keys -t w1 q; sleep 1; tmux kill-session -t w1
total=$(grep -c 'Scoreboard\|Summary\|Stats\|Dated\|Standings' $S/requests.log)
catch=$(grep -c 'Summary(' $S/requests.log)
echo "total=$total catchups=$catch per_min_total=$(( total / 20 )) per_min_catchups=$(( catch / 20 ))"
grep 'Summary(' $S/requests.log | head
```

Expected: `per_min_total` under 45; `catchups` roughly equal to the number of scores across all live games in the window (compare against the deltas you saw on the board). Record the four numbers.

- [ ] **Step 2: Append to §9**

Under `## §9 Verification` add:

```markdown
### Wave 1 — landed <date>

- Suite: 532 → <n> tests. Replay sequences: <list dirs>, each with ≥1 score delta, all passing `tests/replay.rs`.
- Catch-up budget, live window <date time UTC, 20 min, leagues live: …>: <total> requests (<per-min> /min), <catch> catch-up summaries for <deltas> observed score changes.
- D1–D10: closed by tasks 5–11 (D6 recorded as feed pass-through; D4 pinned by test; D10 probe: `children[].standings.seasonType` present, value 2 on 2026-09-06).
- Pending for this wave's DoD: one CFB replay sequence containing a touchdown (next window: Saturday 2026-09-12); captured with `scripts/capture-replay.sh cfb 30 <event>` and added to `fixtures/replay/` under the same harness. NFL sequences land in wave 6 (spec §8.5).
```

`CHANGELOG.md` `### Fixed`: the scoring cut names the actual scoring play (one-shot summary catch-up); college rows no longer print the field position twice; a red zone chip needs a possessing team; zoom rows show period and clock and never the clock twice; a 0-0 record mid-game is hidden; an unknown theme is reported in the footer; standings say PRESEASON/POSTSEASON.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md CHANGELOG.md && git commit -q -m "docs(v4): wave 1 receipts — replay sequences, catch-up budget

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 15 (gated on the CFB window): a CFB replay sequence with a touchdown

**Files:**
- Create: `fixtures/replay/cfb-<stamp>/…`; modify `fixtures/replay/PROVENANCE.md`, spec §9

This task runs when college football is live (Saturday). It is the last item of the wave's definition of done; waves 2 and 3 do not wait for it.

- [ ] **Step 1: Capture** — pick a live game with a close score (`./target/release/gameday probe cfb | grep Live`), run `scripts/capture-replay.sh cfb 30 <event_id>`; keep the sequence only if the delta report shows at least one scoring change; record it in `PROVENANCE.md` with the scoring play's text from the summary.
- [ ] **Step 2: Run** `cargo test --release --test replay 2>&1 | grep -E 'test result|panicked'` → `ok`. A failure here is a real finding about football feeds (for example the summary's `scoringPlays` crediting `team.abbreviation` differently); fix the mapper, never the assertion, and report.
- [ ] **Step 3: Commit and update §9's "Pending" line to "landed".**

```bash
git add fixtures/replay docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md && git commit -q -m "test(replay): a CFB sequence with a touchdown

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

## Self-review against spec §3

- §3.1 (`Play.id`, period on every league, dedupe by id-then-text) → Tasks 1, 4. §3.2 (catch-up plumbing: `Wants.catchup`, sequence numbers, `CATCHUP_TTL`, budget) → Tasks 3, 4, 14. §3.3 D1 → 5; D3 → 6; D4 → 7; D5, D7 → 8; D6 → recorded in Task 14's §9 text; D8 → 9; D9 → 10; D10 → 11 (probe done, field present). §3.4 capture tooling and sequences → 12, 15. §3.5 replay harness → 13; mapper tests → 1, 2, 5, 6, 7, 11; D1 draw tooth → 5; D9 footer test → 10. The four grid teeth belong to wave 3 (spec §5.7), as the spec says.
- Names: `same_play` (Task 4) is `pub(crate)` in `merge.rs` and reached from `src/app/tests.rs` as `crate::app::merge::same_play` (make `mod merge` `pub(crate)` in `app/mod.rs` if it is private); `play_stamp` returns `String` (Task 8) and every call site is inside `format!`; `CatchupReq` fields are public (Task 3) and built in `catchup_wants` (Task 4); `season_type` is added to every `StandingsTable` literal (Task 11); `cuts_fired` and `scoring_events` are `pub` for `tests/replay.rs` (Tasks 4, 13).
- Test count ledger: 532 → 534 (T1) → 535 (T2) → 538 (T3) → 545 (T4) → 547 (T5) → 549 (T6) → 550 (T7) → 553 (T8) → 554 (T9) → 556 (T10) → 558 (T11) → 559 (T13). Counts are expectations; the measured number wins and is reported.
