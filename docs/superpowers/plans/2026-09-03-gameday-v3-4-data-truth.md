# gameday v3.4 Data Truth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every text-parse and data guess with the structured fields ESPN actually publishes — play kinds in all nine leagues, live football situation integers, NHL strength, soccer cards board-wide, finals headlines, and a logo pipeline v2 that completes the pro leagues and unblocks daygame.

**Architecture:** All changes flow through the existing provider (`src/provider/map.rs` + `src/provider/espn.rs`) into `src/domain.rs` types, then out to existing consumers (theme scoring words, rank bonuses, board/zoom chips). No new request patterns — 15s scoreboards + zoomed-game summaries only. Art work is `tools/gen-logos.sh` v2 plus one render gate.

**Tech Stack:** Rust 2021, ratatui 0.29, ureq 2, serde_json, chafa + ImageMagick (art pipeline), the v3.3 `gameday frame` tool for the daygame gate.

**Spec:** docs/superpowers/specs/2026-09-03-gameday-v3-4-data-truth-design.md (binding; Direction A + daygame recorded). Research evidence: `.superpowers/sdd/2026-09-03-sub4-research/findings.md` (id tables, JSON paths, live-verified values — task briefs cite it as "the research").

## Global Constraints

- **Structure over parsing**: where a typed field exists, the text-parse dies in the same task that adds the structure — deleted, not shadowed.
- **No new request load**: 15s scoreboards + zoomed-game summaries only; never fetch a summary slate; never add per-game polls.
- Hard rules stand: one score formatter; R24 (new ranking signals enter the fingerprint set deliberately, with a wiring test proving one event → one reorder → frozen after); logos never move a digit; quadrant-only glyphs (the mark codepoint guard extends to every new art file).
- Unmapped ESPN ids map to `PlayKind::Other` with the id retained — never a panic, never a guess. Every id table row carries the research as its receipt.
- `cargo clippy --all-targets`: ZERO warnings, every task. Full `cargo test` green at DEFAULT parallelism, every task.
- TDD red-first, red run recorded in the task report. Cell-level assertions for UI (R22 precedent: outlined test bodies naming exact properties are plan form; weaker shipped assertions are a review finding).
- NO git stash (shared stack) — red proofs via temp WIP commit + soft reset.
- Numeric constants carry receipts. Errors name the actual value, expected value, and the knob.
- End every commit message with:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

---

### Task 1: Fixture truth — capture tooling fixed, live-state fixtures landed (spec §1)

**Files:**
- Modify: the fixture capture path — grep `fixtures/` references in `tools/` and `docs/` to find how the 29 fixtures were captured (a script if one exists; if capture was manual, CREATE `tools/capture-fixtures.sh` doing curl → jq passthrough with NO play-array truncation and NO top-level key stripping)
- Create: `fixtures/live/` — new live-state fixtures (see Step 2)
- Test: `tests/map_espn.rs` additions

**Interfaces:**
- Produces: `fixtures/live/<league>_scoreboard_live.json` (with `competition.situation` present) for every league reachable live during implementation; `fixtures/live/mlb_summary_live_full.json` (untruncated — all `P` rows, the §4 join needs them); `fixtures/live/epl_scoreboard_redcard.json` (from the research cache: scratchpad `sub4-research/live/` holds an EPL slate with a red card — copy it in and record provenance in a comment at the top-level `_provenance` key or a sibling `.md`); NHL untruncated summary if reachable, else the research cache's copy marked as such.

- [ ] **Step 1: Write the failing test** — a fixture-teeth test:

```rust
// tests/map_espn.rs
#[test]
fn live_fixtures_carry_live_state() {
    // For each fixtures/live/*_scoreboard_live.json: at least one event has
    // status.type.state == "in" AND competitions[0].situation present.
    // The mapped Game for that event has a non-empty situation
    // (situation.summary()/fields per league). This is the regression net
    // spec §1 says does not exist today.
}
#[test]
fn the_mlb_live_summary_is_untruncated() {
    // fixtures/live/mlb_summary_live_full.json: plays[].len() > 100
    // (research: live games carry 300-540; the old capture bug trimmed to
    // exactly 80 — receipt) and at least one play has summaryType == "P".
}
```

