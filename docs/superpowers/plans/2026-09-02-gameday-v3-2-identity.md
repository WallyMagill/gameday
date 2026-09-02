# gameday v3 · sub-project 2 — Identity · Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the tile mosaic with the approved A′ ranked board — team-color hero with 16×10 logo flanks, MY GAMES band, three tiers, scoped scoring takeover, `:tv` mode, role-based themes — so the board is composed at every size and in every season.

**Architecture:** A pure `rank` module scores watchability and owns event-gated ordering; a `board` module renders one ranked list through a height-budget layout (hero → promoted rows → single lines → dim finals/later) with no borders, no sidebar, no paging; the cut overlay and `:tv` reuse the hero's own render functions so the score can never disagree with the board; themes collapse to roles (ground/ink/dim/digits/hot/cool + team-on-hero).

**Tech Stack:** Rust 2021, ratatui 0.29, tui-big-text 0.7 (`PixelSize::Full` renders letters as well as digits — the cut word needs no glyph table), crossterm 0.28, `time` 0.3. Tests: unit + `ratatui::TestBackend`; no network in tests.

**Spec:** `docs/superpowers/specs/2026-09-01-gameday-v3-2-identity-design.md` — read it first; the reference frames it binds to are `docs/research/v3-identity/*.png` and the logo study in `docs/research/v3-identity/logo-study/`. Every task cites its spec section.

## Global Constraints

- No new crates. No network in tests; the one network step is Task 4's logo regeneration (dev-time script, run once).
- `cargo test` green and `cargo clippy --all-targets` **zero warnings** at the end of every task (the tree starts clean — keep it clean).
- No bare `cargo fmt` — the repo is not rustfmt-clean.
- Every numeric constant carries a one-line receipt comment (measured where, or guess and why).
- Color discipline (spec §6): board body amber (`digits` role) and greys; `hot` for the mark/chips/cut; team color ONLY on hero digits, hero logos, and pinned-team abbrs. Team colors pass the existing `ART_FLOOR` lift; hero pairs within 20° hue AND 15% luminance draw the home side in `digits` amber with a one-cell team block (spec §6).
- The cut renders from the same `Game`/`Play` via the same functions as the hero — never a second formatter (spec Decisions; Task 11 enforces with a cell-equality test).
- Ordering never changes under the user's eyes except on an event (score change, `hot` flip, status change, game enter/leave) — spec §2. `MY GAMES` never re-sorts.
- Key grammar stays k9s (`:` `/` `?` Esc-pop); removed: `n/p` paging, `1/2/4/s` layouts, `:layout`, `:score`, `ScoreStyle`, `LayoutPref` (old config keys are ignored silently — serde tolerates unknown keys — with one README line).
- Renderer implementers look at their own output: after any task that changes drawing, run `cargo run --release -- dump` and open the named `.ansi`/`.png` before committing (`GAMEDAY_DUMP_FONT` for PNGs).
- The A′ frames are the visual contract: when a task says "matches the frame", open the frame PNG next to your capture.

## File Structure

| File | Responsibility |
|---|---|
| `src/rank.rs` (new) | `watchability(&Game, now) -> Watch` (pure, per-league), `SortKey`, `OrderState` (event-gated order + `↑n` nudges) |
| `src/board/mod.rs` (new) | The board view: sections (MY GAMES / IN PLAY / FINAL / LATER), selection + scrolling, empty states; replaces `src/views/board.rs` |
| `src/board/layout.rs` (new) | Pure tier budgeting: (height, counts) → `TierPlan`; the sizes ladder (spec §4) |
| `src/board/hero.rs` (new) | Hero block: nameplates, team-color digits + hue-separation, state chip, fragment line, meter row, last play, 16×10 logo flanks; `score_block()` shared with cut/TV |
| `src/board/rows.rs` (new) | Tier-1 (3-row, sextant digits), tier-2 (single line), tier-3 (dim final/later) renderers; hot mark + nudge gutter |
| `src/board/cut.rs` (new) | Scoring takeover (full-screen, `tui-big-text` word) + 2-row band; firing/scoping/suppression state |
| `src/views/tv.rs` (new) | `View::Tv`: hero at full scale + ALSO LIVE strip + auto-cut/lock |
| `src/views/zoom.rs` (rewrite body) | Hero reuse + linescore + matchup/timeouts + PLAYS/STATS tabs |
| `src/theme.rs` | `Roles` replace `Discipline` (compat-read); `BUILTIN_NAMES` → 3; hue-separation helper |
| `src/tiles/logo.rs` → `src/board/logo.rs` | Slimmed: 16×10 hero marks only |
| `src/tiles/mod.rs` | Shrinks: digits/meter/compact-row helpers move under `src/board/`; tile grammar, borders, MOMENTUM, `Density`, `ScoreStyle` deleted |
| `src/tiles/packer.rs` | Deleted (with `tests/packer.rs`) |
| `src/app/{mod,derive,chrome}.rs` | `OrderState` on App; `Derived` gains ordered sections; header counts+SORT; footer legend; ticker gating |
| `src/{command,input,keymap,config}.rs` | `:sort`, `:tv`, `v`, `s`=sort; layout/score removed; `sort` config key |
| `src/{demo,sim,dump}.rs` | Demo data gains hot states + at-bat MLB plays; gallery rebuilt |
| `assets/logos/**` | Regenerated at 16×10 (38 marks) |

Task order: 1–5 are foundations (rank, order, themes, logos, layout — 3 and 4 independent of 1–2); 6–8 build the board; 9–10 chrome/keys; 11–12 cut/TV; 13 zoom; 14 sizes; 15 gallery/demo; 16 DoD. Sequential dispatch as in v3.1.

---

### Task 1: `rank.rs` — watchability (spec §2)

**Files:**
- Create: `src/rank.rs`; add `pub mod rank;` to `src/lib.rs`
- Test: `src/rank.rs` unit tests

**Interfaces:**
- Produces (exact):

```rust
/// One game's watchability verdict. `score` orders the board; `hot` drives
/// the 2-state mark; `chip` is the hero/state label ("RED ZONE", "2-MIN",
/// "TYING RUN ON 3RD", "BASES LOADED", "STOPPAGE", …) or None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch { pub score: u32, pub hot: bool, pub chip: Option<&'static str> }

pub fn watchability(g: &Game, now: time::OffsetDateTime) -> Watch;
/// Lateness 0..=100 from the sport's own clock grammar; 0 when unparsed
/// (an unparsed game can never lead the board).
pub fn lateness(league: League, period: &str, clock: &str) -> u32;
/// Closeness 0..=100 from margin scaled per sport.
pub fn closeness(league: League, margin: u32) -> u32;
```

- [ ] **Step 1: Write the failing tests** — in `src/rank.rs` `#[cfg(test)]`:

```rust
    fn g(league: League, period: &str, clock: &str, away: u16, home: u16) -> Game {
        Game {
            league, period: period.into(), clock: clock.into(),
            status: Status::Live, away_score: away, home_score: home,
            away: Team { abbr: "AAA".into(), ..Default::default() },
            home: Team { abbr: "HHH".into(), ..Default::default() },
            ..Default::default()
        }
    }
    fn now() -> OffsetDateTime { time::macros::datetime!(2026-09-13 16:47 -4) }

    #[test]
    fn lateness_reads_every_league_clock_grammar() {
        // Real strings from the nine leagues' fixtures.
        assert!(lateness(League::Nfl, "Q4", "1:52") > lateness(League::Nfl, "Q1", "12:00"));
        assert!(lateness(League::Mlb, "BOT 9TH", "") > lateness(League::Mlb, "TOP 2ND", ""));
        assert!(lateness(League::Epl, "78'", "") > lateness(League::Epl, "12'", ""));
        assert!(lateness(League::Epl, "90'+3'", "") >= 95, "stoppage is maximal");
        assert!(lateness(League::Cbb, "2ND HALF", "3:10") > lateness(League::Cbb, "1ST HALF", "12:00"));
        assert!(lateness(League::Nhl, "3RD", "4:00") > lateness(League::Nhl, "1ST", "10:00"));
        assert_eq!(lateness(League::Nfl, "HALFTIME", ""), 0, "unparsed period scores 0");
        assert_eq!(lateness(League::Mlb, "", ""), 0);
    }

    #[test]
    fn closeness_uses_the_sports_own_scale() {
        // One-score football game is close; 17 points is not.
        assert!(closeness(League::Nfl, 3) > closeness(League::Nfl, 17));
        assert_eq!(closeness(League::Nfl, 0), 100);
        // A 1-run baseball game beats a 4-run one; hockey/soccer margins are goals.
        assert!(closeness(League::Mlb, 1) > closeness(League::Mlb, 4));
        assert!(closeness(League::Nhl, 1) > closeness(League::Nhl, 3));
        assert!(closeness(League::Nba, 5) > closeness(League::Nba, 18));
    }

    #[test]
    fn a_late_close_game_outranks_an_early_blowout_and_bonuses_make_hot() {
        let close_late = g(League::Nfl, "Q4", "1:52", 24, 21);
        let blowout_early = g(League::Nfl, "Q1", "12:00", 28, 0);
        assert!(watchability(&close_late, now()).score > watchability(&blowout_early, now()).score);
        // Red zone: hot + chip regardless of margin.
        let mut rz = g(League::Nfl, "Q3", "9:05", 31, 3);
        rz.meter = Some(Meter::RedZone { yards_to_goal: 4 });
        let w = watchability(&rz, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("RED ZONE"));
        // Two-minute window: Q4/Q2 under 2:00.
        let w2 = watchability(&g(League::Nfl, "Q4", "0:48", 17, 17), now());
        assert!(w2.hot);
        assert_eq!(w2.chip, Some("2-MIN"));
    }

    #[test]
    fn baseball_bonuses_bases_loaded_and_tying_run_late() {
        let mut bl = g(League::Mlb, "BOT 4TH", "", 2, 3);
        bl.situation = Some(Situation { on_base: Some([true, true, true]), outs: Some(1), balls: Some(3), strikes: Some(2), ..Default::default() });
        let w = watchability(&bl, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("BASES LOADED"));
        // Tying run on base in the 8th+ (batting team down 1, runner on 3rd).
        let mut ty = g(League::Mlb, "BOT 9TH", "", 8, 7); // home batting, down 1
        ty.situation = Some(Situation { on_base: Some([false, false, true]), outs: Some(2), balls: Some(1), strikes: Some(0), ..Default::default() });
        let w = watchability(&ty, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("TYING RUN ON"), "chip prefix; the base is appended by the caller");
    }

    #[test]
    fn non_live_games_score_zero_and_never_chip() {
        let mut f = g(League::Nfl, "Q4", "", 24, 21);
        f.status = Status::Final;
        assert_eq!(watchability(&f, now()), Watch { score: 0, hot: false, chip: None });
        let mut p = g(League::Nfl, "", "", 0, 0);
        p.status = Status::Pre;
        assert_eq!(watchability(&p, now()).score, 0);
    }

    #[test]
    fn soccer_stoppage_within_one_goal_is_hot() {
        let w = watchability(&g(League::Epl, "90'+2'", "", 1, 1), now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("STOPPAGE"));
        let w2 = watchability(&g(League::Epl, "90'+2'", "", 4, 0), now());
        assert!(!w2.hot, "a stoppage blowout is not hot");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib rank 2>&1 | tail -3`
