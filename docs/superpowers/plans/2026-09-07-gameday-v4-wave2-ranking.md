# gameday v4 Wave 2 — Ranking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The hero is the game a knowledgeable fan would pick: the review-day slate leads with Boise at Oregon instead of an FCS blowout, favorites steer the order, and the leading reason is visible in the footer.

**Architecture:** `rank::watchability` stays a pure function of the `Game`; the new inputs ride on the game (`Situation.win_prob` mapped from ESPN's scoreboard `lastPlay.probability`; `Game.favorite` stamped by the app from config after every apply and every favorites edit). The formula becomes `base + situation × closeness/100 + matchup + favorite`; for football, closeness and lateness come from win probability where the feed carries it (CFB on now; NFL behind a one-arm gate until spec §8.5's capture). Leverage enters the reorder fingerprint as a band of 20 so drift never moves a row. `Watch.why` names the largest term and the footer prints it for the selected game.

**Tech Stack:** Rust 2021; `src/rank.rs` unit tests; `tests/draw.rs`; a committed copy of the review-day CFB scoreboard.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §4 (Wave 2), Direction 6, §1 R1–R3.

## Global Constraints

- Branch `v4-wave2` from `main` (`80e1fa2`). Commit after every task with the trailer; never push.
- Suite is 574 at the start; every task ends green with the count reported. `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --check` clean before every commit.
- The board reorders only on a real event: rank and favorite are static per game and never enter the fingerprint; leverage enters as a band (`closeness / 20`, 0..=5); a band crossing is an event, drift inside one is not.
- Weights are guesses calibrated by the review-slate test; each carries a comment saying so: matchup both ranked 20, one ranked 10, top-five +5; favorite 25; situation bonuses × closeness/100.
- Leverage: `closeness = 100 − |2·home − 100|` in percent (100 at 50/50, 0 at 0 or 100); `lateness = elapsed / regulation` from `seconds_left` (regulation = 4 × quarter length); a period label of `OT` pins lateness to 100. `leverage_enabled(League::Cfb) == true`, every other league `false` (NFL turns on in wave 6 when the week-one capture shows the field).
- `Situation` derives `Eq`, so win probability is stored as integers: `WinProb { home_permille: u16, away_permille: u16, seconds_left: u32 }`.
- Names (exact): `domain::WinProb`; `Situation.win_prob: Option<WinProb>`; `Game.favorite: bool`; `App::mark_favorites(&mut self)` (`pub(crate)`); `rank::leverage_enabled(League) -> bool`; `rank::leverage_closeness(&WinProb) -> u32`; `rank::leverage_lateness(League, &str /*period*/, &WinProb) -> u32`; `rank::leverage_band(&Game) -> u8`; `Watch.why: &'static str`; `RankFingerprint` gains `u8` as its last element; fixture `fixtures/review-slate-2026-09-05.json`.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `src/domain.rs` | `WinProb`, `Situation.win_prob`, `Game.favorite` |
| `src/provider/map.rs` | maps `situation.lastPlay.probability` |
| `src/app/merge.rs`, `src/app/keys/board.rs`, `src/app/keys/config.rs`, `src/app/mod.rs` | `mark_favorites` and its call sites |
| `src/rank.rs` | the formula, leverage helpers, `why` |
| `src/app/order.rs` | fingerprint band |
| `src/app/chrome.rs` | footer `LEADS: <why>` |
| `fixtures/review-slate-2026-09-05.json`, `fixtures/PROVENANCE-review-slate.md` | the Saturday that exposed R1 |
| `tests/map_espn.rs`, `src/rank.rs` tests, `src/app/tests.rs`, `tests/draw.rs`, `tests/ranking_slate.rs` | the teeth |

---

### Task 1: `WinProb` on the situation; `Game.favorite` stamped by the app; the review-slate fixture

**Files:**
- Modify: `src/domain.rs`, `src/provider/map.rs`, `src/app/mod.rs`, `src/app/merge.rs`, `src/app/keys/board.rs`, `src/app/keys/config.rs`, `tests/map_espn.rs`, `src/app/tests.rs`
- Create: `fixtures/review-slate-2026-09-05.json`, `fixtures/PROVENANCE-review-slate.md`

**Interfaces:**
- Produces: `WinProb`, `Situation.win_prob`, `Game.favorite`, `App::mark_favorites`. Every `Game { … }` literal without `..Default::default()` gains `favorite: false`; every `Situation { … }` literal without it gains `win_prob: None` (the compiler lists them).

