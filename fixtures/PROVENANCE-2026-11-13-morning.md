# fixtures/nba_scoreboard_2026-11-12.json, fixtures/nba_scoreboard_2026-11-13.json

Two dated NBA scoreboards from the app's host and path (`site.web.api.espn.com/apis/site/v2/sports/basketball/nba/scoreboard?dates=…`), fetched 2026-11-13 16:49Z (`?dates=20261112`) and 16:48Z (`?dates=20261113`). Raw bodies, compact; only the `events` arrays were trimmed, so the packaged crate (which carries them for the unit tests in `src/provider/espn.rs`) stays small.

- **2026-11-12:** trimmed from 4 events to 1: TOR @ PHI (401909976, `STATUS_FINAL`). This is the slate ESPN's bare NBA scoreboard still served at 10:08 ET on 2026-11-13. A dated body has no `day` object; the tests add `"day": {"date": "2026-11-12"}`, the field a bare body carries (the bare body captured at 16:48Z, after the rollover, carried `"day": {"date": "2026-11-13"}`).
- **2026-11-13:** trimmed from 9 events to 2: NY @ IND (401909296) and MIL @ ORL (401909297), both `STATUS_SCHEDULED`. Tonight's slate, which the bare board did not show until ESPN rolled its day over (not rolled at 10:15 ET, rolled by 11:46 ET).

They pin the morning gap: while the bare board's `day.date` is the local date minus one, the provider also fetches the dated board for the local date and merges the two by event id.