Expected: compile error (module does not exist).

- [ ] **Step 3: Implement** `src/rank.rs`:

```rust
//! Watchability: which game deserves the hero and the hot mark. Pure
//! functions of the Game — the clock grammar strings come straight from the
//! mapper (fixtures verified per league), and an unparsed string scores 0 so
//! bad data can never lead the board (spec §2).
use crate::domain::{Game, League, Meter, Status};
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch { pub score: u32, pub hot: bool, pub chip: Option<&'static str> }

/// Sport scale for "one score": margins at or under this are maximally close.
/// football 8 (one TD+2), basketball 10 (~3 possessions... see per-league)
fn one_score(league: League) -> u32 {
    match league {
        League::Nfl | League::Cfb => 8,
        League::Nba | League::Wnba | League::Cbb => 10,
        League::Nhl | League::Epl | League::Mls => 1,
        League::Mlb => 3, // a 3-run homer ties it — the save-situation scale
    }
}

pub fn closeness(league: League, margin: u32) -> u32 {
    let scale = one_score(league);
    if margin == 0 { return 100; }
    // 100 at margin 0, ~60 at one score, → 0 by four scores. Linear enough.
    100u32.saturating_sub(margin.saturating_mul(40) / scale.max(1))
}

/// Elapsed fraction of the game, 0..=100, from the label grammar the mapper
/// emits. Returns 0 for anything it cannot parse (never tier-1 on bad data).
pub fn lateness(league: League, period: &str, clock: &str) -> u32 {
    match league {
        League::Nfl | League::Cfb | League::Nba | League::Wnba => {
            let q = match period { "Q1" => 1, "Q2" => 2, "Q3" => 3, "Q4" => 4, "OT" => 5, _ => return 0 };
            let (qlen, played) = (quarter_len(league), quarter_len(league) - clock_secs(clock).min(quarter_len(league)));
            (((q - 1) as u32 * qlen + played) * 100 / (4 * qlen)).min(100)
        }
        League::Cbb => {
            let h = match period { "1ST HALF" => 1, "2ND HALF" => 2, "OT" => 3, _ => return 0 };
            let hl = 20 * 60;
            (((h - 1) as u32 * hl + (hl - clock_secs(clock).min(hl))) * 100 / (2 * hl)).min(100)
        }
        League::Nhl => {
            let p = match period { "1ST" => 1, "2ND" => 2, "3RD" => 3, "OT" | "SO" => 4, _ => return 0 };
            let pl = 20 * 60;
            (((p - 1) as u32 * pl + (pl - clock_secs(clock).min(pl))) * 100 / (3 * pl)).min(100)
        }
        League::Mlb => {
            // "TOP 2ND" / "BOT 9TH" / "MID 5TH" / "END 8TH"
            let mut it = period.split_whitespace();
            let (half, num) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            let inning: u32 = num.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse().ok().unwrap_or(0);
            if inning == 0 { return 0; }
            let half_add = match half { "TOP" => 0, "MID" | "BOT" => 1, "END" => 2, _ => return 0 };
            (((inning - 1) * 2 + half_add) * 100 / 18).min(100)
        }
        League::Epl | League::Mls => {
            // "63'" / "90'+3'" — stoppage pins to 95+.
            let base: u32 = period.split('\'').next().unwrap_or("").parse().ok().unwrap_or(0);
            if base == 0 { return 0; }
            if period.contains('+') || base >= 90 { return 95 + (base - 90).min(5); }
            base.min(94) * 100 / 94
        }
    }
}

fn quarter_len(league: League) -> u32 {
    match league { League::Nba => 12 * 60, League::Wnba => 10 * 60, _ => 15 * 60 }
}

/// "1:52" → 112; "" or junk → 0 (already-elapsed reads as late, which the
/// period match above guards by returning 0 first when the period is junk).
fn clock_secs(clock: &str) -> u32 {
    let mut it = clock.split(':');
    match (it.next().and_then(|m| m.parse::<u32>().ok()), it.next().and_then(|s| s.parse::<u32>().ok())) {
        (Some(m), Some(s)) => m * 60 + s,
        _ => 0,
    }
}

pub fn watchability(g: &Game, _now: OffsetDateTime) -> Watch {
    if g.status != Status::Live {
        return Watch { score: 0, hot: false, chip: None };
    }
    let margin = g.home_score.abs_diff(g.away_score) as u32;
    let l = lateness(g.league, &g.period, &g.clock);
    let c = closeness(g.league, margin);
    let mut score = l * c / 100;
    let mut hot = false;
    let mut chip = None;
    let mut bonus = |b: u32, ch: Option<&'static str>, hot_flag: &mut bool, chip_slot: &mut Option<&'static str>| {
        score += b; *hot_flag = true;
        if chip_slot.is_none() { *chip_slot = ch; }
    };
    match g.league {
        League::Nfl | League::Cfb => {
            if matches!(g.meter, Some(Meter::RedZone { .. })) { bonus(40, Some("RED ZONE"), &mut hot, &mut chip); }
            if matches!(g.period.as_str(), "Q2" | "Q4") && clock_secs(&g.clock) > 0 && clock_secs(&g.clock) <= 120 {
                bonus(30, Some("2-MIN"), &mut hot, &mut chip);
            }
        }
        League::Mlb => {
            let bases = g.situation.as_ref().and_then(|s| s.on_base).unwrap_or([false; 3]);
            if bases == [true, true, true] { bonus(30, Some("BASES LOADED"), &mut hot, &mut chip); }
            // Tying/go-ahead run on base, 8th or later: batting team within
            // (runners+1) of the lead. Half tells who bats: TOP=away, BOT=home.
            let inning_late = lateness(League::Mlb, &g.period, "") >= 77; // 8th+
            let runners = bases.iter().filter(|b| **b).count() as u16;
            let (bat, field) = if g.period.starts_with("TOP") { (g.away_score, g.home_score) } else { (g.home_score, g.away_score) };
            if inning_late && field > bat && field - bat <= runners + 1 && runners > 0 {
                bonus(40, Some("TYING RUN ON"), &mut hot, &mut chip);
            }
        }
        League::Nba | League::Wnba | League::Cbb => {
            let last2 = matches!(g.period.as_str(), "Q4" | "2ND HALF" | "OT") && clock_secs(&g.clock) > 0 && clock_secs(&g.clock) <= 120;
            if last2 && margin <= one_score(g.league) { bonus(40, Some("CLUTCH"), &mut hot, &mut chip); }
        }
        League::Nhl => {
            if matches!(g.meter, Some(Meter::Penalty { .. })) { bonus(25, Some("POWER PLAY"), &mut hot, &mut chip); }
        }
        League::Epl | League::Mls => {
            if (g.period.contains('+') || lateness(g.league, &g.period, "") >= 95) && margin <= 1 {
                bonus(30, Some("STOPPAGE"), &mut hot, &mut chip);
            }
        }
    }
    Watch { score, hot, chip }
}
```

(Receipts to include as comments where numbers appear: bonus weights are the spec §2 table; 77 = 8th inning start on the 18-half scale; 120 s = the two-minute warning; scales per sport named in `one_score`.)

- [ ] **Step 4: Run tests**

Run: `cargo test --lib rank 2>&1 | tail -4`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/rank.rs src/lib.rs
git commit -m "feat(v3.2): rank — watchability from each sport's own clock grammar, hot flags and state chips"
```

---

### Task 2: `OrderState` — event-gated ordering, nudges, sort keys (spec §2)

**Files:**
- Modify: `src/rank.rs` (add `SortKey`, `OrderState`), `src/app/mod.rs` (field + wiring in `apply_boards`), `src/config.rs` (`sort` key)
- Test: `src/rank.rs`, `tests/config.rs`

**Interfaces:**
- Produces (exact):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey { #[default] Watch, Time, League }   // + impl Default
impl SortKey { pub fn label(self) -> &'static str /* "WATCH"|"TIME"|"LEAGUE" */; pub fn cycled(self) -> SortKey; }

/// Owns the display order of live games. Reorders ONLY in `on_event`; between
/// events the order is frozen even as lateness rises (spec §2).
#[derive(Default)]
pub struct OrderState { /* order: Vec<String>, nudges: HashMap<String,(usize,u64)> */ }
impl OrderState {
    /// Recompute after a data event. `games` = live games (pins excluded by the
    /// caller); returns nothing — read via `ordered`/`nudge`.
    pub fn on_event(&mut self, games: &[Game], key: SortKey, now: OffsetDateTime, tick: u64);
    /// Current order as ids; games not yet seen sort by the key at the end.
    pub fn ordered<'a>(&self, games: &'a [Game]) -> Vec<&'a Game>;
    /// `Some(n)` while game `id` shows an `↑n` nudge (10 s per spec §1).
    pub fn nudge(&self, id: &str, tick: u64) -> Option<usize>;
}
/// Render ticks a nudge stays visible: 10 s at the live cadence (spec §1).
pub const NUDGE_TICKS: u64 = 10 * crate::app::LIVE_TICKS_PER_SEC;
```