- [ ] **Step 1: The fixture**

Copy the review-day slate into place and write its provenance:

```bash
cp /private/tmp/claude-501/-Users-wallymagill-personal-projects-game-day/b1a5021a-fa03-41f2-ae2a-ab52f0282c0c/scratchpad/review-slate-cfb-2026-09-05.json fixtures/review-slate-2026-09-05.json
jq -c '{events: (.events|length), live: ([.events[] | select(.status.type.state=="in")] | length), prob: ([.events[] | .competitions[0].situation.lastPlay.probability? | select(.!=null)] | length)}' fixtures/review-slate-2026-09-05.json
```

Expected: `{"events":68,"live":18,"prob":16}`. If the scratchpad copy is gone, the same payload is `~/.config/gameday/cache/cfb-scoreboard` only if its mtime is still 2026-09-05 16:52; otherwise stop and report BLOCKED (the slate cannot be re-captured).

`fixtures/PROVENANCE-review-slate.md`:

```markdown
# fixtures/review-slate-2026-09-05.json

The CFB scoreboard (`…/football/college-football/scoreboard?groups=80&limit=300&dates=20260905`) as the app's own cache held it at 2026-09-05 16:52 EDT, during the independent review that started the v4 ship pass. Byte-faithful `curl | jq '.'` shape (the cache is the raw body), 68 events, 18 live, 16 carrying `situation.lastPlay.probability`.

It pins finding R1 (spec §1): with the v3.4 formula the hero was FOR at NDSU (FCS, 0-17, a 2-MIN chip) while No. 2 Oregon trailed Boise State 7-17 in the second quarter at a 28% home win probability. `tests/ranking_slate.rs` asserts the wave-2 formula leads with Boise at Oregon.
```

- [ ] **Step 2: Failing tests**

`tests/map_espn.rs`:

```rust
#[test]
fn win_probability_maps_from_the_football_scoreboard() {
    let games = map_scoreboard(League::Cfb, include_str!("../fixtures/review-slate-2026-09-05.json"), et()).unwrap();
    let bois = games.iter().find(|g| g.away.abbr == "BOIS" && g.home.abbr == "ORE").expect("Boise at Oregon is on the slate");
    let wp = bois.situation.as_ref().and_then(|s| s.win_prob).expect("a live football game carries probability");
    assert_eq!(wp.home_permille, 285, "0.2847 → 285 permille");
    assert_eq!(wp.away_permille, 715);
    assert_eq!(wp.seconds_left, 2116);
    let pre = games.iter().find(|g| g.status == Status::Pre).expect("a pre-game event");
    assert!(pre.situation.as_ref().map_or(true, |s| s.win_prob.is_none()));
    // No other league's scoreboard carries the field (checked on the review caches).
    let mlb = map_scoreboard(League::Mlb, include_str!("../fixtures/live/mlb_scoreboard_live.json"), et()).unwrap();
    assert!(mlb.iter().all(|g| g.situation.as_ref().map_or(true, |s| s.win_prob.is_none())));
}
```

`src/app/tests.rs`:

```rust
#[test]
fn favorites_are_stamped_on_apply_and_after_an_edit() {
    let mut app = app_with(vec![], vec![]);
    app.config.favorites = vec![crate::config::Favorite { league: League::Nfl, team_abbr: "KC".into() }];
    let kc = g("1", "KC", "TB", true);
    let other = g("2", "GB", "CHI", true);
    app.apply_boards(League::Nfl, vec![kc, other], false);
    assert!(app.game_by_id("1").unwrap().favorite);
    assert!(!app.game_by_id("2").unwrap().favorite);
    // A favorites edit re-stamps every board without waiting for a poll.
    app.config.favorites.clear();
    app.mark_favorites();
    assert!(!app.game_by_id("1").unwrap().favorite);
    // The board key path (t on the selected game) stamps too.
    app.selected = 0;
    app.on_key(crossterm::event::KeyCode::Char('t'), crossterm::event::KeyModifiers::NONE);
    let stamped = app.game_by_id("1").unwrap().favorite || app.game_by_id("2").unwrap().favorite;
    assert!(stamped, "t favorites the selected game's home team and re-stamps");
}
```

Run: `cargo test --release --test map_espn win_probability 2>&1 | grep -E 'error\[|test result' | head -2`
Expected: compile error (`win_prob` unknown).

