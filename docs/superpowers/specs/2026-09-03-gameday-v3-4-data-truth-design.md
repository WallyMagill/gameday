# gameday v3.4 — Data truth (sub-project 4)

Date: 2026-09-03. Status: draft for Walter's review.
Predecessor: v3.3 Polish (merged to `redzone-draw`; spec `2026-09-03-gameday-v3-3-polish-design.md`).
Evidence: ESPN payload research, 2026-09-03/04 — fixtures + live probes across all nine leagues. Full findings restated where binding; the working file is `.superpowers/sdd/2026-09-03-sub4-research/findings.md` (gitignored).

Direction (Walter, 2026-09-03): logo scope **A + daygame** — pro leagues complete, college ranked-only with color-block fallback, and the light-background art set that promotes the parked daygame theme. Reopens if college coverage feels thin on real boards.

## §0 Goal and principles

v3.1–v3.3 made the app honest and handsome on the data it had. This sub-project widens the data: every place gameday currently *parses prose or guesses* gets the structured field ESPN actually publishes, and every place it says "no data" for something ESPN carries gets the real thing.

Binding principles:
- **Structure over parsing.** Where a typed field exists, the text-parse dies. No new text heuristics ship for anything this spec covers.
- **No new request load.** Everything lands from the payloads we already fetch (15s scoreboards; summaries for the zoomed game only). Never fetch a summary slate; never add per-game polls. A feature that would need one is out of scope by definition.
- **The v3.2/v3.3 hard rules stand**: one score formatter; R24 (no reorder without a real event — new chips/bonuses enter the fingerprint set deliberately, not accidentally); logos never move a digit; quadrant-only glyph ranges (the mark codepoint guard extends to every new art file).
- **Data claims get fixtures.** Any behavior derived from live-game state gets a live-state fixture that can catch its regression (see §1 — today none can).

## §1 Fixture truth (prerequisite wave)

The 29 committed fixtures are all FINAL/all-`post` — `competition.situation` exists only on live events, so no current test can regress-catch any situation-derived feature. Summaries were captured truncated (exactly 80 plays — the live same-games carry 540/480/306) with top-level keys stripped (NHL lost `onIce`).