- `Config` gains `#[serde(default)] pub sort: SortKey` (serialized lowercase); `App` gains `pub order: OrderState` and calls `self.order.on_event(...)` at the end of `apply_boards` (fresh applies only — inside the existing `!stale` guard) and in `merge_summary` when `scoring_plays` changed.

- [ ] **Step 1: Write the failing tests** — in `src/rank.rs`:

```rust
    #[test]
    fn order_is_frozen_between_events_and_nudges_mark_risers() {
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q2", "8:00", 14, 10); a.id = "a".into();
        let mut b = g(League::Nfl, "Q4", "1:52", 24, 21); b.id = "b".into();
        let mut c = g(League::Mlb, "TOP 3RD", "", 1, 0); c.id = "c".into();
        os.on_event(&[a.clone(), b.clone(), c.clone()], SortKey::Watch, now(), 0);
        let ids: Vec<&str> = os.ordered(&[a.clone(), b.clone(), c.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids[0], "b", "late close game leads");
        // No event: calling ordered again (later clock would rank differently) keeps order.
        let mut a2 = a.clone(); a2.period = "Q4".into(); a2.clock = "0:30".into();
        let ids2: Vec<&str> = os.ordered(&[a2.clone(), b.clone(), c.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ids2, "no event, no reorder");
        // Event: a's score changes; it rises and carries a nudge.
        let mut a3 = a2.clone(); a3.away_score = 24; a3.home_score = 24;
        os.on_event(&[a3.clone(), b.clone(), c.clone()], SortKey::Watch, now(), 100);
        let ids3: Vec<&str> = os.ordered(&[a3.clone(), b.clone(), c.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids3[0], "a", "tied in the last minute now leads");
        assert_eq!(os.nudge("a", 100), Some(1), "rose one place");
        assert_eq!(os.nudge("a", 100 + NUDGE_TICKS), None, "nudge expires");
        assert_eq!(os.nudge("b", 100), None, "the faller gets nothing");
    }

    #[test]
    fn sort_keys_time_and_league_are_stable_alternatives() {
        let mut os = OrderState::default();
        let mut a = g(League::Mlb, "TOP 1ST", "", 0, 0); a.id = "a".into();
        a.start = Some(time::macros::datetime!(2026-09-13 13:05 -4));
        let mut b = g(League::Nfl, "Q1", "15:00", 0, 0); b.id = "b".into();
        b.start = Some(time::macros::datetime!(2026-09-13 13:00 -4));
        os.on_event(&[a.clone(), b.clone()], SortKey::Time, now(), 0);
        let ids: Vec<&str> = os.ordered(&[a.clone(), b.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"], "earlier start first");
        os.on_event(&[a.clone(), b.clone()], SortKey::League, now(), 0);
        let ids: Vec<&str> = os.ordered(&[a.clone(), b.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"], "NFL precedes MLB in League::ALL order");
    }

    #[test]
    fn a_new_game_joins_without_scrambling_the_rest() {
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q4", "1:00", 20, 17); a.id = "a".into();
        os.on_event(&[a.clone()], SortKey::Watch, now(), 0);
        let mut b = g(League::Nfl, "Q1", "15:00", 0, 0); b.id = "b".into();
        // b entering IS an event.
        os.on_event(&[a.clone(), b.clone()], SortKey::Watch, now(), 10);
        let ids: Vec<&str> = os.ordered(&[a.clone(), b.clone()]).iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(os.nudge("b", 10), None, "entering is not rising");
    }
```

And in `tests/config.rs`:

```rust
#[test]
fn sort_key_round_trips_and_old_layout_keys_are_ignored() {
    let dir = tmp("sortkey");
    fs::write(dir.join("config.toml"),
        "enabled_tabs = [\"Nfl\"]\nfavorites = []\nsort = \"time\"\nlayout = \"Auto\"\nscore_style = \"big\"\n").unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.sort, gameday::rank::SortKey::Time);
    // layout/score_style are gone from the struct; unknown keys parse fine.
    c.save_to(&dir).unwrap();
    let text = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(text.contains("sort = \"time\""));
    assert!(!text.contains("layout"), "removed key is not re-written");
    fs::remove_dir_all(&dir).ok();
}
```

(NOTE: this config test compiles only after Task 10 removes `layout`/`score_style` from `Config`. Write it here with `#[ignore = "fields removed in the keys task"]` and un-ignore it in Task 10 — the ignore string is the contract.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib rank::tests::order_is_frozen 2>&1 | tail -3`
Expected: compile error (`OrderState` missing).

- [ ] **Step 3: Implement.** `OrderState { order: Vec<String>, nudges: HashMap<String, (usize, u64)> }`. `on_event`: compute the target order (sort by key: Watch → `watchability().score` desc then id; Time → `start` asc then id; League → `League::ALL` position then start); diff each id's new index against its old index (ids absent before are "entering" — no nudge); store `nudges[id] = (old - new, tick)` only when `old > new` (risers); replace `order`. `ordered`: map stored ids to `games` (skip ids no longer present), then append games not in `order` sorted by the key (belt-and-braces; `on_event` should have seen them). `nudge(id, tick)`: `self.nudges.get(id).filter(|(_, t0)| tick.saturating_sub(*t0) < NUDGE_TICKS).map(|(n, _)| *n)`. Wire `App`: field, `apply_boards` (inside `!stale`, after boards insert: `self.order.on_event(&self.live_all(), self.config.sort, self.now(), self.tick)` where `live_all()` = every live game on enabled boards minus pinned ids), `merge_summary` likewise when it changed `scoring_plays`. `Config.sort` with `#[serde(default)]`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib rank 2>&1 | tail -4 && cargo test --test config 2>&1 | tail -3`
Expected: rank tests PASS; the config test is ignored (accepted until Task 10).

- [ ] **Step 5: Commit**

```bash
git add src/rank.rs src/app/mod.rs src/config.rs tests/config.rs
git commit -m "feat(v3.2): OrderState — event-gated ordering with expiring nudges; sort key in config"
```

---

### Task 3: Theme roles (spec §6, theme decision A)

**Files:**
- Modify: `src/theme.rs`, `assets/themes/{broadcast,studio,gruvbox}.toml` (add `[roles]`), `src/views/theme_picker.rs` (only if the names list shrinks breaks a test), `tests/theme.rs`
- Test: `src/theme.rs`, `tests/theme.rs`

**Interfaces:**
- Produces (exact):

```rust
/// Where team color is allowed (spec §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamColorScope { #[default] Hero, HeroMarks, Never }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Roles {
    pub ground: Color, pub ink: Color, pub dim: Color,
    pub digits: Color,   // amber — every score outside the hero
    pub hot: Color,      // mark, state chips, cut, scoring word
    pub cool: Color,     // rules, legend keys, structure
    pub team: TeamColorScope,
}
impl Theme { pub fn roles(&self) -> Roles; }
/// Hue-separation rule (spec §6): when both lifted colors are within 20° of
/// hue AND 15% of luminance, the HOME side falls back to `digits` amber.
pub fn hero_pair(th: &Theme, away: [u8; 3], home: [u8; 3]) -> (Color, Color, bool /* home_fell_back */);
```

- `BUILTIN_NAMES` shrinks to `["broadcast", "studio", "gruvbox"]` (decision A); the other eight TOML files stay in `assets/themes/` (loadable as user files) but are no longer `include_str!`'d. `[roles]` is an optional TOML table mapping role → palette key name (`ground = "bg"` etc. per the spec's block); absent → the documented defaults (`ground=bg, ink=fg, dim=muted, digits=star, hot=live, cool=border, team=hero`). `[discipline]` keys still parse and are ignored with ONE `crate::log::note` line naming the file (spec §6 compat).

- [ ] **Step 1: Write the failing tests** — `src/theme.rs`:

```rust
    #[test]
    fn three_builtins_and_every_one_defines_every_role() {
        assert_eq!(BUILTIN_NAMES, ["broadcast", "studio", "gruvbox"]);
        for name in BUILTIN_NAMES {
            let th = builtin(name);
            let r = th.roles();
            for (label, c) in [("ground", r.ground), ("ink", r.ink), ("dim", r.dim), ("digits", r.digits), ("hot", r.hot), ("cool", r.cool)] {
                assert!(matches!(c, Color::Rgb(..)), "{name}: role {label} must be a concrete color");
            }
        }
    }

    #[test]
    fn discipline_table_still_parses_but_is_ignored() {
        let toml = "name = \"legacy\"\n[palette]\nbg=\"#000000\"\nfg=\"#e0e0e0\"\nbright=\"#ffffff\"\nmuted=\"#888888\"\ndim=\"#444444\"\nborder=\"#333333\"\nlive=\"#ff3333\"\ngreen=\"#33cc66\"\ncyan=\"#33cccc\"\nmagenta=\"#cc66cc\"\nstar=\"#ffbf00\"\n[discipline]\nchips = true\n";
        let (name, _th) = parse_theme(toml).expect("legacy file loads");
        assert_eq!(name, "legacy");
    }

    #[test]
    fn hero_pair_separates_lookalike_colors() {
        let th = builtin("broadcast");
        // SEA navy vs BOS navy-ish: same hue family → home falls back to amber.
        let (a, h, fell) = hero_pair(&th, [12, 44, 86], [19, 41, 75]);
        assert!(fell, "lookalikes must separate");
        assert_eq!(h, th.roles().digits);
        assert!(matches!(a, Color::Rgb(..)));
        // KC red vs BUF blue: both keep their color.
        let (_, _, fell2) = hero_pair(&th, [227, 24, 55], [0, 51, 141]);
        assert!(!fell2);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib theme 2>&1 | tail -3`
