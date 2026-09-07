# fixtures/replay/ provenance

Consecutive scoreboard polls, 15 s apart, captured by `scripts/capture-replay.sh` — full payloads (filtered to one event where noted), then the summary for that event fetched once after the last poll. Used by `tests/replay.rs`, which feeds each sequence through the real apply/merge path and asserts the scoring cut names the actual scoring play.

| Directory | League / event | Captured (UTC) | Polls | Deltas (pair: scores) | Notes |
|---|---|---|---|---|---|
| `mlb-20260907-0334` | MLB `401816841` WSH @ LAD | 2026-09-07 03:34 | 80 | `18 -> 19: ["1","4"] -> ["2","4"]`; `39 -> 40: ["2","4"] -> ["3","4"]`; `50 -> 51: ["3","4"] -> ["4","4"]` | LAD scored three unanswered runs in the bottom 5th (query: `jq -r '.scoringPlays // .plays \| map(select(.scoringPlay==true)) \| .[-1].text'`): "T. Hernández reached on infield single to shortstop, Smith scored, Freeman to second." — ties the game 4-4. |
| `mlb-20260907-0355` | MLB `401816841` WSH @ LAD | 2026-09-07 03:55 | 80 | `31 -> 32: ["4","4"] -> ["4","5"]` | WSH retook the lead in the top 6th (same query): "Crews hit by pitch, Ford scored, Wood to second, Young to third." — away 5, home 4. |