- [ ] **Step 3: Implement**

`src/domain.rs`, beside `Situation`:

```rust
/// ESPN's `situation.lastPlay.probability` on football scoreboards (verified on
/// the 2026-09-05 CFB slate: 16 of 18 live games carried it; absent on every
/// other league's scoreboard in the review caches). Permille so `Situation`
/// stays `Eq`; `seconds_left` is regulation seconds remaining as ESPN counts
/// them. The ranking reads it for closeness and lateness where
/// `rank::leverage_enabled` says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinProb {
    pub home_permille: u16,
    pub away_permille: u16,
    pub seconds_left: u32,
}
```

Add `pub win_prob: Option<WinProb>,` to `Situation` (with `#[derive(Default)]` it defaults to `None`). Add to `Game`:

```rust
    /// One of the two teams is in `config.favorites`. Stamped by
    /// `App::mark_favorites` after every apply and every favorites edit; the
    /// mapper never sets it. Read by `rank::watchability` for the favorite term.
    pub favorite: bool,
```

and `favorite: false` in `Game::default()`.

`src/provider/map.rs`, in the situation construction:

```rust
            win_prob: win_prob_from(&sit_v["lastPlay"]["probability"]),
```

```rust
/// `lastPlay.probability` → permille. All three fields required, else None.
fn win_prob_from(p: &Value) -> Option<WinProb> {
    let home = p["homeWinPercentage"].as_f64()?;
    let away = p["awayWinPercentage"].as_f64()?;
    let seconds_left = p["secondsLeft"].as_u64()?;
    let permille = |x: f64| (x.clamp(0.0, 1.0) * 1000.0).round() as u16;
    Some(WinProb {
        home_permille: permille(home),
        away_permille: permille(away),
        seconds_left: seconds_left.min(u32::MAX as u64) as u32,
    })
}
```

`src/app/mod.rs` (or `merge.rs`, beside `apply_boards`):

```rust
    /// Stamp `Game.favorite` from `config.favorites` on every board. Called
    /// at the end of `apply_boards` (before the reorder check) and after every
    /// favorites edit, so the flag never waits for a poll.
    pub(crate) fn mark_favorites(&mut self) {
        let favs = self.config.favorites.clone();
        for board in self.boards.values_mut() {
            for g in board.iter_mut() {
                g.favorite = favs.iter().any(|f| {
                    f.league == g.league
                        && (f.team_abbr.eq_ignore_ascii_case(&g.home.abbr)
                            || f.team_abbr.eq_ignore_ascii_case(&g.away.abbr))
                });
            }
        }
    }
```

Call it in `apply_boards` right after `self.boards.insert(league, games);` and before the `if !stale { … maybe_reorder … }` block; and after each `favorites.push`/`favorites.remove` in `keys/board.rs` (`toggle_favorite`) and `keys/config.rs` (`config_add_favorite`, `config_remove_favorite`).

- [ ] **Step 4: Run and commit**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'` → `576 tests, 0 failed`; clippy; `cargo fmt`.