- [ ] **Step 2: Run to verify failure** — files absent.
- [ ] **Step 3: Fix the capture tooling** (whatever form it takes) so a capture keeps full arrays + all keys; add a header comment naming the v3.1-era 80-play/key-strip capture bug it fixes. Capture the live fixtures: check `curl -s 'https://site.web.api.espn.com/apis/site/v2/sports/<sport>/<league>/scoreboard' | jq '[.events[].status.type.state]'` per league (URL patterns in src/provider/espn.rs); capture every league with an `"in"` game (MLB/CFB expected; NFL if week 1 started), plus the red-card EPL slate from the research cache. One fetch per endpooint, gently.
- [ ] **Step 4: Run tests** — PASS. Full suite + clippy.
- [ ] **Step 5: Commit**

```bash
git add tools fixtures tests
git commit -m "feat(v3.4): fixture truth — capture keeps everything, live-state fixtures land"
```

---

### Task 2: `PlayKind` — the enum and the six id tables (spec §2)

**Files:**
- Create: `src/provider/kinds.rs` (`pub mod kinds;` in src/provider/mod.rs)
- Modify: `src/domain.rs` (Play gains fields)
- Test: `src/provider/kinds.rs`

**Interfaces:**
- Produces (exact):

```rust
// domain.rs — Play gains (existing fields unchanged):
pub kind: PlayKind,            // default PlayKind::Other for legacy constructors
pub score_value: Option<u8>,   // MLB/NBA/WNBA/CBB/NHL plays + live lastPlay

// domain.rs:
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlayKind {
    Touchdown, FieldGoal, Safety,          // football (scoringType.name)
    HomeRun, RunScoringPlay,               // MLB (kind 28; any other pitch-kind with score_value > 0)
    Goal, OwnGoal, PenaltyGoal,            // soccer + NHL goal → Goal
    YellowCard, RedCard,                   // soccer
    HockeyPenalty,                         // NHL penalty plays (meta in Extras::Hockey, Task 7)
    ThreePointer,                          // hoops, DERIVED: shootingPlay && score_value == Some(3)
    #[default] Other,                      // everything unmapped — id retained on the play? NO:
}                                          // ids are map-time only; Other carries no payload (YAGNI —
                                           // no consumer reads raw ids; the mapper logs nothing).

// provider/kinds.rs — pure lookup fns, one per table (receipts = research findings):
pub fn football_kind(type_id: &str, scoring_type: Option<&str>) -> PlayKind;   // NFL+CFB shared; scoringType.name wins for scoring plays: "touchdown"→Touchdown, "field-goal"→FieldGoal, "safety"→Safety
pub fn hoops_kind(type_id: &str, shooting: bool, score_value: Option<u8>, cbb: bool) -> PlayKind; // NBA/WNBA table vs CBB table (different ids AND spellings — receipt); ThreePointer derived
pub fn mlb_kind(pitch_type_id: &str, score_value: Option<u8>) -> PlayKind;     // 28→HomeRun; else score_value>0→RunScoringPlay; else Other
pub fn nhl_kind(type_id: &str) -> PlayKind;                                    // 505→Goal; penalty-range ids (research: e.g. 29, 55, carrying type.penaltyMinutes)→HockeyPenalty; else Other
pub fn soccer_kind(type_id: &str) -> PlayKind;                                 // 70/137/173→Goal, 97→OwnGoal, 98→PenaltyGoal, 94→YellowCard, 93→RedCard; else Other
```

- [ ] **Step 1: Write the failing tests** — table-driven, exact ids from the research:

```rust
#[test] fn football_scoring_types_win() {
    assert_eq!(football_kind("67", Some("touchdown")), PlayKind::Touchdown);
    assert_eq!(football_kind("59", Some("field-goal")), PlayKind::FieldGoal);
    assert_eq!(football_kind("5", None), PlayKind::Other); // plain rush
}
#[test] fn cbb_ids_are_not_nba_ids() {
    // NBA 92 Jump Shot w/ 3 pts → ThreePointer; CBB uses 558 JumpShot:
    assert_eq!(hoops_kind("92", true, Some(3), false), PlayKind::ThreePointer);
    assert_eq!(hoops_kind("558", true, Some(3), true), PlayKind::ThreePointer);
    assert_eq!(hoops_kind("92", true, Some(3), true), PlayKind::Other); // NBA id through CBB table stays honest
}
#[test] fn mlb_kind_is_homer_or_scoring_or_other() {
    assert_eq!(mlb_kind("28", Some(1)), PlayKind::HomeRun);
    assert_eq!(mlb_kind("35", Some(1)), PlayKind::RunScoringPlay); // sac fly
    assert_eq!(mlb_kind("22", None), PlayKind::Other);             // fly out
}
#[test] fn soccer_and_nhl_tables() {
    assert_eq!(soccer_kind("93"), PlayKind::RedCard);
    assert_eq!(soccer_kind("97"), PlayKind::OwnGoal);
    assert_eq!(nhl_kind("505"), PlayKind::Goal);
    assert_eq!(nhl_kind("29"), PlayKind::HockeyPenalty);
    assert_eq!(soccer_kind("9999"), PlayKind::Other);
}
```

- [ ] **Step 2: Run to verify failure** — module absent.
- [ ] **Step 3: Implement** — tables as `match` arms, one line per id with the play-text receipt comment (`// 67 Passing Touchdown (research §1)`). Every existing `Play { .. }` constructor gains `kind: PlayKind::Other, score_value: None` via `#[derive(Default)]`-friendly update or explicit fields — compile-fix sweep, no behavior change yet.
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/domain.rs src/provider tests
git commit -m "feat(v3.4): PlayKind and the six id tables — structural kinds, receipts per id"
```

---

### Task 3: Mapper wiring — flat-play leagues, football drives, soccer (spec §2)

**Files:**
- Modify: `src/provider/map.rs` (`map_summary` play mapping for NBA/WNBA/CBB/NHL; the football drives path; soccer `details_from` + summary `keyEvents`)
- Test: `tests/map_espn.rs` (fixture-driven)

**Interfaces:**
- Consumes: Task 2's `kinds::*` fns.
- Produces: every mapped `Play` carries a real `kind` + `score_value` in these leagues; soccer summary keyEvents switch on `type.id` (their booleans are null — research §1 soccer note).

- [ ] **Step 1: Failing tests** (fixtures name real games):

```rust
#[test] fn nfl_summary_plays_carry_kinds() {
    // fixtures/nfl_summary_full.json → the mapped game's scoring plays include
    // at least one PlayKind::Touchdown and one PlayKind::FieldGoal; a non-scoring
    // play is Other.
}
#[test] fn nhl_goals_and_penalties_are_kinds() {
    // nhl fixture: a 505 play maps Goal; a penalty play maps HockeyPenalty.
}
#[test] fn cbb_uses_its_own_table() {
    // cbb fixture: a made three (558/shooting/scoreValue 3) → ThreePointer.
}
#[test] fn soccer_summary_events_come_from_type_ids() {
    // epl_summary_full: keyEvents map to kinds via type.id (booleans are null there);
    // a goal event → Goal, a card → YellowCard/RedCard.
}
```

- [ ] **Step 2: Run to verify failure** — kinds are all `Other` today.
- [ ] **Step 3: Implement.** Each league's play loop passes `type.id` (+ `scoringType.name` for football, `shootingPlay`/`scoreValue` for hoops) through the right table. The MLB path is NOT touched here (Task 4 owns the join). Caution from the research: the flat-play mapper's allow-by-default filter passes `summaryType: null` rows — while here, make the filter's intent explicit with a comment; do not change its behavior in this task.
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/provider/map.rs tests
git commit -m "feat(v3.4): mapped kinds — flat leagues, football drives, soccer by type.id"
```

---

### Task 4: MLB — the at-bat join (spec §2)