Expected: compile errors (`Roles`, `hero_pair` missing; BUILTIN_NAMES length).

- [ ] **Step 3: Implement.** `Roles`/`TeamColorScope`; `Theme` stores `roles: Roles` built at parse (role table maps names to palette fields via a match on the string — error names the key and the valid set); `roles()` accessor. Shrink `BUILTIN_NAMES`/`BUILTIN_TOML` to 3 and add a `[roles]` block to those three TOMLs (broadcast: defaults; studio: same with `team = "hero"`; gruvbox: `digits = "star"`, others default). `[discipline]` becomes `#[serde(default)] Option<toml::Value>` on `ThemeFile` — when `Some`, `crate::log::note_once(&format!("theme:{name}:discipline"), …)`. `hero_pair`: lift both through `art_color`, convert to HSL (write a tiny `fn hsl(c: [u8;3]) -> (f32, f32, f32)` — hue in degrees, lightness 0..1, with a receipt comment), compare `(hue_delta < 20.0 && l_delta < 0.15)`; on collision return `(lifted_away, roles.digits, true)`. Existing `Discipline`-driven helpers (`chip`, `section_label`, `team_text`, `clock`, `sidebar_header`) survive this task untouched (the board tasks stop calling them; Task 15 deletes the dead ones). Fix `tests/theme.rs` assertions that enumerate 11 built-ins (shrink to 3; keep the per-theme render assertions for the three).

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -4`
Expected: PASS (theme picker/config tests that referenced dropped names updated honestly — e.g. `theme phosphor` completion tests now use `gruvbox`).

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs assets/themes tests/theme.rs src/views/theme_picker.rs src/input.rs
git commit -m "feat(v3.2): themes are roles — three built-ins, [roles] table, discipline compat-read, hero hue separation"
```

---

### Task 4: Logos — regenerate at 16×10, slim the module (spec logo decision)

**Files:**
- Modify: `tools/gen-logos.sh` (no logic change needed — `SIZE` env exists), `assets/logos/**` (regenerated), `src/tiles/logo.rs` → Create `src/board/logo.rs` (move + slim; `src/board/mod.rs` stub with `pub mod logo;` so it compiles before Task 8), `src/lib.rs` (`pub mod board;`)
- Test: `tests/logo.rs` (rewrite)

**Interfaces:**
- Produces (exact):

```rust
// src/board/logo.rs
/// A parsed 16×10 hero mark. Cells are (char, fg, bg) like the old AnsiArt.
pub struct HeroMark { /* rows: Vec<Vec<ArtCell>>, pub width: u16, pub height: u16 */ }
/// The committed art for `logo_key` ("nfl/kc"), parsed once (OnceLock map).
pub fn hero_mark(logo_key: &str) -> Option<&'static HeroMark>;
/// Blit into `area` (clipped), colors routed through `theme::art_color`.
pub fn draw_hero_mark(frame: &mut Frame, area: Rect, mark: &HeroMark);
```

- The OLD tile-draw API (`load_logo`, `draw_logo`, `draw_abbr_mark`) survives until Task 8 deletes its callers — this task moves the module and re-exports the old names from the new location (`pub use` shims in `src/tiles/logo.rs` replaced by `pub use crate::board::logo::*;`) so nothing breaks mid-sequence.

- [ ] **Step 1: Regenerate the art** (network allowed, one run): `SIZE=16x10 tools/gen-logos.sh` — it overwrites `assets/logos/**` in place, which is the intent this time. Verify: `head -1 assets/logos/nfl/kc.ans` shows sextants; `wc -l assets/logos/nfl/*.ans` rows are 8–10; `du -sh assets/logos` well under 200 KB. If `magick` is absent the brighten step self-skips (script already guards) — note it in the report.

- [ ] **Step 2: Write the failing tests** — `tests/logo.rs` (replace the file):

```rust
use gameday::board::logo::{hero_mark, HeroMark};

#[test]
fn every_committed_mark_parses_at_hero_size() {
    // The build embeds the same set logo.rs lists; spot-check the demo set.
    for key in ["nfl/kc", "nfl/buf", "nfl/dal", "mlb/nyy", "nba/bos", "nhl/edm"] {
        let m: &HeroMark = hero_mark(key).unwrap_or_else(|| panic!("{key} missing"));
        assert!(m.height >= 6 && m.height <= 10, "{key}: 16x10 regeneration, got {}", m.height);
        assert!(m.width >= 10 && m.width <= 16, "{key}: width {}", m.width);
    }
    assert!(hero_mark("mlb/sea").is_none(), "missing art is None, the caller falls back");
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test logo 2>&1 | tail -3`
Expected: compile error (`gameday::board::logo` missing).

- [ ] **Step 4: Implement.** Create `src/board/mod.rs` containing only `pub mod logo;` for now. Move the parser from `src/tiles/logo.rs` (the `parse_ansi_art` SGR reader and the `OnceLock<HashMap>` from v3.1) into `src/board/logo.rs`, renaming the parsed type `HeroMark`; keep `LOGO_SOURCES` as the single embed list. `hero_mark` = the memoized lookup; `draw_hero_mark` = the existing blit routed through `theme::art_color`, clipped to `area`. `src/tiles/logo.rs` becomes `pub use crate::board::logo::{...}` shims for the old names, with the old `load_logo`/`draw_logo` reimplemented as thin wrappers over `hero_mark`/`draw_hero_mark` (the tile grammar still calls them until Task 8). Update `tests/logo.rs` as above; the old logo tests that asserted 10×6 sizes are replaced by the new size band.

- [ ] **Step 5: Run tests + look**

Run: `cargo test 2>&1 | tail -4 && cargo run --release -- dump >/dev/null && open out/board-broadcast.html`
Expected: PASS; the CURRENT tile board still renders (bigger marks in the old slots will overflow their 8×5 boxes — the tile draw clips to its rect, so expect cropped marks in the gallery; that is acceptable for this intermediate commit and Task 8 removes the tile grammar. Say so in the commit body.)

- [ ] **Step 6: Commit**

```bash
git add tools/gen-logos.sh assets/logos src/board src/tiles/logo.rs src/lib.rs tests/logo.rs
git commit -m "feat(v3.2): logos regenerated at 16x10; board::logo::hero_mark with tile-era shims (tiles crop until the board lands)"
```

---

### Task 5: `board/layout.rs` — the tier budget (spec §1, §4)

**Files:**
- Create: `src/board/layout.rs` (`pub mod layout;` in `src/board/mod.rs`)
- Test: `src/board/layout.rs`

**Interfaces:**
- Produces (exact):

```rust
/// How many rows each piece of the board gets at a given size. Pure: height
/// and counts in, plan out — the sizes ladder (spec §4) lives here and only here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierPlan {
    pub hero_rows: u16,        // 0 = no hero fits (only < 12 total rows)
    pub hero_digits_full: bool,// 8-row PixelSize::Full vs 4x3 sextant
    pub tier1: usize,          // promoted 3-row rows (0..=3)
    pub tier2: usize,          // single-line live rows
    pub finals: usize,         // dim single lines (0 = collapse to count line)
    pub later: usize,
    pub scores_lane: bool,     // the off-screen SCORES lane (only when truncated)
}
pub fn plan(width: u16, height: u16, live: usize, finals: usize, later: usize, my_games: usize) -> TierPlan;
/// Row cost of one section rule/label line.
pub const RULE_ROWS: u16 = 1;
```

Budget rules (the spec §4 table, encoded): hero = 10 rows at ≥100 cols & ≥32 body rows (8-row Full digits + fragments + meter), 6 rows with sextant digits at 80–99 cols or 24–31 rows, 2-row compact hero under 60 cols, capped 3 digit rows at ≤24 total rows; tier1 up to 3 at 120×40 (scaled: `(body_rows - hero - rules) / 6` capped 3, 2 at 100–119 cols, 0 below 100 cols); rows run out bottom-up: later → count line first, then finals, then tier2 truncates (lane on), tier1 demotes, hero shrinks last.

- [ ] **Step 1: Write the failing tests**:

