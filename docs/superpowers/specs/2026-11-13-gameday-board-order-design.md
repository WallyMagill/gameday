# gameday — old finals and the nothing-live order

Date: 2026-11-13. Status: behavior approved in principle (Walter, 2026-11-13); the stale clock cell's look waits on a render sitting.
Evidence: the 10:08 EST capture of all nine bare scoreboards on 2026-11-13, with nothing live. The FINAL section listed a WNBA final from Oct 31, an MLB final from Oct 28 and five Premier League finals from Nov 8, undated, beside last night's results. `--once --top 5` printed five of last night's finals while 18 upcoming games sat under LATER. Trimmed fixtures: `fixtures/PROVENANCE-2026-11-13-slate.md`.

## Directions

Recorded as directions, not rulings. Each names what won, why, and what reopens it.

1. **A result from before yesterday shows its date and closes its section.** A `final` or `off` game whose local start date is more than `RESULT_CURRENT_DAYS` (1) before today is stamped `Game::earlier_day`. Its clock cell carries its date in `fmt_start`'s spelling, and it sorts after every current row of its section, MY GAMES included. A game with no start is current. Nothing is dropped. Only today's board is stamped: a date-traveled board names its day in the header. Reasoning: ESPN's bare scoreboard serves a league's last game day while it is between rounds or out of season, and an undated October final reads as last night's. The 1 is measured: at 10:08–10:15 ET the NBA, NHL and college basketball boards still served 11-12, so last night's results stay undated through the morning. Reopens if ESPN starts dropping old days itself, or if a league's results turn out to linger past the next morning.
2. **Staleness is decided when a scoreboard lands, not when the board draws.** Reasoning: the board reorders only on a real event, and a final crossing midnight would otherwise move on the clock alone. A poll result is an event. The cost: a final from yesterday that is still on the board becomes dated at the first poll after midnight (within 60 s idle), not at midnight itself. Reopens if that move proves noticeable on a live night.
3. **With nothing live, LATER is drawn above FINAL.** "Nothing live" is read on the tab's unfiltered board, MY GAMES included, so a `/` filter never flips it; only a status change can. With anything live the order stays IN PLAY, FINAL, LATER. Reasoning: with nothing live the next game is the news, and `--once --top N` should return it. `derive()` hands out the one ordered section list that the board, `--once` text, the selection (j/k, `--json`), the lane's counts and the layout cascade all read. Reopens if the flip reads as the board jumping when a lone late game ends.
4. **LATER runs in start-time order**: missing starts last, equal starts in board order. Reasoning: league order put Sunday's NFL above tonight's 7:00 PM kickoff, and after item 3 `--top 3` would have returned the wrong games. Reopens never on its own.

## Render gate (pending)

The stale final's clock cell (`CLOCK_W` 14), on the real 10:08 slate, at 120×36 and 80×24 (`out/design/`):

- **A** `FINAL OCT 31`, `FT NOV 8`: the status word, then the date. Shipped as the default.
- **B** `OCT 31`: the date alone.
- **C** `OCT 31 7:00 PM`: the date and start time, the form a far-off LATER row already prints.

A stale called-off row (`POSTPONED NOV 8` is 15 cells) shows the date in the clock cell under every option, and ESPN's word moves to the prose column, which a called-off row leaves empty (`stale-off-a-120.png`, one synthetic postponed match added to the real slate). Switching options is the one line in `rows::earlier_day_clock`.