**Files:**
- Modify: `src/provider/map.rs` (the MLB summary play filter restructures)
- Test: `tests/map_espn.rs` (against Task 1's untruncated live MLB fixture)

**Interfaces:**
- Consumes: `kinds::mlb_kind`, `fixtures/live/mlb_summary_live_full.json`.
- Produces: MLB narrative plays (`57 Play Result`) carry the kind lifted from their at-bat's pitch row via `atBatId`. The join is map-internal — **plan-level deviation, recorded**: the spec names `Play.at_bat_id` as a field, but no consumer exists beyond the join itself, so the field does not land (YAGNI); the spec's intent (the join) is served. The executor ledgers this deviation at pre-flight.

- [ ] **Step 1: Failing test:**

```rust
#[test] fn the_home_run_kind_rides_the_narrative_play() {
    // fixtures/live/mlb_summary_live_full.json: find an at-bat whose pitch row
    // is type 28 (Home Run); assert the mapped play list contains its narrative
    // sibling ("... homered ...") with kind == PlayKind::HomeRun and the pitch
    // rows themselves still filtered out (play count unchanged from today's
    // mapping minus zero — pin the mapped count).
}
#[test] fn a_scoring_non_homer_is_run_scoring_play() {
    // A sac-fly or RBI-single at-bat in the fixture → RunScoringPlay on its
    // narrative play.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** — two-pass over the raw plays: pass 1 builds `atBatId → (pitch kind, score_value)` from `P` rows (`28 Home Run` wins over later pitch kinds in the same at-bat); pass 2 maps narrative rows as today, attaching the joined kind. The existing "drop P rows" behavior survives as pass-2's filter.
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/provider/map.rs tests
git commit -m "feat(v3.4): MLB at-bat join — the kind rides the narrative play"
```

---

### Task 5: Scoreboard situation truth — lastPlay kinds, integer downs, honest red zone (spec §2, §3)

**Files:**
- Modify: `src/provider/map.rs` (`map_event`: `situation.lastPlay` keeps type/scoreValue; `situation` integers; `redzone_from` dies), `src/domain.rs` (Situation gains fields)
- Test: `tests/map_espn.rs` (against Task 1's live scoreboard fixtures)

**Interfaces:**
- Produces (exact):

```rust
// domain.rs — Situation gains:
pub down: Option<u8>, pub distance: Option<u8>,
pub yard_line: Option<u8>,       // absolute 0-100, ESPN's yardLine
pub is_red_zone: Option<bool>,   // ESPN's isRedZone — the RED ZONE source now
pub drive_desc: Option<String>,  // "1 play, 3 yards, 0:08" — live scoreboard carries it
```

- The board's last-play line gains kind awareness: `map_event`'s lastPlay becomes a real `Play` with `kind` + `score_value` (via the football table; other leagues' lastPlay ids pass through their tables).
- `redzone_from`'s `rsplit_once(' ')` parse is DELETED; the RED ZONE chip/meter read `is_red_zone`/`yard_line`. Fallback: when `is_red_zone` is absent (non-live or non-football), the chip simply doesn't fire from situation (existing kind-of-quiet behavior preserved — pin it).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn live_situation_maps_integers_not_strings() {
    // fixtures/live/cfb_scoreboard_live.json: the in-game event maps
    // down == Some(2), distance == Some(7)-style integers (use the fixture's
    // real values), yard_line == Some(n), is_red_zone == Some(_),
    // drive_desc.is_some().
}
#[test] fn the_red_zone_chip_reads_the_payload_not_the_text() {
    // Build a Game with is_red_zone: Some(true) and a possessionText that
    // would have FAILED the old rsplit parse ("weird &format 3") — chip fires.
    // And is_red_zone: Some(false) with text that would have TRICKED the old
    // parse — chip does not fire.
}
#[test] fn last_play_carries_its_kind_at_scoreboard_cadence() {
    // live fixture's lastPlay with a typed play → mapped Play.kind != Other
    // where the table knows the id.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Delete `redzone_from` and its callers' string plumbing in the same commit (structure-over-parsing rule).
- [ ] **Step 4: Full suite + clippy** (board tests asserting the old parse update honestly, `// spec v3.4 §3` comments).
- [ ] **Step 5: Commit**

```bash
git add src/domain.rs src/provider/map.rs src tests
git commit -m "feat(v3.4): situation integers and kind-aware lastPlay; the red-zone text parse dies"
```

---

### Task 6: Consumers convert — scoring words and rank read kinds; text paths die (spec §2)

**Files:**
- Modify: `src/theme.rs` (`scoring_word_for_play` → kind match), `src/rank.rs` (any play-text reads → kind), `src/board/cut.rs` (word selection call site)
- Test: existing scoring-word/cut/rank tests rewritten

**Interfaces:**
- Consumes: `Play.kind` everywhere plays flow (cut fire sites pass the Play already).
- Produces: `theme::scoring_word_for_play(league, play: &Play) -> &'static str` (signature takes the play, not text) — Touchdown→TOUCHDOWN, FieldGoal→FIELD GOAL, Safety→SAFETY, HomeRun→HOME RUN, RunScoringPlay→RUN SCORES, Goal/PenaltyGoal→GOAL, OwnGoal→GOAL (the scorer's misfortune is the detail line's job), ThreePointer→BUCKET (word set unchanged from v3.3 §7 — no `!`), kind Other→the league word (existing fallback).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn the_blocked_fg_returned_for_td_is_now_trivially_right() {
    // A Play { kind: Touchdown, text: "Blocked Field Goal returned ... TOUCHDOWN" }
    // → "TOUCHDOWN". The v3.3 R34 text-priority dance is gone.
}
#[test] fn mlb_words_come_from_kinds() {
    // kind HomeRun → "HOME RUN"; kind RunScoringPlay (a bases-loaded walk text) → "RUN SCORES".
}
#[test] fn no_text_matching_survives_in_scoring_words() {
    // Compile-level: the fn body has no .contains/.to_lowercase on play text —
    // enforced by deleting the old helpers; grep-style test optional, honest
    // deletion + clippy dead-code is the real gate.
}
```

- [ ] **Step 2: Run to verify failure** (signature change → compile errors).
- [ ] **Step 3: Implement**; delete the text-matching helpers (`// spec v3.4 §2: structure over parsing` at the deletion site). Rank: grep `\.text` in src/rank.rs — convert any semantic read to kind; display-only reads stay.
- [ ] **Step 4: Full suite + clippy** (cut/word tests update honestly with spec comments; live-capture-derived tests from v3.3 keep their wire texts as Other-kind fallbacks where the fixture lacks types).
- [ ] **Step 5: Commit**