```bash
git add -A src tests fixtures/review-slate-2026-09-05.json fixtures/PROVENANCE-review-slate.md && git commit -q -m "feat(rank): win probability on football situations; favorites stamped on the game; the review-day slate as a fixture

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 2: The formula — scaled bonuses, matchup, favorite, leverage, `why`

**Files:**
- Modify: `src/rank.rs` (+ its tests)

**Interfaces:**
- Produces: `Watch { score, hot, chip, why }`; `pub fn leverage_enabled(league: League) -> bool`; `pub fn leverage_closeness(p: &WinProb) -> u32`; `pub fn leverage_lateness(league: League, period: &str, p: &WinProb) -> u32`; `watchability(g, now)` signature unchanged.

- [ ] **Step 1: Failing tests** (in `src/rank.rs`'s `mod tests`, reusing its `g(league, period, clock, away, home)` helper)

```rust
    fn wp(home_permille: u16, seconds_left: u32) -> WinProb {
        WinProb { home_permille, away_permille: 1000 - home_permille, seconds_left }
    }

    #[test]
    fn leverage_math_at_the_three_points() {
        assert_eq!(leverage_closeness(&wp(500, 1800)), 100);
        assert_eq!(leverage_closeness(&wp(750, 1800)), 50);
        assert_eq!(leverage_closeness(&wp(999, 1800)), 0, "0.999 rounds to nothing left to play for");
        assert_eq!(leverage_closeness(&wp(285, 1800)), 57, "Boise at Oregon");
        assert_eq!(leverage_lateness(League::Cfb, "Q2", &wp(500, 1800)), 50);
        assert_eq!(leverage_lateness(League::Cfb, "Q4", &wp(500, 0)), 100);
        assert_eq!(leverage_lateness(League::Cfb, "OT", &wp(500, 600)), 100, "overtime pins lateness");
        assert!(leverage_enabled(League::Cfb));
        assert!(!leverage_enabled(League::Nfl), "off until the week-one capture shows the field");
        assert!(!leverage_enabled(League::Mlb));
    }

    #[test]
    fn situation_bonuses_scale_with_closeness() {
        let mut close = g(League::Nfl, "Q3", "9:05", 21, 17);
        close.situation = Some(Situation { is_red_zone: Some(true), possession: Some("AAA".into()), ..Default::default() });
        let mut blowout = g(League::Nfl, "Q3", "9:05", 42, 7);
        blowout.situation = close.situation.clone();
        let now = OffsetDateTime::now_utc();
        let (wc, wb) = (watchability(&close, now), watchability(&blowout, now));
        assert_eq!(wc.chip, Some("RED ZONE"));
        assert_eq!(wb.chip, Some("RED ZONE"), "the chip still names the situation");
        assert!(wb.hot, "hot follows the chip");
        // Closeness 0 at a 35-point margin: the bonus contributes nothing to the score.
        assert!(wc.score > wb.score + 30, "close {} vs blowout {}", wc.score, wb.score);
    }

    #[test]
    fn matchup_and_favorite_terms() {
        let now = OffsetDateTime::now_utc();
        let base = g(League::Nfl, "Q2", "7:00", 10, 7);
        let plain = watchability(&base, now).score;
        let mut one = base.clone();
        one.home.rank = Some(12);
        assert_eq!(watchability(&one, now).score, plain + 10);
        let mut both = one.clone();
        both.away.rank = Some(20);
        assert_eq!(watchability(&both, now).score, plain + 20);
        let mut top = both.clone();
        top.home.rank = Some(3);
        assert_eq!(watchability(&top, now).score, plain + 25);
        let mut fav = base.clone();
        fav.favorite = true;
        assert_eq!(watchability(&fav, now).score, plain + 25);
    }

    #[test]
    fn football_leverage_replaces_the_margin_path_only_where_enabled() {
        let now = OffsetDateTime::now_utc();
        let mut cfb = g(League::Cfb, "Q2", "7:27", 17, 7);
        cfb.situation = Some(Situation { win_prob: Some(wp(285, 2116)), ..Default::default() });
        let mut nfl = cfb.clone();
        nfl.league = League::Nfl;
        let (c, n) = (watchability(&cfb, now), watchability(&nfl, now));
        // CFB: closeness 57, lateness (3600-2116)/3600 = 41 → base 23.
        assert_eq!(c.score, 23, "{c:?}");
        // NFL, gated off: margin path (margin 10 → closeness 50; Q2 7:27 → lateness 37) → 18.
        assert_eq!(n.score, 18, "{n:?}");
        assert_eq!(c.why, "LEVERAGE");
    }

    #[test]
    fn why_names_the_leading_term() {
        let now = OffsetDateTime::now_utc();
        let mut fav = g(League::Nfl, "Q1", "14:00", 0, 0);
        fav.favorite = true;
        assert_eq!(watchability(&fav, now).why, "FAVORITE");
        let mut ranked = g(League::Nfl, "Q1", "14:00", 0, 0);
        ranked.home.rank = Some(1);
        ranked.away.rank = Some(2);
        assert_eq!(watchability(&ranked, now).why, "RANKED");
        // Early and tied, so the red zone (40 × closeness) outweighs the base.
        let mut rz = g(League::Nfl, "Q1", "10:00", 7, 7);
        rz.situation = Some(Situation { is_red_zone: Some(true), possession: Some("AAA".into()), ..Default::default() });
        assert_eq!(watchability(&rz, now).why, "RED ZONE");
        let tight = g(League::Nba, "Q3", "6:00", 80, 79);
        assert_eq!(watchability(&tight, now).why, "CLOSE");
        let late = g(League::Nba, "Q4", "0:30", 110, 90);
        assert_eq!(watchability(&late, now).why, "LATE");
        assert_eq!(watchability(&g(League::Nfl, "Q1", "15:00", 0, 0), now).why, "CLOSE", "a scoreless opener is close before it is anything else");
        let mut pre = g(League::Nfl, "Q1", "15:00", 0, 0);
        pre.status = Status::Pre;
        assert_eq!(watchability(&pre, now).why, "");
    }