Ships first:
- **Capture tooling fixed**: the fixture capture path keeps full play arrays and all top-level keys (the 80-play trim and key-strip die; a comment records what happened in v3.1's captures).
- **Live-state fixtures** captured for whatever is live during implementation (MLB and CFB are live now; NFL week 1 imminent): at least one live scoreboard per reachable league with `situation` present, one untruncated MLB summary (the `P` rows carry §2's kinds), one untruncated NHL summary when reachable (else the committed live probe cache seeds it, marked as such).
- **A soccer slate containing a red card** (one exists in the research cache from EPL 08-23). Second-yellow remains unobserved — §5 handles it defensively.
- Fixture-dependent tests in later sections cite these files.

## §2 `Play.kind` — structural in all nine leagues

ESPN carries `type.{id,text}` on every play in every league. The model gains:

```rust
// domain.rs — Play grows:
pub kind: PlayKind,          // structural; Unknown(id) preserves unmapped ids
pub score_value: Option<u8>, // present MLB/NBA/WNBA/CBB/NHL + live lastPlay
```

`PlayKind` is one enum with league-appropriate variants (Touchdown, FieldGoal, Safety, HomeRun, SacFly, RunScoringPlay, Goal, OwnGoal, PenaltyGoal, Dunk, ThreePointer {derived}, PenaltyMinor/Major, …, Other) mapped from **six id tables**: NFL/CFB (shared namespace, plus the independent `scoringType.name` enum for scoring plays), NBA/WNBA (shared), CBB (different ids AND spellings — its own table), NHL, MLB, soccer. Tables live beside the mapper with the verified ids from the research as receipts; unmapped ids map to `Other` with the id retained — never a panic, never a guess.

League specifics:
- **MLB**: the kind sits on the pitch row (`type 28 Home Run`, `35 Sacrifice Fly`), the prose on the narrative row (`57 Play Result`), joined by `atBatId`. `map_summary`'s filter restructures from "drop P rows" to "lift the P-row's type onto the at-bat's narrative play first, then drop". `Play.at_bat_id` lands to support it. RBI = kind + `score_value > 0` (derived, not read).
- **Three-pointer** = `shootingPlay && score_value == 3` (NBA has no distinct type id for threes).
- **Scoreboard `situation.lastPlay`** carries `type` + `scoreValue` at the 15-second cadence — `map_event` keeps them (today it drops everything but text/clock/team), so board-visible last plays are kind-aware without touching a summary.
- **Consumers converted, parsers deleted**: `theme::scoring_word_for_play`'s text-matching (v3.3 R34/I3 lineage) becomes a `PlayKind` match — FIELD GOAL, SAFETY, HOME RUN, RUN SCORES chosen structurally; the blocked-FG-returned-for-TD case becomes trivially correct. `cut::split_surname` keeps its display job but stops inferring meaning. Watchability's play-driven signals read kind where they read text today. The old text paths are deleted, not shadowed.

## §3 Situation truth (football)

Live scoreboards carry integers the model currently re-derives from strings:
- `Situation` gains `down: Option<u8>`, `distance: Option<u8>`, `yard_line: Option<u8>` (absolute), `is_red_zone: Option<bool>` from the payload's own fields. The composed `down_distance` string stays for display.
- `redzone_from`'s `rsplit_once(' ')` text parse (map.rs ~L143) dies — `isRedZone` + `yardLine` are the source. The RED ZONE chip and field meter consume them.
- `drive.description` ("1 play, 3 yards, 0:08") is on the live scoreboard (the mapper's summary-only comment is wrong) — mapped into the football situation and available to tier-1/hero fragment lines.

## §4 NHL — the strength of the game

- **`Extras::Hockey` lands**: per-play `strength` (Even/PowerPlay/Shorthanded/EmptyNet — today collapsed into a `"PP · "` string prefix that destroys the structure), penalty metadata (`penaltyMinutes`, `penaltyType`, committing team, period, clock).
- **`Meter::Penalty { team_abbr, seconds }`** — the variant that has existed unconstructed since v1 — is finally built from penalty plays for the zoomed game: the zoom (and TV, same hero path) shows the running penalty-kill meter.
- **Zoom PP chip**: current man-advantage = strength of the most recent summary play; rendered as the situation chip for the zoomed NHL game. Structural, replacing the string prefix.
- **The board chip is explicitly deferred**: whether scoreboards carry PP state is unverifiable until NHL goes live (October). The chip's plumbing is written so a positive October probe promotes it additively (one mapping + one fingerprint entry). The probe command is documented in the spec's Verification section as an open receipt — it does NOT block this sub-project's DoD.

## §5 Soccer — cards that count

- `Extras::Soccer` events gain `athlete_id` (mapped from `athletesInvolved[].id` — required for second-yellow detection).
- **`men_on_field` derived board-wide** from scoreboard `details[]` alone: `11 − reds`, where reds = explicit `redCard == true` (type 93) **plus** any athlete with two yellows (defensive: ESPN's second-yellow encoding is unobserved; this rule is correct under every plausible encoding).
- Board surfaces: a `10 MEN` (or `9 MEN`) chip on the row/hero for the short-handed side, and **the red card joins watchability** as an event bonus (the v3.2-deferred item) — entering the fingerprint set so it fires exactly one honest reorder when it happens (R24 discipline).
- Summary keyEvents switch on `type.id` (their booleans are null) so zoom's match-event list gets card letters from structure, not text.

## §6 Finals get their story

- Startup finals (loaded already-final, no delta ever captured) headline from the scoreboard's own `headlines[0].shortLinkText` — a complete one-line game story, ~95–100% coverage in 8/9 leagues.
- Ladder, in order: `shortLinkText` → leaders line (already mapped, 5/9 leagues) → newest scoring play (existing behavior). MLS (0% headlines) lands on the ladder's lower rungs honestly.
- `description` (em-dash wire copy) is never used. Summaries are never fetched for finals (400–900KB each; the scoreboard field is free).
- FINAL tier-3 rows and the zoomed final's header consume it.

## §7 Logos and daygame (scope A + daygame)

**Pipeline v2** (`tools/gen-logos.sh` rewrite):
- URL resolution moves from the `$league/500/$abbr` guess to the teams endpoint's **`logos[]` rel tags** — which also fixes college/soccer (id-keyed under `ncaa/500/<id>.png`, `soccer/500/<id>.png`; `Team.logo_key` carries the id for those leagues).
- **Pro leagues complete**: NHL 32, NBA 30, MLB 30, MLS 30, EPL 20, WNBA 15 → 157 new marks beside NFL's 32. One run; EPL churn is a one-command annual refresh, documented.
- **College ranked-only**: CFB/CBB marks generated for `curatedRank` top-25 teams (the payload marks them); everyone else keeps the color-block fallback. Regeneration refreshes with the polls; documented.
- **The light set for daygame**: same run emits a second set composited on the daygame ground color (source marks are true-alpha; ESPN also publishes `primary_logo_on_white_color` for contrast-hostile marks — the pipeline uses it where the standard mark fails a contrast check against the light ground, receipt per substitution). The v3.3 dark-mark brightness lift inverts for the light set.
- **Guards**: the codepoint guard (quadrant-range only) extends to every new mark file; a size-budget receipt records the asset weight delta (189 + college-25s + light set — estimate and record, cap nothing yet).
- **daygame promotes** to BUILTIN_NAMES behind one final render gate: `gameday frame --view board --theme daygame` with real light marks — Walter eyeballs the frame (the v3.3 parking reason was exactly this art; the gate confirms it's dissolved). If the frame disappoints, daygame stays parked and the light set still ships (user themes can use it).

## §8 Ranking integration, gated

New signals enter watchability only where this spec names them: the soccer red card (§5) and — post-October-probe only — NHL PP. Each addition: a `bonus!` entry with a receipt, a fingerprint-set entry so it fires real events (R24), and a wiring test proving one event → one reorder → frozen after. Nothing else about ordering changes.

## §9 Verification

- Live-window receipts like v3.2's: run the real app during a live window (MLB/CFB now; NFL if week 1 arrives first), capture kind-driven scoring words on real plays (a FIELD GOAL rendering FIELD GOAL structurally), the soccer card chip if the slate provides one (else recorded honestly as not-fired).
- The daygame render gate (§7) is the only Walter sitting in this sub-project — one frame, one look.
- CPU re-sampled against v3.3's ~0.0% idle; the new mapping work must not move it beyond jitter.
- The NHL October probe documented as an open receipt with its exact command; closing it is a follow-up, not DoD.
- Full suite green at default parallelism; clippy zero; the fixture-truth tests (§1) demonstrably catch a live-state regression (one mutation receipt).

## §10 Non-goals

- No new leagues, views, commands, or request patterns; no per-game polling.
- Shot clock stays demo-only (no public field exists — settled by research).
- NHL board-wide PP chip (October follow-up), NHL shots-on-goal (summary boxscore untested — same follow-up).
- No logo redesign or size change; no full-college art (Direction A's cut).
- No provider abstraction work beyond what the above needs — ESPN remains the single provider.

## Verification (2026-09-04)

Task 13, the DoD sweep. Every number below was produced on this branch at
`3d0b296`; nothing here changed product code. Captures live in
`.superpowers/sdd/2026-09-03-gameday-v3-4-data-truth/live-captures/`.

### Suite + clippy

`cargo test`, default parallelism, verbatim result lines (518 tests, 0 failed):

```
     Running unittests src/lib.rs (target/debug/deps/gameday-c7deaff0697c3ea1)
test result: ok. 286 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s
     Running unittests src/main.rs (target/debug/deps/gameday-28c727f55daba84a)
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/config.rs (target/debug/deps/config-783b9ad214a9d1af)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/draw.rs (target/debug/deps/draw-c32e740536845090)
test result: ok. 117 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.04s
     Running tests/home.rs (target/debug/deps/home-e05b2f2c53aa586c)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/logo.rs (target/debug/deps/logo-9e8e07a33dc23a89)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
     Running tests/map_espn.rs (target/debug/deps/map_espn-157154d31e986f1a)
test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
     Running tests/poll.rs (target/debug/deps/poll-ecc62764cf24cade)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/theme.rs (target/debug/deps/theme-3cf9f445b37bd0a4)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
   Doc-tests gameday
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo clippy --all-targets` after `cargo clean -p gameday` (so the whole crate
is actually re-linted, not replayed from cache) — zero warnings, zero notes:

```
    Checking gameday v0.1.0 (/Users/.../.worktrees/v3-4-data-truth)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.60s
```

### Named receipts (spot-run)

| What | Test | Result |
| --- | --- | --- |
| R24 wiring: one red card, one reorder, frozen after | `app::tests::a_red_card_reorders_once_and_freezes` | ok |
| Red card from the scoreboard alone | `map_espn::a_red_card_yields_ten_men_from_the_scoreboard_alone` | ok |
| Two yellows on one athlete count as a red | `map_espn::two_yellows_on_one_athlete_count_as_a_red` | ok |
| Cut/hero identity — every digit cell agrees | `draw::the_takeover_and_the_hero_agree_on_every_digit_cell` | ok |
| Cut variants are a takeover and a two-row band | `dump::tests::cut_variants_are_a_takeover_and_a_two_row_band` | ok |
| Size sweep, 7 widths x 6 heights = 42 combos | `draw::the_board_survives_every_size_the_app_will_draw_at` | ok |
| Codepoint guard, dark set (230 marks) | `board::logo::tests::no_bundled_mark_uses_a_symbol_outside_the_quadrant_blocks` | ok |
| Codepoint guard, light set (230 marks) | `logo::light_marks_are_quadrant_only_too` | ok |
| Stem contract, 23 fixed names | `dump::tests::gallery_stems_are_the_promised_fixed_names` | ok |

Both codepoint guards are count-agnostic — they scan the bundle, so the 230
committed marks per ground are covered without a hardcoded number.

### Fixture teeth — the mutation receipt (§9)

Inverted the live-state mapping in `src/provider/map.rs`:

```rust
-            is_red_zone: sit_v["isRedZone"].as_bool(),
+            is_red_zone: sit_v["isRedZone"].as_bool().map(|b| !b),
```

Two tests went red on the real captured payload, not a hand-built struct:

```
test the_red_zone_meter_reads_the_flag_and_the_yard_line ... FAILED
test live_situation_maps_integers_not_strings ... FAILED
test result: FAILED. 50 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
```

```
thread 'live_situation_maps_integers_not_strings' panicked at tests/map_espn.rs:1049:5:
assertion `left == right` failed
  left: Some(true)
 right: Some(false)
```

Reverted; the file is byte-identical to `3d0b296` in the receipts commit.

### Live window — 2026-09-04 05:02Z-05:14Z

Slate at 05:02:35Z, checked per league before the app was launched:

```
baseball/mlb                live=2 total=9
football/nfl                live=0 total=16   (week 1 opens Wed Sep 9)
football/college-football   live=0 total=25
hockey/nhl                  live=0 total=7    (preseason, not started)
soccer/eng.1                live=0 total=1
soccer/usa.1                live=0 total=1
```

Two live MLB games, both in the 9th: ATH @ SEA and STL @ LAD. The real binary
ran in a detached 120x40 tmux session against a scratch `--config-dir`.
Session killed at 05:14Z; nothing left running.

- **Finals headline on the board — CONFIRMED.** Seven of the night's finals
  carried the scoreboard's own story text on the board row, e.g. "Pirates use
  six relievers to two-hit the Giants in a 5-2 victory", "Crow-Armstrong hits
  39th homer to back Gausman in the Cubs' 2-1 win over the Brewers".
  (`live-captures/01-board-live-finals-headlines.txt`)
- **`10 MEN` chip — NOT FIRED, recorded honestly.** No soccer match was live
  anywhere in the window (EPL and MLS each had one fixture, both scheduled).
  The chip's receipt stays the fixture tests above.
- **Kind-driven scoring word on a real play — PARTIAL.** A run-scoring play did
  fire at 05:10:38Z (LAD walk-off, `type.id=57`, `scoreValue=2`,
  "T. Hernández doubled to center, Betts scored and Edman scored") and the
  payload is exactly the shape §2's MLB rule reads — a non-28 pitch kind with
  `score_value > 0` = `RunScoringPlay`. But the poller was watching
  `situation.lastPlay.scoringPlay`, which ESPN never set on this play, so the
  frame carrying the word was not captured before both games went final. The
  payload is recorded (`live-captures/03-espn-lastplay-poll.log`,
  `04-espn-alternativetext-quirk.txt`); the on-screen word is not. Honest
  status: the kind's *input* is confirmed live, the rendered word is not.
  Next live window (NFL week 1, Wed Sep 9) closes it.

#### OPEN — live-window finding, MLB last-play label (not fixed here)

The live window caught a real data-truth bug that the fixtures did not.
`map::mlb_last_play_text` prefers `lastPlay.type.alternativeText`, but on MLB
that field is a *pitch-type* label, not the play outcome: pitch type id 5
("Ball") carries `alternativeText: "Walk"`, and id 36 ("Strike Looking")
carries "Strikeout". So every ball in the count renders as `Walk — <batter>`
and every called strike as `Strikeout — <batter>`. Observed on the board at
05:04:46Z (`Walk — M. Betts` on a 2-0 count) and at 05:13Z (`Walk —
T. Hernandez` on the final row of a game he ended with a double). ESPN's own
summary shows the shape:

```
id=5  alt=Walk   text=Ball        | Pitch 6 : Ball 3
id=3  alt=Double text=Double      | Pitch 7 : Ball In Play
id=57 alt=-      text=Play Result | T. Hernández doubled to center, Betts scored and Edman scored.
```

Left OPEN deliberately: this is a DoD sweep, and the fix is a behavior change
to the T4/T5 surface with fixture updates attached (the committed expectations
encode the same misreading — `"Walk — J. Sanoja"` appears as an expected value
in `board::cut` and `theme`). Sized as its own task.

### CPU — `--demo`, 30 s idle

Method as in the v3.3 receipt: `./target/release/gameday --demo` in a detached
120x40 tmux session, 30 s idle, then `ps -o %cpu` three times 3 s apart
(a lifetime average, so it is the comparable number).

```
v3.4 (3d0b296)     pid=9851    0.8  0.6  0.9   etime 00:39  %cpu 0.7
v3.4, second run   pid=11150   0.8  0.7  0.8   etime 00:39  %cpu 1.2
```

v3.3's receipt says 0.0 / 0.0 / 0.0, so this needed an honest check rather than
a jitter claim. Two things were measured:

1. **Is it startup cost?** No. Sampling cumulative CPU time on one v3.4
   process: `0:00.31` at 30 s, `0:00.82` at 90 s, `0:01.55` at 180 s — steady
   state ~0.8 %, not a decaying startup average.
2. **Is the machine the difference?** Partly. The pre-branch baseline
   (`c12235d`, v3.3 tip) was exported to a scratch tree, built `--release`, and
   measured the same way *on this machine tonight*: `0.3 0.4 0.4`, `0:00.13` at
   30 s → `0:00.34` at 91 s, steady state ~0.34 %. So v3.3's own code does not
   reproduce 0.0 % here either — that sitting's machine was quieter than this
   one.

Comparing like with like, same machine, same night: **baseline ~0.34 % vs v3.4
~0.8 %.** Roughly half a percentage point of a single core, both far under 1 %,
on a laptop also running parallel agent threads. Not a gate, and not claimed as
"no change" — v3.4 costs a little more per frame than v3.3 did, which is about
what the larger mark bundle and the new structural mapping would be expected to
cost.

### OPEN — the NHL October probe (follow-up, not DoD)

Per §4, whether NHL *scoreboards* carry power-play state is unverifiable until
the season starts. When NHL goes live in October, run:

```bash
curl 'https://site.web.api.espn.com/apis/site/v2/sports/hockey/nhl/scoreboard' | jq '[.events[]|select(.status.type.state=="in")][0].competitions[0].situation'
```

A non-null `situation` carrying strength / power-play fields promotes the
board-wide PP chip additively: one mapping in `map.rs` plus one fingerprint
entry. A null `situation` closes the question the other way and the zoom-only
chip stands. Confirmed still open on 2026-09-04: NHL had 7 scheduled events,
none live.

### README pass

- **Themes** paragraph already named four built-ins (broadcast / studio /
  gruvbox / daygame) with daygame described as the one light theme, and kept
  the note that the eight retired palettes still load as user files — landed in
  T11/T12, verified, unchanged.
- **Logo coverage** was the gap: nothing in the README described mark coverage
  after T10 replaced the pipeline. Added a `## Logos` section — pro leagues
  complete with counts, college ranked-only with the abbreviation fallback
  named as designed behavior, both grounds, and the two refresh one-liners
  (`LEAGUES="epl" tools/gen-logos.sh` for summer promotion/relegation,
  `LEAGUES="cfb cbb" tools/gen-logos.sh` for the weekly polls). Counts verified
  on disk: nfl 32, nhl 32, nba 30, mlb 30, soccer 50, wnba 15, ncaa 41 = 230
  marks per ground, 992K + 996K.
- **Keys** section spell-checked against `src/keymap.rs`: every key the README
  lists still binds to the action it claims. The line is a summary — `n`
  (TV NEXT) and `space` (TV LOCK) are TV-mode keys it omits, as before.