```bash
git add src/theme.rs src/rank.rs src/board/cut.rs tests
git commit -m "feat(v3.4): scoring words read kinds; the text-parsing era ends"
```

---

### Task 7: NHL — Extras::Hockey, the penalty meter, zoom PP chip (spec §4)

**Files:**
- Modify: `src/domain.rs` (Extras variant), `src/provider/map.rs` (map_summary NHL path), `src/views/zoom.rs` (PP chip + penalty meter), `src/tiles/mod.rs` only if `meter_line` needs the Penalty arm wired
- Test: `tests/map_espn.rs`, `tests/draw.rs`

**Interfaces:**
- Produces (exact):

```rust
// domain.rs:
pub enum HockeyStrength { Even, PowerPlay, Shorthanded, EmptyNet } // 701/702/703/903 (research §2)
pub struct PenaltyEvent { pub team: String, pub minutes: u8, pub kind: String, // "Minor"/"Major"
                          pub period: u8, pub clock: String }
// Extras gains:
Extras::Hockey { strength: HockeyStrength,          // strength of the most recent play
                 penalties: Vec<PenaltyEvent> }
```

- The `"PP · "` string prefix collapse (map.rs ~L594-597) is DELETED — strength is structural.
- Zoom (NHL zoomed game): the situation chip shows `POWER PLAY`/`SHORTHANDED` from `strength` (existing POWER PLAY chip styling — v3.3 §7 caps survivor); `Meter::Penalty { team_abbr, seconds }` (domain.rs:117, never constructed since v1) is built from the newest active penalty (seconds = penalty minutes × 60 minus elapsed since its clock — if elapsed math needs game-clock deltas the fixture can't support, ship the static `minutes × 60` display with a receipt saying why, honestly).
- The BOARD chip is NOT built (spec §4 defers it to the October probe); the mapping is scoreboard-independent so promotion later is additive.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn nhl_strength_is_structural_and_current() {
    // NHL summary fixture: Extras::Hockey.strength equals the last play's
    // strength id mapping; a 702 tail → PowerPlay.
}
#[test] fn penalties_carry_their_metadata() {
    // A penalty play → PenaltyEvent { minutes: 2, kind: "Minor", team, period, clock }.
}
#[test] fn the_zoom_shows_the_penalty_meter_and_pp_chip() {
    // Zoomed NHL game with a PowerPlay strength: "POWER PLAY" chip cell-asserted;
    // the meter row renders the Penalty variant (cell-level on the label + team).
}
#[test] fn the_pp_string_prefix_is_gone() {
    // No mapped play text begins "PP · " for the fixture that used to produce it —
    // updated legacy assertion with // spec v3.4 §4 comment.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/domain.rs src/provider/map.rs src/views/zoom.rs src/tiles tests