```

Run: `cargo test --release rank::tests::leverage_math 2>&1 | grep -E 'error\[|test result' | head -2`
Expected: compile error.

- [ ] **Step 2: Implement** (replace the body of `watchability` and add the helpers; keep `closeness`, `lateness`, `clock_secs`, `quarter_len`, `one_score`)

```rust
/// Which leagues' scoreboards carry `lastPlay.probability` and are trusted
/// for it. CFB: verified on the 2026-09-05 slate. NFL: unverified until the
/// week-one capture (spec §8.5); off until then. No other league sends it.
pub fn leverage_enabled(league: League) -> bool {
    matches!(league, League::Cfb)
}

/// 100 at a coin flip, 0 when one side is certain: `100 − |2·home − 100|`.
pub fn leverage_closeness(p: &WinProb) -> u32 {
    // In permille, then to percent: 285 → 57, 750 → 50, 999 → 0, 500 → 100.
    let h = p.home_permille.min(1000) as i32;
    ((1000 - (2 * h - 1000).abs()).clamp(0, 1000) / 10) as u32
}

/// Elapsed share of regulation from ESPN's `secondsLeft`; overtime pins to 100.
pub fn leverage_lateness(league: League, period: &str, p: &WinProb) -> u32 {
    if period == "OT" {
        return 100;
    }
    let reg = 4 * quarter_len(league);
    let left = p.seconds_left.min(reg);
    ((reg - left) * 100 / reg).min(100)
}

/// Leverage band for the reorder fingerprint: closeness in steps of 20, so a
/// drift inside a band is not an event and a crossing is. 0 when the game
/// carries no leverage.
pub fn leverage_band(g: &Game) -> u8 {
    match g.situation.as_ref().and_then(|s| s.win_prob) {
        Some(p) if leverage_enabled(g.league) => (leverage_closeness(&p) / 20).min(5) as u8,
        _ => 0,
    }
}
```

`Watch` gains `pub why: &'static str` (empty for non-live). Inside `watchability`, after the non-live early return:

```rust
    let margin = g.home_score.abs_diff(g.away_score) as u32;
    // Football with a win probability: ESPN's own read of the game, which is
    // what closeness and lateness are trying to approximate from the score
    // and the clock. Everything else keeps the margin path.
    let leverage = g
        .situation
        .as_ref()
        .and_then(|s| s.win_prob)
        .filter(|_| leverage_enabled(g.league));
    let (l, c, base_why) = match leverage {
        Some(p) => (leverage_lateness(g.league, &g.period, &p), leverage_closeness(&p), "LEVERAGE"),
        None => {
            let l = lateness(g.league, &g.period, &g.clock);
            let c = closeness(g.league, margin);
            (l, c, if c >= l { "CLOSE" } else { "LATE" })
        }
    };
    let base = l * c / 100;
    let mut score = base;
    let mut hot = false;
    let mut chip: Option<&'static str> = None;
    // (label, value) of the largest single term, for the footer's LEADS.
    let mut lead: (&'static str, u32) = (base_why, base);

    // A situation bonus is worth its full weight only in a close game: a
    // red zone at 35 points down is a chip on the row, not the hero. Guess,
    // calibrated by tests/ranking_slate.rs.
    macro_rules! bonus {
        ($b:expr, $ch:expr) => {
            let scaled = $b * c / 100;
            score += scaled;
            hot = true;
            if chip.is_none() {
                chip = $ch;
            }
            if let Some(name) = $ch {
                if scaled > lead.1 {
                    lead = (name, scaled);
                }
            }
        };
    }
```

(the existing per-league `match g.league { … }` block stays as is, using the macro), then after it:

