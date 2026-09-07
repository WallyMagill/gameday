# fixtures/review-slate-2026-09-05.json

The CFB scoreboard (`…/football/college-football/scoreboard?groups=80&limit=300&dates=20260905`) as the app's own cache held it at 2026-09-05 16:52 EDT, during the independent review that started the v4 ship pass. Byte-faithful `curl | jq '.'` shape (the cache is the raw body), 68 events, 18 live, 16 carrying `situation.lastPlay.probability`.

It pins finding R1 (spec §1): with the v3.4 formula the hero was FOR at NDSU (FCS, 0-17, a 2-MIN chip) while No. 2 Oregon trailed Boise State 7-17 in the second quarter at a 28% home win probability. `tests/ranking_slate.rs` asserts the wave-2 formula leads with Boise at Oregon.