git commit -m "feat(v3.4): NHL strength structural — Meter::Penalty lives, zoom PP chip, string prefix dead"
```

---

### Task 8: Soccer — cards that count (spec §5, §8)

**Files:**
- Modify: `src/domain.rs` (soccer event athlete id; derived men), `src/provider/map.rs` (`details_from` + map_event derivation), `src/rank.rs` (red-card bonus), `src/app/mod.rs` (fingerprint set), `src/board/rows.rs` or hero chip site (the `10 MEN` chip)
- Test: `tests/map_espn.rs`, `tests/draw.rs`, rank tests

**Interfaces:**
- Produces (exact):

```rust
// domain.rs — soccer MatchEvent gains: pub athlete_id: Option<String>,
// Extras::Soccer gains: pub men: Option<(u8, u8)>,   // (away, home) on-field counts, None when 11v11
```

- Derivation (map_event, scoreboard `details[]` alone — research §3): `reds(team) = count(redCard == true) + count(athlete_ids with ≥2 yellowCard events)`; `men = 11 - reds` per side; `None` unless some side < 11. The defensive second-yellow rule is the receipt (ESPN's encoding unobserved).
- Board surface: a `10 MEN` chip (or `9 MEN` etc. — `{n} MEN`) on the short-handed side's row/hero chip slot, `roles.hot` styling (a red card is hot by definition).
- Rank: `bonus!` red-card entry in `watchability` (value: same tier as CLUTCH-class bonuses — read the existing bonus table and slot it with a receipt comparing against the PP bonus the spec family planned); the fingerprint tuple in `src/app/mod.rs` gains the per-game red-count so the card fires exactly one reorder (R24).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn a_red_card_yields_ten_men_from_the_scoreboard_alone() {
    // fixtures/live/epl_scoreboard_redcard.json → the mapped game's
    // Extras::Soccer.men == Some((10, 11)) (side per the fixture's card).
}
#[test] fn two_yellows_on_one_athlete_count_as_a_red() {
    // Synthesized details: same athlete_id, two yellowCard events, no explicit
    // red → men == Some((10, 11)).
}
#[test] fn the_ten_men_chip_renders_hot() {
    // Board row for that game: "10 MEN" cells in roles.hot (cell-level).
}
#[test] fn a_red_card_reorders_once_and_freezes() {
    // R24 wiring: apply with the card → one reorder (nudge visible); re-apply
    // identical payload → no reorder (fingerprint unchanged).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/domain.rs src/provider/map.rs src/rank.rs src/app src/board tests
git commit -m "feat(v3.4): soccer cards — ten-men chip board-wide, red card ranks, one honest reorder"
```

---

### Task 9: Finals get their story (spec §6)

**Files:**
- Modify: `src/domain.rs` (Game gains headline), `src/provider/map.rs` (map_event reads headlines), `src/board/rows.rs` (tier-3 FINAL headline source ladder), `src/views/zoom.rs` (zoomed final header)
- Test: `tests/map_espn.rs`, `tests/draw.rs`

**Interfaces:**
- Produces: `Game.headline: Option<String>` from `competitions[0].headlines[0].shortLinkText` (never `description` — em-dash wire copy, research §4). Tier-3 FINAL ladder becomes: headline → newest scoring play text → leaders line → existing fallback (order per spec §6 with leaders and scoring-play swapped? NO — spec order: shortLinkText → leaders → newest scoring play; use the spec's order verbatim).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn finals_headline_from_short_link_text() {
    // nfl scoreboard fixture: a final event with headlines → Game.headline
    // == Some(the fixture's shortLinkText); an event without → None.
}
#[test] fn the_final_row_prefers_the_headline() {
    // Tier-3 FINAL with headline → row shows it; without → leaders line;
    // without either → newest scoring play (existing behavior pinned).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** (MLS's 0% headline coverage exercises the ladder naturally — note in a test comment).
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit**

