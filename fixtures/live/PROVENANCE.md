# fixtures/live/ provenance

Captured for v3.4 Task 1 (spec §1: fixture truth). Every file is a `curl | jq
'.'` passthrough of the ESPN response — full body, no truncation, no
top-level key stripping. Kept out of the JSON itself (byte-faithful to the
wire) and recorded here instead.

| File | Source | Captured | Notes |
|---|---|---|---|
| `cfb_scoreboard_live.json` | `https://site.web.api.espn.com/apis/site/v2/sports/football/college-football/scoreboard` | fresh, 2026-09-03 (this task) | Live at capture: events `401856663`, `401856768` both `status.type.state == "in"` with `competition.situation` present (down/distance + possession). |
| `mlb_scoreboard_live.json` | `https://site.web.api.espn.com/apis/site/v2/sports/baseball/mlb/scoreboard` | fresh, 2026-09-03 (this task) | Live at capture: events `401816788`, `401816793`, `401816792`, `401816794`, all `"in"` with `situation` (balls/strikes/outs/on-base). |
| `mlb_summary_live_full.json` | `https://site.web.api.espn.com/apis/site/v2/sports/baseball/mlb/summary?event=401816793` | fresh, 2026-09-03 (this task) | Live game, untruncated: 440 `plays[]` entries, 227 with `summaryType == "P"`. Same event as one of the `mlb_scoreboard_live.json` rows. |
| `epl_scoreboard_redcard.json` | `https://site.web.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard` | captured 2026-09-03 research probe (no EPL fixture live during this task; reused from cache rather than faking freshness) | Event `401879297` carries a red card in `competitions[0].details[]`: João Gomes (team 362), 40', `redCard: true`. |
| `nhl_summary_final_full.json` | `https://site.web.api.espn.com/apis/site/v2/sports/hockey/nhl/summary?event=<final NHL game>` | captured 2026-09-03 research probe (NHL scoreboard was all `"pre"` during this task; no live NHL game reachable) | Untruncated final-game summary — kept for the §4 join tests that need a real, full-size summary payload rather than the old 80-play-capped fixture. |

No committed fixture before this task carried `competition.situation` at
all — the v3.1-era `scripts/capture-fixtures.sh` only ever captured `post`
(final) events for the standing `fixtures/*_full.json` set, and its summary
capture piped through a python filter that dropped every key except
`{header, scoringPlays, drives, keyEvents, boxscore, leaders}` and capped
`plays[]` at exactly 80. These `fixtures/live/` files are the fix: real
in-progress state, full arrays, untouched top-level keys.