```rust
    #[test]
    fn the_sizes_ladder_matches_the_spec_table() {
        // 120x40: full hero, up to 3 promoted, no lane.
        let p = plan(120, 38, 8, 2, 4, 0);
        assert_eq!(p.hero_rows, 10); assert!(p.hero_digits_full);
        assert_eq!(p.tier1, 3); assert!(!p.scores_lane);
        // 80x24: sextant hero, all tier 2, lane when truncated.
        let p = plan(80, 22, 8, 2, 4, 0);
        assert!(p.hero_rows <= 6 && p.hero_rows >= 4); assert!(!p.hero_digits_full);
        assert_eq!(p.tier1, 0);
        assert!(p.scores_lane, "8 live don't fit 22 rows — lane on");
        // <60 cols: 2-row compact hero, still one list.
        let p = plan(55, 38, 3, 1, 2, 0);
        assert_eq!(p.hero_rows, 2);
        // Never negative / overlapping: sum of allocated rows <= height.
        for (w, h) in [(40u16, 12u16), (60, 20), (100, 30), (200, 60)] {
            let p = plan(w, h, 12, 5, 8, 2);
            let used = p.hero_rows + 3 * p.tier1 as u16 + p.tier2 as u16
                + p.finals as u16 + p.later as u16 + 4 * RULE_ROWS
                + if p.scores_lane { 1 } else { 0 };
            assert!(used <= h, "{w}x{h}: used {used}");
        }
    }

    #[test]
    fn rows_run_out_bottom_up() {
        // Shrinking height drops LATER to a count line before touching live rows.
        let tall = plan(120, 38, 6, 3, 6, 0);
        let short = plan(120, 26, 6, 3, 6, 0);
        assert!(short.later < tall.later, "later shrinks first");
        assert!(short.tier2 + short.tier1 >= 6usize.min(tall.tier1 + tall.tier2), "live rows survive");
        // Down further: finals go, then the lane appears.
        let tiny = plan(120, 16, 6, 3, 6, 0);
        assert_eq!(tiny.finals, 0);
        assert!(tiny.scores_lane);
    }

    #[test]
    fn my_games_rows_come_off_the_same_budget() {
        let without = plan(120, 30, 6, 2, 3, 0);
        let with = plan(120, 30, 6, 2, 3, 2);
        assert!(with.tier2 <= without.tier2, "band rows are not free");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib board::layout 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement** with the priority order as a single top-down subtraction (hero first by the width/height gates, then a rule+row budget walked in the order: MY GAMES rows (1 line each; the hero may be one of them — the caller decides identity, layout only counts), tier1 from what's left `/4` (3 rows + breathing) capped by width class, tier2 fill, finals min(2, left), later min(left, later), collapse flags). Keep it under ~80 lines; every gate carries the spec §4 row as its receipt comment.

- [ ] **Step 4: Run tests** — `cargo test --lib board::layout` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/board/layout.rs src/board/mod.rs
git commit -m "feat(v3.2): board layout — the sizes ladder as one pure tier budget"
```

---

### Task 6: `board/hero.rs` — the hero block (spec §1 Hero, logo decision)

**Files:**
- Create: `src/board/hero.rs`
- Modify: `src/tiles/mod.rs` (make `render_digits`-equivalent reusable: `pub(crate) fn digit_block(...)` extraction), `src/board/mod.rs` (`pub mod hero;`)
- Test: `tests/draw.rs` additions run in Task 8 when the view exists; this task tests via a direct `TestBackend` harness in `src/board/hero.rs`

**Interfaces:**
- Consumes: `theme::hero_pair`, `rank::Watch` (chip), `board::logo::{hero_mark, draw_hero_mark}`, the meter renderer (`tiles::meter_line` made `pub(crate)`), `text::fmt_start`.
- Produces (exact):

```rust
/// Draw the hero for `game` into `area` per the plan. `now` for pre-game
/// starts; `chip` from rank; `pinned`/`favorite` for the nameplate glyphs.
pub fn draw_hero(frame: &mut Frame, area: Rect, game: &Game, plan: &HeroPlan);
#[derive(Clone, Debug)]
pub struct HeroPlan {
    pub digits_full: bool, pub chip: Option<&'static str>,
    pub now: time::OffsetDateTime, pub pinned: bool, pub favorite: bool,
    pub show_logos: bool,   // false under 100 cols — the flanks are the first casualty
}
/// The score digits alone (mirror pair, team colors via hero_pair), reused
/// verbatim by the cut overlay and TV — the "one formatter" hard rule.
pub fn score_block(frame: &mut Frame, area: Rect, game: &Game, full: bool);
/// The fragment line under the digits: football "2ND & GOAL · BALL ON 4 · KC BALL";
/// MLB returns None — its diamond meter row IS the fragment line (A′ call #2).
pub fn fragment_line(game: &Game) -> Option<ratatui::text::Line<'static>>;
```

Layout inside `area` (the A′ hero, `docs/research/v3-identity/nfl-sunday-120x40.png` rows 2–13): row 0 nameplates (away left `KC 2-0` team-colored abbr + dim record + `⚑`/`★`; home right mirrored); rows 1..n digits via `score_block` (away digits left third, home right third, both through `theme::hero_pair`; center column carries `Q4 1:52` (`ink`) and the chip (`hot` bg badge)); under the digits the fragment line (or nothing for MLB); then the meter row (full width, `tiles::meter_line` with its track stretched to `METER_MAX_BAR`); then `▸ <last play>` (`ink`). When `show_logos` and both flanks are ≥18 cols of true margin, `draw_hero_mark` centers each team's mark in its outer margin — a missing mark leaves the margin empty (the color-block identity already lives in the nameplate; never draw a placeholder). Logos never move a digit: compute digit rects first, flanks from what remains.