```bash
git add src/domain.rs src/provider/map.rs src/board/rows.rs src/views/zoom.rs tests
git commit -m "feat(v3.4): startup finals headline from the scoreboard's own story"
```

---

### Task 10: Logo pipeline v2 — rel-tag URLs, pro completion, college top-25 (spec §7)

**Files:**
- Modify: `tools/gen-logos.sh` (rewrite: teams-endpoint `logos[]` rel-tag resolution instead of the `$league/500/$abbr` guess), `src/provider/map.rs` (`Team.logo_key` carries the ESPN id for ncaa/soccer leagues: `"ncaa/<id>"`, `"soccer/<id>"`; pro keys unchanged), `src/board/logo.rs` (art loading for the new keys)
- Create: `assets/logos/**` new marks (157 pro + college top-25)
- Test: `tests/logo.rs`, `tests/map_espn.rs`

**Interfaces:**
- Produces: complete pro-league marks (NHL 32, NBA 30, MLB 30, MLS 30, EPL 20, WNBA 15 — counts are the research's receipts) + CFB/CBB `curatedRank` top-25 marks; everyone uncovered keeps the color-block fallback (existing behavior, pin it). The codepoint guard test scans ALL marks including new ones (it already scans the bundle — verify it's count-agnostic, fix if it hardcodes 38). An asset-weight receipt (before/after `du -sh assets/logos`) lands in the commit body.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn college_and_soccer_logo_keys_are_id_keyed() {
    // Mapped CFB team → logo_key "ncaa/<id>"; EPL team → "soccer/<id>";
    // NFL stays "nfl/<abbr>" (fixture-driven).
}
#[test] fn every_bundled_mark_is_quadrant_only() { /* the existing guard, now count-agnostic */ }
#[test] fn a_covered_pro_team_resolves_a_mark_and_an_uncovered_college_team_falls_back() {
    // hero_mark("nba/bos").is_some(); hero_mark("ncaa/999999").is_none() → color-block path.
}
```

- [ ] **Step 2: Run to verify failure** (key scheme + marks absent).
- [ ] **Step 3: Implement + generate.** gen-logos.sh v2: per league, fetch `/teams?limit=1000`, resolve `logos[]` `rel:["full","default"]` href, chafa with the v3.3 quadrant symbols (`SYMBOLS="space+solid+half+quad"` — the guard enforces), name by logo_key. College: filter to `curatedRank <= 25` where present. Document the EPL-churn and poll-refresh one-liners in the script header.
- [ ] **Step 4: Full suite + clippy; regen board-broadcast and eyeball two new-league marks (NBA, EPL) in the PNG.**
- [ ] **Step 5: Commit** (asset-weight receipt in body)

```bash
git add tools/gen-logos.sh src assets tests
git commit -m "feat(v3.4): logo pipeline v2 — rel-tag URLs, 157 pro marks, college top-25, id-keyed ncaa/soccer"
```

---

### Task 11: The light set — daygame's art (spec §7)

**Files:**
- Modify: `tools/gen-logos.sh` (light-set mode), `src/board/logo.rs` (theme-aware set selection: dark ground → standard set, light ground → light set; smallest honest mechanism — a `roles().ground` luminance check with receipt)
- Create: `assets/logos-light/**`
- Test: `tests/logo.rs` + a theme-aware render test

**Interfaces:**
- Produces: a parallel light-background mark set composited on daygame's ground color; where a mark fails a contrast check against the light ground (relative-luminance delta < the WCAG-ish 3:1 for graphics, receipt), the pipeline substitutes ESPN's `primary_logo_on_white_color` variant, logged per-team in the script output. The v3.3 dark-mark brightness lift inverts for the light set. `hero_mark` resolution becomes ground-aware; dark themes see byte-identical behavior (pin with the existing mark tests).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn light_ground_selects_the_light_set() {
    // With a light-ground theme active, hero_mark resolves from logos-light/;
    // with broadcast, from logos/ (existing tests keep passing untouched).
}
#[test] fn light_marks_are_quadrant_only_too() { /* guard extends to logos-light */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement + generate the light set (pro + college-25, same coverage).**
- [ ] **Step 4: Full suite + clippy. Render the gate frame: `cargo run --release -- frame --view board --theme <daygame roles via user-TOML export or the test constructor's dump hook — the v3.3 frame tool accepts a user theme file: write daygame's roles to a scratch TOML> --size 120x36 --out out/design/daygame-real-marks.png` and Read it — the marks must sit clean on the light ground (the Sitting-2 black-box defect visibly dead). Report what you see.**
- [ ] **Step 5: Commit** (weight receipt in body)