```rust
    // Matchup: the poll rank ESPN already parses (guess, calibrated by the
    // slate test: Boise at No. 2 Oregon had to beat an FCS 2-MIN).
    let ranks = [g.away.rank, g.home.rank];
    let ranked = ranks.iter().filter(|r| r.is_some()).count();
    let mut matchup = match ranked {
        2 => 20,
        1 => 10,
        _ => 0,
    };
    if ranks.iter().flatten().any(|r| *r <= 5) {
        matchup += 5;
    }
    if matchup > 0 {
        score += matchup;
        if matchup > lead.1 {
            lead = ("RANKED", matchup);
        }
    }
    // Favorite: the viewer said so in config (guess: one tier above a ranked
    // matchup, since it is the viewer's own answer to "what should lead").
    if g.favorite {
        score += 25;
        if 25 > lead.1 {
            lead = ("FAVORITE", 25);
        }
    }

    Watch { score, hot, chip, why: lead.0 }
```

The early non-live return gets `why: ""`. Every `Watch { … }` literal in tests gains `why` (or uses field access).

- [ ] **Step 3: Run**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s " tests, " f " failed"}'`
Expected: `581 tests, 0 failed`. Existing rank tests whose expected scores change because bonuses now scale (a bonus in a lopsided fixture) are updated with a one-line comment each; list them in the report. Existing tests that build `Watch { … }` literals gain `why`.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src && git commit -q -m "feat(rank): matchup, favorite, football leverage; situation bonuses scale with closeness; why names the lead

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 3: The fingerprint band

**Files:**
- Modify: `src/app/order.rs`, `src/app/tests.rs`

**Interfaces:**
- Produces: `RankFingerprint = (u16, u16, Status, bool, Option<(u8, u8)>, u8)` with `rank::leverage_band(g)` as the last element.

- [ ] **Step 1: Failing tests** (`src/app/tests.rs`, reusing `ordering_app`, `ranked`, `ord` where they fit; otherwise `app_with` + `g`)

```rust
fn cfb_with_prob(id: &str, away: u16, home: u16, home_permille: u16) -> Game {
    let mut game = g(id, "AAA", "HHH", true);
    game.league = League::Cfb;
    game.away_score = away;
    game.home_score = home;
    game.period = "Q2".into();
    game.clock = "7:00".into();
    game.situation = Some(Situation {
        win_prob: Some(WinProb { home_permille, away_permille: 1000 - home_permille, seconds_left: 2100 }),
        ..Default::default()
    });
    game
}

#[test]
fn a_leverage_band_crossing_reorders_once_and_freezes() {
    let mut app = app_with(vec![], vec![]);
    app.tick = 400;
    // A is the coin flip (closeness 100, band 5), B is lopsided (950 → closeness 10, band 0).
    let a = cfb_with_prob("a", 10, 10, 500);
    let b = cfb_with_prob("b", 21, 3, 950);
    app.apply_boards(League::Cfb, vec![a.clone(), b.clone()], false);
    let first = ord(&app);
    assert_eq!(first[0], "a");
    // Drift inside B's band: 950 → 930 (closeness 10 → 14, both band 0). No reorder.
    let mut b2 = b.clone();
    b2.situation.as_mut().unwrap().win_prob.as_mut().unwrap().home_permille = 930;
    app.apply_boards(League::Cfb, vec![a.clone(), b2], false);
    assert_eq!(ord(&app), first);
    // A collapses to certain (990 → band 0) while B tightens (520 → closeness 96, band 4): one reorder.
    let mut a3 = a.clone();
    a3.situation.as_mut().unwrap().win_prob.as_mut().unwrap().home_permille = 990;
    let mut b3 = b.clone();
    b3.situation.as_mut().unwrap().win_prob.as_mut().unwrap().home_permille = 520;
    app.apply_boards(League::Cfb, vec![a3.clone(), b3.clone()], false);
    assert_eq!(ord(&app)[0], "b");
    // Drift again inside both bands: frozen.
    let mut b4 = b3.clone();
    b4.situation.as_mut().unwrap().win_prob.as_mut().unwrap().home_permille = 540;
    app.apply_boards(League::Cfb, vec![a3, b4], false);
    assert_eq!(ord(&app)[0], "b");
}

#[test]
fn rank_and_favorite_never_enter_the_fingerprint() {
    let mut app = app_with(vec![], vec![]);
    app.tick = 400;
    let a = cfb_with_prob("a", 10, 10, 500);
    let mut b = cfb_with_prob("b", 10, 10, 500);
    b.id = "b".into();
    app.apply_boards(League::Cfb, vec![a.clone(), b.clone()], false);
    let first = ord(&app);
    // B becomes a favorite and ranked between polls: same scores, same bands → no reorder.
    let mut b2 = b.clone();
    b2.home.rank = Some(1);
    app.config.favorites = vec![crate::config::Favorite { league: League::Cfb, team_abbr: "HHH".into() }];
    app.apply_boards(League::Cfb, vec![a, b2], false);
    assert_eq!(ord(&app), first, "static terms wait for the next real event");
}
```

