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