```bash
git add tools/gen-logos.sh src/board/logo.rs assets tests
git commit -m "feat(v3.4): the light mark set — daygame's blocker dissolved, ground-aware resolution"
```

---

### Task 12: RENDER GATE — daygame promotion (controller checkpoint, spec §7/§9)

**This is NOT a subagent task.** The controller:

- [ ] **Step 1:** Open `out/design/daygame-real-marks.png` (regenerate beside `out/board-broadcast.png` for contrast) for Walter with the question: promote daygame to BUILTIN_NAMES, or keep parked (light set ships either way)?
- [ ] **Step 2 (apply, one subagent after the decision):** if promoted — daygame enters BUILTIN_NAMES + theme-picker row + README themes paragraph + the v3.3 park comment updated; if kept parked — the park comment gains "art unblocked, look declined <date>" honesty. Small commit either way.

```bash
git commit -m "feat(v3.4): daygame gate decision applied"
```

---

### Task 13: DoD sweep (spec §9)

**Files:** receipts appended to the spec; README; no new code beyond receipts.

- [ ] **Step 1:** Full `cargo test` (default parallelism) + `cargo clippy --all-targets` — record verbatim.
- [ ] **Step 2: Live-window receipts.** During a live window (MLB/CFB now): run the real app (tmux, scratch --config-dir, 120×40), capture kind-driven behavior on real plays — a structurally-chosen scoring word if one fires; the `10 MEN` chip if a card happens (else "not fired — honest"); a finals headline on the board. Save captures to the workspace; KILL everything started.
- [ ] **Step 3: Fixture-teeth mutation receipt** (spec §9): break one live-state mapping (e.g. invert `is_red_zone`) → name the test that fails → revert. Recorded in the spec receipts.
- [ ] **Step 4: CPU** — `--demo` 30s idle sampled the v3.3 way; compare v3.3's ~0.0% receipt; record.
- [ ] **Step 5: The October probe** documented in the spec's Verification section verbatim (the curl+jq one-liner from the research §2) marked OPEN — a follow-up, not DoD.
- [ ] **Step 6:** README pass: logo coverage note (pro complete, college top-25 + fallback), themes paragraph matching the gate outcome. Append `## Verification (date)` to the spec; commit.

```bash
git add docs README.md
git commit -m "docs(v3.4): verification receipts — tests, clippy, live window, fixture teeth, CPU; October probe open"
```

---

## Self-Review

**Spec coverage:** §1→T1; §2→T2 (enum/tables), T3 (flat+football+soccer wiring), T4 (MLB join), T5 (lastPlay kind at 15s), T6 (consumers+parser deletion); §3→T5; §4→T7 (board chip explicitly deferred — spec-faithful); §5→T8; §6→T9; §7→T10+T11+T12 gate; §8→T8's fingerprint/wiring test (NHL entry deferred with §4); §9→T13; §10 respected (no new requests anywhere; T7/T9 read existing payloads). One recorded plan-level deviation: `Play.at_bat_id` stays map-internal (T4 interface note) — executor ledgers it at pre-flight.

**Placeholder scan:** clean — every step names files, exact ids, signatures, or the grep that locates them; UI test outlines follow the R22 convention.

**Type consistency:** `PlayKind` variants used in T3/T4/T5/T6/T7/T8 match T2's enum; `kinds::*` signatures consumed as declared; `HockeyStrength`/`PenaltyEvent`/`Extras::Hockey` (T7), `MatchEvent.athlete_id`/`Extras::Soccer.men` (T8), `Game.headline` (T9), `Situation` fields (T5), logo_key scheme (T10) each declared once and consumed by name. T12 is a controller checkpoint — the subagent-driven executor must not dispatch it as an implementer task.