If `ord` is not available in scope (it lives beside the ordering tests), use the same helper by name; it exists in `src/app/tests.rs`.

Run: `cargo test --release app::tests::a_leverage_band 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL (a band crossing is not an event today) — or a compile error if `WinProb` is not imported in the tests module; add `use crate::domain::WinProb;`.

- [ ] **Step 2: Implement**

`src/app/order.rs`: `pub(super) type RankFingerprint = (u16, u16, Status, bool, Option<(u8, u8)>, u8);` and in the fingerprint construction add `crate::rank::leverage_band(g),` as the sixth element, with a comment: "leverage band: a crossing is an event, drift inside one is not; rank and favorite are static per game and stay out on purpose".

- [ ] **Step 3: Run and commit**

Run: full suite → `583 tests, 0 failed`.

```bash
cargo fmt && git add -A src && git commit -q -m "feat(order): the leverage band joins the reorder fingerprint

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 4: `LEADS:` in the footer

**Files:**
- Modify: `src/app/chrome.rs`, `tests/draw.rs`

- [ ] **Step 1: Failing draw test**

```rust
#[test]
fn the_footer_names_the_selected_games_leading_term() {
    let mut app = mk();
    let mut rz = g("1", "KC", "TB", true);
    rz.period = "Q4".into();
    rz.clock = "1:30".into();
    rz.away_score = 21;
    rz.home_score = 20;
    rz.situation = Some(Situation { is_red_zone: Some(true), possession: Some("KC".into()), ..Default::default() });
    let other = g("2", "GB", "CHI", true);
    app.apply_boards(League::Nfl, vec![rz, other], false);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let footer = s.lines().last().unwrap();
    assert!(footer.contains("LEADS: RED ZONE"), "{footer}");
    // A pre-game selection has no lead.
    let mut app2 = mk();
    app2.apply_boards(League::Nfl, vec![g("3", "NE", "SEA", false), g("4", "NO", "DET", false)], false);
    let mut t2 = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t2.draw(|f| app2.draw(f)).unwrap();
    assert!(!buf_text(&t2).lines().last().unwrap().contains("LEADS"));
    // At 80 columns LEADS is shed before GAME.
    let mut t3 = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t3.draw(|f| app.draw(f)).unwrap();
    let f3 = buf_text(&t3).lines().last().unwrap().to_string();
    assert!(f3.contains("GAME") || !f3.contains("LEADS"), "{f3}");
}
```

Run: `cargo test --release --test draw the_footer_names 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL.

- [ ] **Step 2: Implement**

In `src/app/chrome.rs`, in the `else` branch that pushes `GAME {}/{}`, push the lead first so the shedding loop (which drops from the front) sheds it before `GAME`:

```rust
            let sel_len = self.derived().selection.len();
            // The selected live game's leading term, shed before GAME x/y
            // because it is explanation, not position.
            if let Some(game) = self.derived().selection.get(self.selected) {
                if game.status == Status::Live {
                    let why = crate::rank::watchability(game, self.now()).why;
                    if !why.is_empty() {
                        right.push(format!("LEADS: {why}"));
                    }
                }
            }
            if sel_len > 1 {
                right.push(format!("GAME {}/{}", self.selected + 1, sel_len));
            }
```

(`self.now()` exists on `App`; `selection` is the derived list the footer already reads.)

- [ ] **Step 3: Run and commit**

Full suite → `584 tests, 0 failed`; any footer test that asserted the exact right-cluster text for a live selection is updated and listed.

```bash
cargo fmt && git add -A src tests && git commit -q -m "feat(footer): the selected game's leading term

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 5: The review-slate test

**Files:**
- Create: `tests/ranking_slate.rs`

- [ ] **Step 1: Write the test**