- [ ] **Step 1: Write the failing tests** — `src/board/hero.rs` `#[cfg(test)]` (TestBackend, reuse the KC/BUF game shape from `tests/draw.rs`'s helper inline):

```rust
    #[test]
    fn hero_digits_take_team_colors_and_lookalikes_separate() {
        // KC red vs BUF blue → both colored; find a red cell on the left
        // half and a blue cell on the right half of the digit band.
        // (assert on buffer cell fg values via term.backend().buffer())
    }
    #[test]
    fn the_chip_is_the_only_filled_badge_and_sits_center() { /* RED ZONE bg == roles.hot */ }
    #[test]
    fn mlb_hero_has_no_duplicate_fragment_line() {
        // fragment_line(mlb_game) is None; the meter row renders BASES/OUTS once.
    }
    #[test]
    fn logos_flank_when_art_exists_and_leave_clean_margin_when_not() {
        // nfl/kc exists → non-ground cells in the left flank; a fake team → flank stays ground.
    }
    #[test]
    fn under_100_cols_logos_drop_before_digits_shrink() { /* show_logos=false path */ }
```

Write these as real assertions against buffer cells (the v3.1 suite shows the pattern — `tests/theme.rs` asserts exact cell fg). Each test builds its own 120×12 or 80×8 `TestBackend`.

- [ ] **Step 2: Run to verify failure** — `cargo test --lib board::hero` → compile error.

- [ ] **Step 3: Implement.** Extract the digit renderer from `src/tiles/mod.rs::render_digits` into a shared `pub(crate) fn digit_glyphs(frame, rect, value: u16, color: Color, full: bool) -> bool` (same clamping discipline — the v3.1 review verified those bounds; keep them) and build `score_block` on it: away digits right-aligned in the left third, home left-aligned in the right third, colors from `hero_pair`; when `full` fails to fit, fall through to sextant, then to a bold `24 - 21` text line (never blank). `fragment_line`: football `situation.down_distance · BALL ON x · POSS BALL`; basketball/hockey nothing extra (clock chip suffices) → None; MLB None. `draw_hero` composes per the layout above.

- [ ] **Step 4: Run tests** — `cargo test --lib board::hero` → PASS; full suite still green (tile grammar untouched).

- [ ] **Step 5: Commit**

```bash
git add src/board/hero.rs src/board/mod.rs src/tiles/mod.rs
git commit -m "feat(v3.2): hero block — mirror team-color digits, one fragment line, meter, logo flanks that never move a digit"
```

---

### Task 7: `board/rows.rs` — tiers 1/2/3 (spec §1)

**Files:**
- Create: `src/board/rows.rs` (`pub mod rows;`)
- Test: `src/board/rows.rs`

**Interfaces:**
- Consumes: `rank::{Watch, OrderState}` values via parameters (no App access), `tiles::digit_glyphs` (sextant), theme roles.
- Produces (exact):

```rust
pub struct RowCtx { pub hot: bool, pub nudge: Option<usize>, pub selected: bool,
                    pub pinned: bool, pub league_tag: bool, pub now: time::OffsetDateTime }
/// Tier 1: a 3-row promoted row (sextant digits) — abbr+league stack, digits,
/// clock+state column, fragments, last play (A′ tier-1 rows).
pub fn draw_tier1(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx);
/// Tier 2: one line — mark │ nudge │ ABBR n ABBR n │ clock │ [league] │ fragment.
pub fn draw_tier2(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx);
/// Tier 3: dim final (`· ARS 3 BHA 0 FT EPL headline`) or later
/// (`· TB @ ATL 4:25 PM NFL FOX TB -1.5 O/U 47.5`).
pub fn draw_tier3(frame: &mut Frame, area: Rect, game: &Game, ctx: &RowCtx);
/// The two fixed gutters every tier-1/2 row reserves: 2-cell hot mark, 2-cell nudge.
pub const GUTTER: u16 = 4;
```

Rules: the mark is `▌` in `hot` when `ctx.hot`, `dim` otherwise (two ink states — spec Decisions); the nudge gutter always reserved, `↑n` in `digits` while `ctx.nudge` is Some (A′ call #6/#7: rank and hotness are different facts — the mark never changes because of a nudge); scores in `digits` amber bold; a PINNED team's abbr in its team color (`TeamColorScope::HeroMarks`+ only — read `roles().team`), everything else `ink`; `league_tag` printed only when `ctx.league_tag` (mixed list — A′ call #5); selection = `▸` in the nudge gutter + `bright` text; a final's headline = its newest `scoring_plays` text, else the situation summary, else empty.

- [ ] **Step 1: Write the failing tests** (TestBackend per test; assert cells):

```rust
    #[test] fn tier2_line_layout_and_amber_scores() { /* mark col 0, scores fg == roles.digits, league tag present only when ctx.league_tag */ }
    #[test] fn hot_mark_has_two_states_only() { /* hot => roles.hot; !hot => roles.dim; nudge does NOT change it */ }
    #[test] fn nudge_gutter_never_shifts_the_row() { /* same x for abbr with and without a nudge */ }
    #[test] fn pinned_abbr_wears_team_color_others_ink() { }
    #[test] fn tier3_later_shows_local_time_never_iso() { /* fmt_start via ctx.now; "4:25 PM" present, "2026-" absent */ }
    #[test] fn tier1_is_three_rows_with_sextant_digits() { /* digit cells above baseline, fragment on row 2 */ }
```

- [ ] **Step 2: Run to verify failure** — compile error.
- [ ] **Step 3: Implement** per the A′ frames (open `docs/research/v3-identity/nfl-sunday-120x40.png` while writing — the IN PLAY rows at y≈250–520 are the contract).
- [ ] **Step 4: Run tests** — PASS.
- [ ] **Step 5: Commit**

```bash
git add src/board/rows.rs src/board/mod.rs
git commit -m "feat(v3.2): board rows — three tiers, two-state mark, reserved nudge gutter, amber discipline"
```

---

### Task 8: The board view — assembly, selection, deletions (spec §1, §7)

**Files:**
- Modify: `src/board/mod.rs` (the view), `src/views/mod.rs` (route `View::Board` to `board::draw`), `src/app/derive.rs` (`Derived` reshaped), `src/app/mod.rs` (selection/scroll handlers; delete paging), `src/app/chrome.rs` (footer legend interim), `src/views/board.rs` (DELETE), `src/tiles/packer.rs` + `tests/packer.rs` (DELETE), `src/tiles/mod.rs` (delete tile grammar: `render_tile`, borders, MOMENTUM, `Density`, `ScoreStyle`, slate/sidebar helpers — keep `digit_glyphs`, `meter_line`, `play_stamp`, the compact-row text helpers the tiers reuse), `src/dump.rs` + `src/demo.rs` (compile fixes only — gallery redesign is Task 15; keep stems building against the new view)
- Test: `tests/draw.rs` (board sections), existing suites updated

**Interfaces:**
- Consumes: everything from Tasks 1–7.
- Produces (exact):

```rust
// src/app/derive.rs — Derived reshaped:
pub struct Derived {
    pub my_games: Vec<Game>,     // pins then favorites, never re-sorted (spec §1)
    pub in_play: Vec<Game>,      // OrderState order, pins excluded
    pub finals: Vec<Game>, pub later: Vec<Game>,
    pub selection: Vec<Game>,    // my_games ++ in_play ++ finals ++ later
    pub hero_id: Option<String>, // top of MY GAMES if live, else in_play[0]
    pub mixed: bool,             // >1 league present → league tags on rows
    pub scoring: Vec<(Game, Play)>, pub ticker_live: Vec<Game>, pub ticker_events: Vec<(Game, Play)>,
}
// src/board/mod.rs
pub fn draw(app: &mut App, frame: &mut Frame, area: Rect);
```

Selection: `j/k` walk `selection` as one list with a scroll offset that keeps the selected row visible (the hero scrolls away like any row — spec §1); `enter`/`z` zoom the selection; `n`/`p` and pages are DELETED (`change_page`, `page`, `page_len_of`, `page_count_of`, the `GAME x/y`/`PAGE x/y` footer parts → `GAME x/y` only). Empty states: keep v3.1's `nothing live · next: …` and offline/filter branches verbatim (they moved files; the strings must not change — tests pin them). Section rules per A′: `MY GAMES ─── 2 PINNED · NEVER RE-SORTS`, `IN PLAY ─── SORTED BY WATCHABILITY` (right side names `SortKey::label()`), `FINAL ───`, `LATER ───`; league tabs' league view = the same board filtered to one league (Home vs league tab differ only in the game set — `visible_games` already handles it).

- [ ] **Step 1: Write the failing tests** — `tests/draw.rs`:

```rust
#[test]
fn the_board_is_one_ranked_list_with_sections() {
    // 6 live + 2 final + 2 later at 120x40: MY GAMES absent (no pins),
    // IN PLAY rule present with SORTED BY WATCHABILITY, hero digits row,
    // FINAL and LATER sections, NO borders (no '┌' anywhere), no MOMENTUM,
    // no SLATE, no GLOBAL ALERTS sidebar text.
}
#[test]
fn pinned_games_sit_in_a_band_that_never_resorts() {
    // Pin two games; band label "2 PINNED · NEVER RE-SORTS"; their order
    // is pin order even when watchability says otherwise.
}
#[test]
fn selection_walks_the_whole_list_and_scrolls() {
    // j past the visible rows moves the window (selected row text is bright
    // somewhere on screen after 20 presses at 120x24).
}
#[test]
fn the_ticker_is_gone_at_40_rows_and_the_lane_appears_when_truncated() {
    // 120x40 with 4 live: no SCORES lane. 80x24 with 10 live: one SCORES
    // lane above the footer listing off-screen games.
}
#[test]
fn paging_keys_are_dead_and_not_advertised() {
    // 'n' changes nothing; footer contains no "PAGE"; keymap has no PAGE binding.
}
```

Update the many existing draw/app tests that assert tiles/sidebar/slate/pages honestly: each changed assertion gets a one-line comment naming the spec section that retired the behavior. Delete `tests/packer.rs`, `tests/tile_snap.rs` (tile grammar gone — its panic-safety width sweep moves to Task 14's board sweep).

- [ ] **Step 2: Run to verify failure** — the five new tests fail/compile-error.
- [ ] **Step 3: Implement** — `derive()` builds the new `Derived` (my_games from `home_games`-style pin/fav filter; in_play via `app.order.ordered(...)`; hero per rule; `mixed`); `board::draw` walks `layout::plan` → sections; key handlers updated (`move_selected` clamps against `selection`, scroll offset on App); deletions per the Files list. Keep `derived_lists_agree_with_the_fns` alive by re-pointing it at the new composition (the fns it compares move into `derive.rs` as the single source now — fold the old list fns into `derive()` and DELETE the standalone methods; key handlers read `self.derive()` fresh (cheap enough off-frame) or the frame cache inside draw. This resolves v3.1's "two sources of truth" deferred minor — note it in the commit body.)
- [ ] **Step 4: Run everything + look**

Run: `cargo test 2>&1 | tail -4 && cargo run --release -- dump >/dev/null`
Then open `out/board-broadcast.png` (font env set) NEXT TO `docs/research/v3-identity/tonight-120x40.png` — the grammar must read as the same design (sections, hero, tiers; demo data differs). Expected: PASS; visual match.

- [ ] **Step 5: Commit**

```bash
git add -A src tests
git commit -m "feat(v3.2): the ranked board — one list, MY GAMES band, tier sections; tiles/sidebar/slate/paging deleted"
```

---

### Task 9: Chrome — header, footer, ticker gating (spec §1 Header/Footer/Ticker)

**Files:**
- Modify: `src/app/chrome.rs`, `src/ticker.rs` (lane variant), `src/app/mod.rs` (ticker height gate)
- Test: `tests/draw.rs`

**Interfaces:**
- Consumes: `Derived.mixed`, `SortKey::label()`, the Task 13-era shed ladder (keep it — only the content changes).
- Produces: header left = ` GAMEDAY ` + chips for enabled leagues **with games today** (dim, lit = active tab) — no `FILTER:` label ever (the ladder's rung 2 becomes the default; the ladder itself stays for narrower widths); header right = `s SORT: WATCH` (only on Board view) + NetStatus chip + date + clock, same never-clip guarantees as v3.1. Footer legend (Board): `↑↓ move  enter zoom  space pin  / filter  s sort  v tv  ? help  q quit` with the same shed order discipline (HELP and QUIT never shed). Ticker: `App::draw` allocates the ticker rows ONLY when `layout::plan(...).scores_lane` (the board hands the flag up via `Derived`/a draw-time value) — one `SCORES` lane listing games not on screen (`2 OFF-SCREEN · 2 FINAL · 4 LATER` when nothing live is off), reusing `ticker::whole_segments`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn header_shows_sort_key_and_only_leagues_with_games() { /* "SORT: WATCH"; a league with an empty board has no chip; clock survives (reuse the 40..180 sweep bounds) */ }
#[test] fn footer_advertises_sort_and_tv_not_pages() { /* "s sort" and "v tv" present; "PAGE"/"PIN [SPC]" old caps style gone per A′ lowercase legend */ }
#[test] fn scores_lane_lists_off_screen_games_only() { /* game on screen absent from lane */ }
```

- [ ] **Step 2: Run to verify failure** — fails.
- [ ] **Step 3: Implement** (footer legend becomes lowercase per the A′ frames — update `keymap.rs` labels or map at render; keep `FOOTER_DROP_ORDER` semantics).
- [ ] **Step 4: Run tests + eyeball `out/board-broadcast.png` header/footer vs the A′ frame.**
- [ ] **Step 5: Commit**

```bash
git add src/app/chrome.rs src/ticker.rs src/app/mod.rs src/keymap.rs tests/draw.rs
git commit -m "feat(v3.2): chrome — sort key in the header, lowercase legend, ticker only as the off-screen lane"
```

---

### Task 10: Commands and keys — `:sort`, `:tv`, `v`, `s`; layout/score removed (spec §9, Global Constraints)

**Files:**
- Modify: `src/command.rs`, `src/input.rs`, `src/keymap.rs`, `src/app/mod.rs` (key handlers), `src/config.rs` (remove `layout`, `score_style`), `src/views/config_view.rs` (DISPLAY section: THEME + SORT rows; LAYOUT/SCORE rows deleted), `README.md` (one line: old `layout`/`score_style` config keys are ignored), `tests/config.rs` (un-ignore Task 2's test)
- Test: `src/command.rs`, `src/input.rs`, `src/keymap.rs`, `tests/config.rs`

**Interfaces:**
- Produces: `Cmd::Sort(Option<SortKey>)` (`:sort` bare cycles, `:sort watch|time|league` sets), `Cmd::Tv`; registry rows `("sort", ArgSpec::Sort)`, `("tv", ArgSpec::None)`; `ArgSpec::Sort` completes the three values; `Cmd::Layout`/`Cmd::Score` and their registry rows/ArgSpecs DELETED (unknown-command error text shrinks with them). Keys on the board: `s` = cycle sort (persists via `persist_config`), `v` = enter TV; `1/2/4` unbound (dead); keymap rows: SORT (`S`, footer Board), TV (`V`, footer Board), PAGE/LAYOUT rows deleted; help/coverage tests updated (the coverage test's `chord_for` special-cases for `1/2/4/s` are deleted — v3.1 deferred minor, resolved here since `s` changes meaning).

- [ ] **Step 1: Write the failing tests**:

```rust
// src/command.rs tests
#[test]
fn sort_and_tv_parse_and_layout_score_are_gone() {
    assert_eq!(parse("sort").unwrap(), Cmd::Sort(None));
    assert_eq!(parse("sort time").unwrap(), Cmd::Sort(Some(crate::rank::SortKey::Time)));
    assert!(parse("sort sideways").unwrap_err().contains("watch|time|league"));
    assert_eq!(parse("tv").unwrap(), Cmd::Tv);
    let err = parse("layout").unwrap_err();
    assert!(err.contains("unknown command"), "{err}");
    assert!(!valid_names().contains("score"), "registry cleaned");
}
// src/app tests
#[test]
fn s_cycles_the_sort_and_persists_and_the_header_follows() {
    let mut app = app_with(six_live(), vec![]);
    assert_eq!(app.config.sort, SortKey::Watch);
    app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
    assert_eq!(app.config.sort, SortKey::Time);
    assert!(app.status_line.as_deref().unwrap_or("").contains("sort time"));
}
#[test]
fn v_enters_tv_and_esc_leaves() {
    let mut app = app_with(six_live(), vec![]);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(matches!(app.view, View::Tv));
    app.on_key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.view, View::Board);
}
```

(`View::Tv` lands as an empty variant here — routes to a placeholder draw that Task 12 fills; the test asserts the mode transition only.)

- [ ] **Step 2: Run to verify failure** — compile errors.
- [ ] **Step 3: Implement**, including: `Config` loses `layout`/`score_style` (their `use` lines, `default_all`, save paths); `App::set_layout`/`cycle_theme`-era layout code deleted; `effective_layout` deleted; config_view DISPLAY = THEME ◂▸ and SORT ◂▸. Un-ignore Task 2's config test.
- [ ] **Step 4: Run tests** — full suite green.
- [ ] **Step 5: Commit**

```bash
git add -A src tests README.md
git commit -m "feat(v3.2): :sort/:tv and s/v keys; layout and score grammar removed, old config keys ignored"
```

---

### Task 11: The cut — takeover and band, scoped (spec §3)

**Files:**
- Create: `src/board/cut.rs`
- Modify: `src/app/mod.rs` (`CutState` field; firing in `apply_boards`/`merge_summary`; suppression), `src/app/chrome.rs` or `App::draw` (render overlay last, like `draw_help`), `src/theme.rs` (`scoring_word` gains FIELD GOAL/SAFETY arms if missing)
- Test: `src/board/cut.rs`, `tests/draw.rs`

**Interfaces:**
- Consumes: `hero::score_block` (the hard rule), `theme::scoring_word`, `tui_big_text` for the word.
- Produces (exact):

```rust
/// One firing. `full` = the takeover; false = the 2-row band.
pub struct Cut { pub game_id: String, pub play: Play, pub full: bool, pub until_tick: u64 }
#[derive(Default)]
pub struct CutState { /* active: Option<Cut> */ }
impl CutState {
    /// Called with every newly captured scoring play (the Task-8-era delta
    /// path). `full` when the team is pinned/favorited or TV is on; refused
    /// entirely during the first 30 s (startup history) and while a prompt
    /// or help is open (spec §3) — the caller passes those flags.
    pub fn fire(&mut self, game_id: &str, play: &Play, full: bool, tick: u64);
    pub fn active(&self, tick: u64) -> Option<&Cut>;
}
/// Takeover lifetime 3 s; band 1.5 s (spec §3).
pub const CUT_TICKS: u64 = 30; pub const BAND_TICKS: u64 = 15;
/// Draw the takeover: header row survives; rows 1.. cleared; chip line,
/// the scoring word in PixelSize::Full block letters, score_block, one
/// detail line from the Play, dimmed bottom strip.
pub fn draw_takeover(frame: &mut Frame, area: Rect, game: &Game, play: &Play);
/// Draw the band into the 2 rows above the list.
pub fn draw_band(frame: &mut Frame, area: Rect, game: &Game, play: &Play);
```

Firing wiring in `App`: the delta-capture block in `apply_boards` (and the summary merge when it appends a NEW scoring play for the zoomed game) calls `self.cuts.fire(id, play, full, tick)` where `full = pinned || favorited || matches!(self.view, View::Tv)`; suppression flags: `self.tick < 30 * LIVE_TICKS_PER_SEC || self.mode != InputMode::Normal || self.help_open` → don't fire at all. Bell rings only for `full` (the band is quiet). Render: in `App::draw` after `views::draw`, `if let Some(cut) = self.cuts.active(self.tick)` → takeover replaces rows 1.. (board behind is NOT drawn — clear + draw, per the A′ cut frame), band is handed to `board::draw` to insert above the list. Overlay pins the loop to `LIVE_TICK` while active (same trick as alerts).

- [ ] **Step 1: Write the failing tests**:

```rust
// src/board/cut.rs
#[test] fn fire_scoping_and_expiry() { /* full flag honored; active() None after until_tick; a second fire while active replaces only if full && !active.full (a takeover is never downgraded) */ }
// tests/draw.rs
#[test]
fn the_takeover_and_the_hero_agree_on_every_digit_cell() {
    // Render the board hero for KC 24 BUF 21 into one TestBackend; render
    // draw_takeover for the same Game into another; assert the digit glyph
    // cells (chars + fg) inside score_block's rect are identical. THE hard-rule test.
}
#[test]
fn a_pinned_score_takes_the_screen_and_an_unpinned_one_is_a_band() {
    // pin KC; delta KC -> takeover: "TOUCHDOWN" letters present, header row
    // still shows GAMEDAY. Unpin, delta TEX -> two-row band above IN PLAY,
    // board still visible below.
}
#[test]
fn cuts_are_suppressed_during_prompts_and_startup() { /* mode = Filter, delta → no cut; tick < 300 → no cut */ }
```

- [ ] **Step 2: Run to verify failure** — compile errors.
- [ ] **Step 3: Implement.** The word: `game.league` → `theme::scoring_word` (TOUCHDOWN/HOME RUN/GOAL/BUCKET…); render via `tui_big_text::BigText` `PixelSize::Full` centered, `roles().hot`; if the word at Full exceeds the width (TOUCHDOWN = 9 glyphs × 8 = 72 cols), step down to `PixelSize::Sextant`; below that, a plain bold line — never clipped letters. Detail line: `{surname} · {yards/desc from play.text truncated} · {period clock}` — built from the `Play` fields only.
- [ ] **Step 4: Run tests + dump: add nothing to the gallery yet (Task 15) but eyeball via a scratch `--tick` run if quick.**
- [ ] **Step 5: Commit**

```bash
git add src/board/cut.rs src/app src/theme.rs tests/draw.rs
git commit -m "feat(v3.2): the cut — scoped takeover and band, one formatter with the hero, suppressed during prompts"
```

---

### Task 12: `:tv` (spec §3 TV)

**Files:**
- Create: `src/views/tv.rs`
- Modify: `src/views/mod.rs` (route `View::Tv`), `src/app/mod.rs` (TV keys: `space` lock, `n` next, `Esc`/`v` back; auto-cut state), `src/app/chrome.rs` (footer ctx for TV: `space lock  n next  esc board  q quit`)
- Test: `tests/draw.rs`, `src/app/mod.rs`

**Interfaces:**
- Consumes: `hero::{draw_hero, score_block}`, `layout` (TV is `hero_rows = body - strip`), `rows::draw_tier2` (the strip rows), `CutState` (every cut is `full` in TV).
- Produces: `View::Tv` draws: the hero game at full body height (digits `PixelSize::Full`; linescore row when present — reuse Task 13's `linescore_lines` if landed, else the v3.1 zoom one; meter; last three plays with clock stamps); bottom `ALSO LIVE — n games` strip = tier-2 rows for the rest (hot marks live); `next cut: CHW 2 HOU 3` bottom-right when the ranking's top differs from the shown game (the switch happens on the next EVENT, not a timer — spec §3); `space` toggles `tv_lock: Option<String>` (locked game never auto-switches; footer shows `space unlock`); `n` advances to the next live game manually; selection/`j/k` inert.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn tv_fills_the_screen_with_the_hero_and_strips_the_rest() {
    // 6 live at 120x40 in View::Tv: Full digits present (a row of 8-tall
    // glyph cells), "ALSO LIVE" strip lists the other 5, no IN PLAY rule.
}
#[test]
fn tv_auto_cuts_on_event_not_on_timer_and_lock_holds() {
    // Shown game A; make B outrank A with no event → still A and
    // "next cut:" names B. Fire an event (apply_boards delta) → B shown.
    // space → locked; make C outrank; event; still B.
}
#[test]
fn every_scoring_play_takes_the_screen_in_tv() { /* unpinned delta while View::Tv → takeover */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** (`tv_shown: Option<String>` + `tv_lock` on App; the "shown" updates inside `on_event` wiring: after `order.on_event`, if TV and unlocked and `order` top changed → `tv_shown = top`).
- [ ] **Step 4: Run tests; eyeball a `--tick` TV capture vs `docs/research/v3-identity/tv-nfl-sunday-120x40.png`.**
- [ ] **Step 5: Commit**

```bash
git add src/views/tv.rs src/views/mod.rs src/app tests/draw.rs
git commit -m "feat(v3.2): :tv — jumbotron hero, ALSO LIVE strip, event-gated auto-cut with lock"
```

---

### Task 13: Zoom rebuild (spec §5)

**Files:**
- Modify: `src/views/zoom.rs` (Overview tab body), `src/board/hero.rs` (nothing — consumed)
- Test: `tests/draw.rs`

**Interfaces:**
- Consumes: `hero::draw_hero` (full-width, `digits_full` when it fits), the v3.1 `linescore_lines` (move it into `zoom.rs` scope if it isn't already), `Situation.{pitcher,batter,due_up}`, `Game.timeouts`, `Extras::Soccer.events`.
- Produces: Zoom/Overview = hero block (same function as the board) + linescore table + one matchup/state line per sport — MLB `P: G. Kirby (…) · AB: R. Devers (…) · DUE UP a, b, c` from `situation.pitcher/batter/due_up`; football `TIMEOUTS ●●○ | ●●●  ·  KC BALL` from `timeouts`+possession; soccer the last 3 `MatchEvent`s (`24' ⚽ D. Ndoye · 61' 🟨 …` — use `G`/`OG`/`PEN`/`Y`/`R` letters, not emoji, per the terminal-legal rule) — then LAST PLAYS and SCORING as today. PLAYS/STATS tabs unchanged. This renders the 3.1-mapped fields the final review called "tested, dead data" (R18 note).

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn zoom_overview_reuses_the_hero_and_shows_the_matchup_line() {
    // MLB zoomed game with pitcher/batter/due_up → "P: " and "AB: " and "DUE UP" present;
    // hero digit cells identical to the board hero for the same game (same fn).
}
#[test] fn football_zoom_shows_timeouts_and_possession() { /* "TIMEOUTS" + ●●○ pattern */ }
#[test] fn soccer_zoom_lists_match_events_with_minute_and_letter() { /* "24' G " + surname */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests; `dump` and eyeball `out/focus.*`.**
- [ ] **Step 5: Commit**

```bash
git add src/views/zoom.rs tests/draw.rs
git commit -m "feat(v3.2): zoom — hero reuse, linescore, per-sport matchup line; the mapped fields finally render"
```

---

### Task 14: Sizes and resize — the sweep (spec §4)

**Files:**
- Modify: `src/board/{mod,layout,hero,rows}.rs` as the sweep demands, `src/app/mod.rs` (`need 40×12, have WxH` guard keeps both numbers)
- Test: `tests/draw.rs`

**Interfaces:** none new — this task hardens Tasks 5–9 against every size.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn the_board_survives_every_size_the_app_will_draw_at() {
    // The v3.1 clock sweep pattern, board edition: for w in [40,55,60,80,100,120,180],
    // h in [12,16,24,30,40,60]: draw 14 games (8 live incl. one hot, 3 final,
    // 3 later, 2 pinned) and assert: no panic; the key legend is the last row;
    // the selected row is visible after 10 j presses; if h>=12 the hero (or
    // compact hero) exists — its game's away abbr appears above the IN PLAY rule;
    // no line exceeds w (TestBackend guarantees, but assert no '…'-free hard
    // clip by checking the rightmost column isn't mid-word for section rules).
}
#[test]
fn the_minimum_size_message_names_both_numbers() {
    // 39x11 → "need 40×12, have 39×11".
}
#[test]
fn resize_relayouts_from_the_same_list() {
    // Draw at 120x40, then same App at 80x24: selection preserved by id,
    // no stale hero (the hero may demote but the id set is identical).
}
```

- [ ] **Step 2: Run to verify failure** — expect real failures at the odd sizes (that is the point).
- [ ] **Step 3: Fix what the sweep finds** in layout/hero/rows (each fix gets its own receipt comment naming the failing size).
- [ ] **Step 4: Run the full suite.**
- [ ] **Step 5: Commit**

```bash
git add src/board src/app/mod.rs tests/draw.rs
git commit -m "feat(v3.2): size sweep — the ladder holds from 40x12 to 180x60, minimum message names both numbers"
```

---

### Task 15: Demo, gallery, docs (spec §8 Gallery, §7 cleanup)

**Files:**
- Modify: `src/demo.rs`/`src/sim.rs` (richer demo: at-bat MLB plays with inning tags, one hot game per scenario tick, a pinned KC, invented scoring plays already exist), `src/dump.rs` (stems), `src/tiles/mod.rs` (delete now-dead helpers the board no longer calls; `src/tiles/` may collapse into `src/board/` re-exports if empty — keep `tiles::meter_line`/`digit_glyphs` homes wherever they landed in Task 6/8), `src/theme.rs` (delete `Discipline` helpers nothing calls: `chip`, `section_label`, `team_text`, `league_text`, `clock`, `sidebar_header`, `SidebarHeader(s)` — grep first; anything still called stays), `README.md` (keys/`dump` stems/theme list)
- Test: `src/dump.rs` stem tests

**Interfaces:** gallery stems become: `board-broadcast`, `board-studio`, `board-gruvbox` (three themes × the ranked board), `board-narrow` (80×24), `board-sixty` (60×40), `tv`, `cut-full`, `cut-band`, `zoom`, `plays-feed`, `standings`, `config`, `filter`, `theme-picker`, `help`, `home-live`, `offline`, `stale`, `config-error`, plus `nudge-seq` (three `--tick` frames around a scripted re-sort showing the `↑n` gutter). Old stems (`board-<six tints>`, `board-compact`, `tab-nfl`, `focus`, `narrow`) are deleted with their variants; the stem-contract test pins the new list. Demo data: give the sim one scripted event at a known tick that re-sorts the board (the nudge capture), a red-zone tick, and MLB plays as at-bat text with `period` tags so `[B7]` finally has a visual receipt (v3.1 deferred item).

- [ ] **Step 1: Update the stem test to the new list; run — RED.**
- [ ] **Step 2: Implement demo/sim/dump changes; delete dead theme/tile helpers (grep-verified).**
- [ ] **Step 3: `cargo run --release -- dump` with the font; open EVERY new stem PNG side-by-side with its A′ counterpart** (`board-broadcast` vs `tonight-120x40`, `tv` vs `tv-nfl-sunday-120x40`, `cut-full` vs `cut-fullscreen…`, `board-narrow` vs `tonight-80x24`). They must read as the same design executed on demo data.
- [ ] **Step 4: Full suite; README: Keys line (`s sort · v tv`), dump stems paragraph, themes paragraph (three built-ins, user files still load, old names still valid as user themes).**
- [ ] **Step 5: Commit**

```bash
git add -A src tests README.md
git commit -m "feat(v3.2): gallery rebuilt around the ranked board; demo scripts a re-sort, a red zone, and real at-bat text; dead grammar deleted"
```

---

### Task 16: Definition-of-done sweep (spec §10)

**Files:** receipts appended to the spec; no new code beyond what the receipts demand.

- [ ] **Step 1:** `cargo test` all green; `cargo clippy --all-targets` zero warnings — record both lines.
- [ ] **Step 2: Side-by-side.** Regenerate the gallery with `GAMEDAY_DUMP_FONT`; montage or open `out/board-broadcast.png` beside `docs/research/v3-identity/tonight-120x40.png` and `out/tv.png` beside the TV frame. Record "matches the grammar" or the specific divergence — the OWNER looks before merge (spec §10; do not self-certify: the report must say "awaiting Walter's eye" as its verdict).
- [ ] **Step 3: Live captures.** tmux, scratch `--config-dir`, 120×40 and 80×24 on a live night: the board with real games; `v` for TV; if a pinned team scores during the window, the takeover (this also closes v3.1's deferred live-scoring receipt — pin a live favorite and wait ≤15 min). Save `.txt` captures; note honestly what fired and what didn't.
- [ ] **Step 4: CPU.** `--demo` 30 s idle + live-cadence samples vs the v3.1 numbers (0.46% live demo) — must not regress above v3.1's live number by more than jitter; record.
- [ ] **Step 5:** Append `## Verification (date)` to the spec with the receipts; commit `docs(v3.2): verification receipts — tests, clippy, side-by-sides, live captures, CPU`.

---

## Self-review

**Spec coverage.** §1 board anatomy → T5 (budget), T6 (hero), T7 (tiers/marks/nudge gutter), T8 (sections/band/selection/scroll), T9 (header/footer/ticker-lane); §2 watchability/ordering → T1, T2 (+ hero pick in T8's `Derived.hero_id`); §3 cut → T11, TV → T12; §4 sizes → T5 encodes, T14 enforces; §5 zoom → T13; §6 themes/roles/hue-separation → T3 (+ hero use in T6); §7 deletions → T8 (grammar/packer/sidebar/paging), T10 (commands/config), T15 (dead helpers, tints, stems); §8 tests/gallery → per-task + T15; logo decision → T4 (regenerate/slim), T6 (flanks), T15 (gallery shows them); theme decision A → T3; §9 non-goals respected (no new leagues/providers; `:sort`/`:tv` are the only grammar additions); §10 → T16.

**Ordering/dependency check.** T2 wires `on_event` into `apply_boards` before the board exists — harmless (state maintained, unread until T8). T4's shims keep the tile grammar compiling until T8 deletes it. T10's `View::Tv` placeholder precedes T12's body. T11 consumes `hero::score_block` (T6). T12 may use T13's linescore or fall back to the v3.1 zoom helper — stated in T12. T15 deletes only what grep proves dead.

**Placeholder scan.** The hero/rows test bodies in T6/T7 are outlines with named assertions rather than full literals — deliberate: they assert buffer cells whose exact coordinates depend on T6's implementation choices; each names the exact property (fg == roles.digits, cell equality, gutter x-stability) the implementer must pin. Every other code block is concrete. No TBD/TODO anywhere.

**Type consistency.** `Watch`/`SortKey`/`OrderState::{on_event,ordered,nudge}` (T1/T2) match T7's `RowCtx` consumption and T8's `Derived`; `hero::score_block(frame, area, game, full)` is what T11's equality test and T12 call; `TierPlan` fields match T8's walk and T9's `scores_lane` gate; `Roles`/`hero_pair` (T3) match T6's usage; `hero_mark`/`draw_hero_mark` (T4) match T6. `NUDGE_TICKS` uses `app::LIVE_TICKS_PER_SEC` (exists since v1). `CUT_TICKS`/`BAND_TICKS` receipts are spec §3's 3 s/1.5 s.
