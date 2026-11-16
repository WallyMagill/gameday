# fixtures/nfl_scoreboard_2026-11-13.json, fixtures/wnba_scoreboard_2026-11-13.json

The bare NFL and WNBA scoreboards (`…/football/nfl/scoreboard`, `…/basketball/wnba/scoreboard`, no `dates=`) as the app's own cache held them at 2026-11-13 10:08 EST, with nothing live on any of the nine boards. Raw bodies, re-serialized compact.

- **NFL:** trimmed from 14 events to 2: WSH @ NYG (401873052, `STATUS_FINAL`, Thursday night, 2026-11-12 8:15 PM EST) and NE vs DET (401873053, `STATUS_SCHEDULED`, Sunday 9:30 AM EST). Nothing else edited.
- **WNBA:** as captured, one event: ATL @ GS (401918311, `STATUS_FINAL`, 2026-10-31). The season is over and ESPN's bare scoreboard still serves its last game day (`day.date` 2026-10-31).

They pin the two 2026-11-13 morning findings: a final from an earlier day sat undated among last night's results, and with nothing live `--once --top N` printed last night's finals above the day's upcoming games. `tests/once.rs` asserts that LATER leads, that last night's NFL final stays undated, and that the October final closes FINAL with its date.