```rust
//! The Saturday that exposed R1: the v3.4 formula led with an FCS 0-17 game
//! carrying a 2-MIN chip while No. 2 Oregon trailed Boise State 7-17 at a 28%
//! home win probability. This test pins the wave-2 order on that exact slate.
use gameday::domain::*;
use gameday::provider::map::map_scoreboard;
use gameday::rank::{top_id, watchability, SortKey};

#[test]
fn the_review_day_slate_leads_with_boise_at_oregon() {
    let games = map_scoreboard(
        League::Cfb,
        include_str!("../fixtures/review-slate-2026-09-05.json"),
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
    )
    .unwrap();
    let live: Vec<Game> = games.into_iter().filter(|g| g.status == Status::Live).collect();
    assert_eq!(live.len(), 18);
    let now = time::OffsetDateTime::now_utc();
    let mut scored: Vec<(u32, &'static str, String)> = live
        .iter()
        .map(|g| {
            let w = watchability(g, now);
            (w.score, w.why, format!("{} {} @ {} {}", g.away.abbr, g.away_score, g.home.abbr, g.home_score))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let top5: Vec<String> = scored.iter().take(5).map(|(s, w, n)| format!("{s:>3} {w:<9} {n}")).collect();
    assert!(scored[0].2.starts_with("BOIS "), "top five:\n{}", top5.join("\n"));
    assert_eq!(top_id(&live, SortKey::Watch, &[League::Cfb], now).as_deref().map(|id| live.iter().find(|g| g.id == id).unwrap().away.abbr.as_str()), Some("BOIS"));
    let fcs_pos = scored.iter().position(|(_, _, n)| n.starts_with("FOR ")).unwrap();
    assert!(fcs_pos >= 3, "FOR at NDSU (0-17) sits at {fcs_pos}; top five:\n{}", top5.join("\n"));
    // Nothing with a win probability leads on lateness alone.
    for g in &live {
        if g.situation.as_ref().and_then(|s| s.win_prob).is_some() {
            assert_ne!(watchability(g, now).why, "LATE", "{}", g.away.abbr);
        }
    }
}
```

- [ ] **Step 2: Run** `cargo test --release --test ranking_slate 2>&1 | grep -E 'test result|top five' -A 6 | head -12`
Expected: `ok`. If Boise at Oregon does not lead, the message prints the top five with scores and why; the weights in Task 2 are the knob, and the commit that changes one must say which and by how much.

- [ ] **Step 3: Commit**

```bash
cargo fmt && git add tests/ranking_slate.rs && git commit -q -m "test(rank): the review-day slate leads with Boise at Oregon

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 6: Receipts

**Files:**
- Modify: `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§9), `CHANGELOG.md`

- [ ] **Step 1:** Append under `## §9 Verification`:

```markdown
### Wave 2 — landed <date>

- Suite: 574 → <n>. `tests/ranking_slate.rs` top five on the 2026-09-05 slate: <paste the five lines with score, why, matchup>.
- Weights: matchup 20/10 (+5 top five), favorite 25, situation × closeness/100, leverage closeness `100 − |2·home − 100|`, lateness from `secondsLeft`; all guesses calibrated by the slate test.
- Leverage: CFB on; NFL gated off until the week-one capture (spec §8.5) — `rank::leverage_enabled`.
- Fingerprint: leverage band (closeness/20) added; rank and favorite excluded; band-crossing test in `src/app/tests.rs`.
- Footer: `LEADS: <why>` for the selected live game, shed before `GAME x/y`.
```

`CHANGELOG.md` `### Changed`: "The board's watchability order now weighs ranked matchups, your favorites, and (college football) ESPN's live win probability; situation chips count in proportion to how close the game is. The footer names why the selected game leads."

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md CHANGELOG.md && git commit -q -m "docs(v4): wave 2 receipts

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

## Self-review against spec §4

§4.1 formula → Task 2; §4.2 leverage + NFL gate → Tasks 1 (data), 2 (math), 6 (receipt); §4.3 chip and why → Tasks 2, 4; §4.4 reorder discipline → Task 3; §4.5 tests → Tasks 1–5; R1 → Task 5; R2 → Tasks 1, 2; R3 → Task 2. Names: `WinProb` fields (`home_permille`, `away_permille`, `seconds_left`) used identically in Tasks 1, 2, 3; `leverage_band` (Task 2) consumed by Task 3; `Watch.why` (Task 2) consumed by Tasks 4, 5; `mark_favorites` (Task 1) called by the key paths named there. Test count ledger: 574 → 576 (T1) → 581 (T2) → 583 (T3) → 584 (T4) → 585 (T5); measured numbers win.
