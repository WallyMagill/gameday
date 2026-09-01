# gameday v3 · sub-project 1 — Truth

Date: 2026-08-31
Status: approved in-session (Walter); spec awaiting his read
Series: v3 is four sub-projects, each its own spec → plan → build:
**1 Truth** (this) · 2 Identity (ranked board, `:tv`, themes) ·
3 Reach (leagues, second provider) · 4 Ship (packaging, launch).
1 and 2 run in parallel; 3 depends on 1; 4 last.

Source of every finding below: the 2026-08-31 review — live drive of the
binary against ESPN with 7 MLB games in play, five parallel audits (code,
data, UX, design, market), 294 tests green, clippy 7 warnings.

## Goal

Every number on screen is real, local, and current. The app degrades loudly
instead of silently. The first screen is the best screen. Nothing in this
sub-project changes tile shape, color, digits, logos, themes, or the sidebar —
that is sub-project 2, and it builds on this one.

Grades this sub-project moves: data layer C → A, correctness B- → A,
tests A- → A, UX C+ → B+ (the remaining UX points are visual).

## Decisions

- **Direction: Home = every live game across enabled leagues; pins and
  favorites sort first** — reasoning: tonight's first boot was a 62%-empty
  screen while 7 games were live, because Home showed only pins the user
  hadn't made yet; this removes the empty state in every season and after
  every pin expiry; reopens if users ask for a pins-only mode (would be a
  one-key config, not a redesign).
- **Direction: config lives at `~/.config/gameday` on every platform** —
  reasoning: it is what the README already claims and what terminal users
  expect; `dirs::config_dir()` put it in `~/Library/Application Support` on
  macOS with no way to relocate it; reopens never (a read-fallback keeps old
  installs working).
- **Direction: summary is polled only for the zoomed game; every tile's last
  play and scoring events come from the scoreboard** — reasoning: measured tonight, the MLB
  summary is 917 KB and was polled every 15 s per visible live tile
  (~490 KB/s with eight tiles), while the 232 KB whole-league scoreboard
  already carries `situation.lastPlay`; reopens if a sport's scoreboard turns
  out not to carry it.

## 1. Domain (`src/domain.rs`)

`Game` gains:

| Field | Type | Source |
|---|---|---|
| `scoring_plays` | `Vec<Play>` | two feeds, merged by (period, clock, text): for **every** game, a score delta between two scoreboard polls appends that poll's `situation.lastPlay` as a scoring play (this is how alerts already fire via `last_scores`); for the **zoomed** game, the summary's full list (`scoringPlays` for football, `plays[].scoringPlay` elsewhere) replaces it. Today `map_summary` builds the list and `merge_summary` discards it (`app.rs:905-921`), and `scoring` is flagged only on plays surviving the last-8 truncation (`map.rs:395`) — dead for all nine leagues |
| `start` | `Option<OffsetDateTime>` | `ev.date`, parsed; replaces `start_time: Option<String>` which was printed raw (`2026-09-10T00:20Z`) in tile titles and slate rows |
| `linescore` | `Vec<(u16, u16)>` | `competitors[].linescores[]` — per period/inning, away then home |
| `timeouts` | `Option<(u8, u8)>` | `situation.awayTimeouts/homeTimeouts` |
| `extras` | `Extras` | per-sport small enum: `Football { drive: Option<String> }`, `Baseball { hits, errors, pitcher, batter, due_up }`, `Hockey { shots, power_play: Option<(String, u16)> }`, `Soccer { events: Vec<MatchEvent> }`, `None` |

`Team` gains `rank: Option<u8>` (`competitors[].curatedRank.current`, present
for CFB/CBB; verified `14` for USC tonight).

`Play` gains `period: String` so baseball plays render `B9` instead of the
literal `[-:--]` (`tiles/mod.rs:517, 650`) — MLB plays carry no `clock`
object at all.

`Situation` gains `possession_abbr` drawn (it is mapped today and never
rendered), and baseball `pitcher`, `batter`, `due_up: Vec<String>` from
`situation.pitcher/.batter/.dueUp[]` (each `dueUp` entry has a `summary`
like `4-5, HR, 2B, 4 RBI`).

`MatchEvent { minute: String, kind: Goal|OwnGoal|Penalty|Yellow|Red|Sub,
team: String, player: String }` from soccer `competition.details[]` — the
whole goal/card feed is on the scoreboard and unused today.

`Meter::Penalty` is constructed from NHL summary `plays[].strength`
(verified `{"id":"702"}` on goals; live-scoreboard `situation` shape for NHL
still unverified — all games were `pre` tonight; the summary path is the
confirmed source, the scoreboard path is added when a live NHL fixture
exists).

`Summary.scoring_plays` stays; `merge_summary` now copies it onto
`Game.scoring_plays`.

## 2. Mapper (`src/provider/map.rs`)

- **Per-event fallibility.** A malformed event is skipped with one stderr
  line naming the league, event id, and the missing JSON path. It never
  aborts the league. Today `?` on required fields inside the event loop
  (`map.rs:203-245`) erases a whole slate on one placeholder row.
- **Time.** One function, `local_time(iso: &str, offset: UtcOffset) ->
  Option<OffsetDateTime>`. The offset is captured once on the main thread at
  startup (`UtcOffset::current_local_offset()`; `time` refuses to read TZ
  from a multi-threaded process on Unix) and passed to the mapper and to the
  poll thread. Display formatters (`9:38 PM`, `THU 8:20 PM`, `‹ SUN AUG 30 ›`)
  live in `src/text.rs`; nothing else formats a time. Prefer this over
  ESPN's `status.type.shortDetail` (`"8/31 - 9:38 PM EDT"`), which is fixed
  to Eastern.
- **MLB.** Plays filtered to `summaryType ∈ {S, N}` (scoring, at-bat
  narrative); tonight's distribution for one game was P=285 pitch rows,
  N=74, S=11. Scoreboard `situation.lastPlay.text` is a pitch string for MLB;
  use `lastPlay.type.alternativeText` (`"Strikeout"`) plus the batter name
  when the `summaryType` is `P`. `map_stats` reads the **grouped** shape
  (`statistics[{name:"batting", stats:[…]}]`) when the flat read yields zero
  rows; MLB summaries have no `leaders` key — leaders come from
  `boxscore.players[].statistics[].athletes` top-N by the group's leading
  stat, or are omitted (never `no stats yet` on a live game). Header count
  reads `2 OUT · 1-2` — count visibly not a score; the glyphing is
  sub-project 2's, the ambiguity fix is here. `outsText` is available if the
  wording should match ESPN.
- **Records** fall back to the abbr form (`SEA 64-73`) instead of vanishing
  when `name + record` exceeds the half-width (`tiles/mod.rs:311-326`
  dropped MARINERS/RED SOX but kept PADRES/REDS tonight).
- **CFB** date semantics: "today" and `[`/`]` use one fetch path,
  `?dates=YYYYMMDD&groups=80` (FBS, not Top-25). Tonight the Monday tab
  showed Saturday's ~100 finals under Monday's header while `[` to Saturday
  returned 8 games.
- **Soccer**: `competition.situation` does not exist (verified), so soccer
  never gets a `Situation`; `Extras::Soccer.events` carries goals/cards, and
  `form` (`"LLLWW"`) maps onto `Team.record` for soccer where the W-L-T
  record is less meaningful mid-season.
- Everything else already mapped stays as is.

## 3. Provider (`src/provider/espn.rs`)

- **Map, then cache.** A body is written to disk only after `map` succeeds.
  Today `espn.rs:30` writes first, so a 200 with an HTML interstitial or a
  schema change overwrites last-good and `map(&body)?` at `espn.rs:127`
  propagates without consulting the cache — the "stale rather than blank"
  promise fails in exactly the case it exists for.
- **Fallback on map failure**, not only on transport failure, returning the
  cached payload with `stale = true` and the mapping error attached.
- **Timeouts**: 10 s connect, 10 s read, on every request. Today there is
  none, and the poll thread is a single serial loop, so one hung socket
  freezes all nine leagues forever with only `UPD 47m` as a signal.
- **Backoff per league** (today one global counter retries the identical
  9-request burst), exponential with ±20 % jitter, capped at 5 min. League
  fetches are staggered across the polling window, never issued as one
  burst.
- **Conditional requests**: send `If-Modified-Since` / `If-None-Match` from
  the cached response headers; a 304 refreshes `cache_age` without a body.
- **User-Agent**: `gameday/<version> (+<repo url>)`. Today it is
  `Mozilla/5.0 (gameday/0.1)` — neither a browser nor an honest contact.
- **Error type** carries `status`, `url`, `retry_in`, and the mapping error
  when there is one, so the footer can print
  `ESPN 403 nfl scoreboard · retry 40s` (today `espn.rs:109` hardcodes
  `status=0` on a body-read failure).
- **Request budget** (receipts: ESPN returned `cache-control: max-age=5`
  on the scoreboard tonight; 40 rapid sequential requests produced 0
  non-200s; community convention is 30–60 s per resource):

  | Resource | Cadence | Scope |
  |---|---|---|
  | scoreboard | 15 s when any enabled league is live, 60 s otherwise | every enabled league, staggered |
  | summary | 15 s | zoomed game only |
  | box score | 30 s | zoomed game only |
  | standings | on open, then 10 min | current view |
  | dated slate | on target change, retry per `DATED_RETRY` | current view |

  Worst case with nine leagues live and one zoom: 36 + 4 + 2 = 42 req/min,
  of which up to 36 may be 304s. A `MemoryProvider` test asserts the budget.
- **`poll::plan` becomes the scheduler.** `main.rs:308-437` reimplements the
  plan inline and `tests/poll.rs` tests the unused one. The poll thread
  consumes `PollPlan` and owns no cadence constants of its own.

## 4. App

- **Home** = `home_games` returns pins (surviving the 6 h prune), then
  favorites' games, then every other live game across enabled leagues, then
  nothing else. League order inside each band follows `enabled_tabs`. Boot
  lands on Home. League tabs are unchanged.
- **Feedback for every verb.** `space` and `t` toast exactly like `:pin`
  does today; a pinned game shows `⚑` and a favorited team `★` in the tile
  title bar (sub-project 2 restyles the glyphs; the state must be visible
  now). `c` names the theme it switched to.
- **Keys.** `q` inside the help overlay closes help (today it quits the app
  from any depth, `app.rs:430`, while help and footer both say Q = BACK).
  `[`/`]` move into the keymap table (today handled at `app.rs:783-784`
  outside it, so help and footer never show them); a test asserts every key
  the handler matches is a `Binding`. Tab-bar order never changes when a
  league is toggled. Layout changes keep the selected game, not the page
  index. `:` completion shows the candidate list in the footer, not just the
  cycled value.
- **Filter footer names its scope**: `no games match "red" on MLB · ticker
  matches SEA@BOS · esc clears`. Today the board says no match while the
  ticker filters to the very game.
- **`App` split** (execution): `app/state.rs` (fields, view enum, mode),
  `app/derive.rs` (a per-frame `Frame<'a> { mosaic, slate, selection,
  scoring: Vec<&'a Game> }` built once per draw — replaces ~15 deep board
  clones per frame at 10 fps), `app/chrome.rs` (header, footer, help),
  dispatch stays in `input.rs`. Logo art is parsed once into a `OnceLock`
  map (today `load_logo` re-parses `include_str!` art twice per tile per
  frame).

## 5. Failure visibility

Header carries one status chip with these states and nothing else in that
cell: `LIVE` · `STALE 4m` · `OFFLINE · retry 40s` · `NO DATA YET`. It is
never concatenated with the date (today renders `STALEMON AUG 31`). The
`UPD` counter freezes and dims while stale (today it keeps resetting, so the
freshness signal contradicts the stale signal). Cold start with no network
and no cache shows the `OFFLINE` chip and one centered line in the board
area naming the last error and the next retry — never a blank board.

`panic::set_hook`: restore the terminal (leave alternate screen, disable
raw mode, disable mouse capture) **then** print the panic message plus
version and a "please file this" line. Today `RestoreTerminal::drop` runs
after the message is written, so `LeaveAlternateScreen` wipes it and the
app appears to vanish.

Every error string names the value, the expectation, and the knob:
`config.toml:7 unknown league "NFLL", valid: nfl|cfb|…`.

## 6. CLI and config

- `gameday` (board) · `gameday --demo` · `gameday --help` · `gameday
  --version` · `gameday --config-dir <path>`. Unknown flags exit 2 naming
  the valid set. `dump`, `probe`, `--tick` remain and are listed under a
  `dev:` heading in `--help`. Hand-rolled parsing is fine; no clap.
- Running without a TTY (`gameday | head`) exits 1 with
  `gameday needs a terminal (stdout is not a tty); try --help` instead of
  `Os { code: 6 }`.
- Config dir: `$XDG_CONFIG_HOME/gameday` else `~/.config/gameday`. If that
  does not exist and the legacy macOS dir does, read from legacy and print
  one line saying where the app now looks and how to move it. Never write to
  the legacy dir.
- `Config::load_from` and `load_pins` errors are fatal to *writes*, not to
  the app: the app runs with defaults, shows the parse error (file, line,
  key, expected form) in the status line, and refuses every `save_to` until
  restart. Today a typo is swallowed and the next keypress overwrites the
  file with defaults (`main.rs:149`).

## 7. Standings and date travel

- Sorted by win% (then W, then name) within each group. Division sub-headers
  when the feed groups by division (today rows sit in division order under a
  conference header and look unsorted). Season label always shown (today
  NBA/NHL/CBB show last season unlabeled). `T` / `OTL` columns only for
  sports that have them (today MLB shows 30 zeroes).
- CFB standings: probe `…/standings?group=<conf>`; ship conference tables
  if it answers, otherwise one line — `ESPN offers no CFB standings table;
  try :standings <conf>` — never a permanent `no standings yet`.
- Date travel header and slate always agree (see §2 CFB).

## 8. Tests

- Full-length real fixtures for all nine leagues, captured this week
  (today's WNBA summary fixture is a hand-cut tail of ~8 plays, which is why
  the truncation bug was invisible).
- Scoring-play regression: a fixture with 20 plays and the scoring play at
  index 3 must surface in `scoring_events()`.
- `local_time` across a UTC day boundary in `America/Los_Angeles` and
  `Europe/London`.
- Malformed-event skip: one broken event, the other N−1 map.
- Cache poison: a 200 with an HTML body serves the cached payload with
  `stale = true`; the disk file is unchanged.
- Request budget against `MemoryProvider` with a fake clock: nine leagues
  live plus one zoom for ten simulated minutes stays ≤ 42 req/min.
- Config parse error → no `save_to` succeeds; status line names file/line.
- Handler → keymap coverage.
- Home ordering: pins, favorites, live by league order; nothing else.
- Standings sort and column selection per sport.
- Dump gallery gains `home-live`, `offline`, `stale`, `config-error`.

## 9. Non-goals

Tile shape, color budget, digits, logos, themes, sidebar, ranking, `:tv`
(sub-project 2). New leagues, athlete-shaped competitors, a second provider
(3). LICENSE, CI, releases, Homebrew, README rewrite (4). NHL live
`situation` mapping stays deferred until a live NHL fixture exists.

## 10. Definition of done

- `cargo test` green; `cargo clippy --all-targets` clean.
- A real-terminal capture of Home on a live night shows live games, local
  times, and at least one scoring entry in TOP PLAYS / ticker.
- Offline capture shows the `OFFLINE` chip and the retry line; stale capture
  shows `STALE 4m` with a frozen `UPD`.
- `gameday --help | head -3` prints usage; `gameday --version` prints the
  Cargo version.
- A 10-minute request log with nine leagues enabled and one zoom stays under
  the documented budget.
- The implementer looks at their own PNGs (dump gallery + one live capture)
  before each commit — same gate as v1/v2.

## Verification (2026-09-01)

Run on 2026-09-01, 13:15–13:31 EDT, from the `v3-1-truth` worktree at
7ac096b + the env-gated request log added by this task. Every live run used
`--config-dir /tmp/gd-dod` (scratch config; the real config was untouched)
and was driven inside tmux at 120×40.

### Tests and lint

```
$ cargo test 2>&1 | grep -E '^test result'
test result: ok. 199 passed; 0 failed; ...   (lib)
test result: ok. 71 passed; ...              (integration)
test result: ok. 31 / 11 / 10 / 8 / 7 / 7 / 5 / 5 / 3 / 0 passed  (remaining suites)
```

357 tests, 0 failed, 0 ignored across 12 suites.

```
$ cargo clippy --all-targets 2>&1 | grep -c '^warning'
7
```

7 = 5 distinct warnings + 2 per-crate summary lines. Not clean, but not this
branch's: `git blame` dates every one to 2026-08-29/30 (pre-v3.1 commits
72c417f, 02a833b, 59a3cf7, 09b6fc8, 27dc942) — `src/app/mod.rs:53`
(`% 2 == 0` → `is_multiple_of`), `src/app/mod.rs:1490` (field assignment
after `Default::default()`), `src/provider/memory.rs:15` (missing `Default`),
`src/views/board.rs:142` (`sort_by` → `sort_by_key`),
`src/dump.rs:683` (`trim_start` before `split_whitespace`). The count is
unchanged by v3.1 and by the request-log lines added here. §10's "clippy
clean" is therefore not met at the letter; no v3.1 code contributes.

### Live capture (Home)

```
$ tmux new-session -d -s dod -x 120 -y 40 './target/release/gameday --config-dir /tmp/gd-dod'
$ tmux capture-pane -t dod -p > /tmp/gd-dod/home-live.txt
 GAMEDAY [ALL] [ NFL ] [ CFB ] [ CBB ] [ NBA ] [ WNBA ] [ NHL ] [ MLB ] [ EPL ] [ MLS ]   TUE SEP 1 2026  1:17:50 PM
                               nothing live · next: NYM @ TB 6:40 PM
                                                                    │⚑ GLOBAL ALERTS     │
                                                                    │no alerts           │
                                                                    │TOP PLAYS           │
                                                                    │no scoring yet      │

$ grep -c 'AM\|PM' home-live.txt   → 3   (local clock + next-up time)
$ grep -c '2026-'   home-live.txt  → 0   (no ISO timestamps)
$ grep -c -- '-:--' home-live.txt  → 0   (no placeholder clocks)
```

Honest gap: nothing was live anywhere at 13:15–13:31 EDT on a Tuesday
afternoon — the first MLB first pitch was 6:40 PM. Home drew the
`nothing live · next: NYM @ TB 6:40 PM` line (the §10 alternative), and the
ALERTS / TOP PLAYS lanes read `no alerts` / `no scoring yet` for the whole
window. The "at least one scoring entry within ten minutes of a live game
scoring" clause could not be exercised in this window; the scoring path's
receipts remain the simulator gallery (`out/home-live.png`, which shows
GLOBAL ALERTS and TOP PLAYS populated) plus the Task 9 delta tests.

League tab (`:mlb`, 5 s later) — slate rows carry local times, not ISO:

```
┌[MLB] UPCOMING───────────────── 6:40 PM MLB.TV ┐┌[MLB] UPCOMING───────────────── 6:40 PM MLB.TV ┐
│  ⟨NYM⟩ …  METS 62-76   RAYS 82-55             ││   ⟨SD⟩ …  PADRES 73-65  REDS 65-73            │
```

### Offline and stale

```
$ cargo run --release -- dump           # writes out/*.ansi + out/*.png

out/offline.ansi (ANSI stripped):
 GAMEDAY   FILTER: [ALL] [ NFL ] … [ EPL ]       OFFLINE · retry 40s  MON AUG 31 2026  9:30:01 PM
                                        OFFLINE · retry 40s
                            last error: ESPN unreachable nfl scoreboard

out/stale.ansi (ANSI stripped):
 GAMEDAY   FILTER: [ALL] [ NFL ] … [ EPL ]                  STALE 4m  MON AUG 31 2026  9:30:01 PM
 NAV: … [Q] QUIT                                                  PAGE 1/2  UPD 4m ·
```

Chip, board line, and error string on offline; `STALE 4m` chip with the
frozen `UPD 4m ·` footer on stale.

### 10-minute request budget

`poll_loop` gained a `GAMEDAY_LOG_REQUESTS=1` stderr line per request
(seconds since loop start + the `Request`); nothing else changed.

```
$ tmux new-session -d -s dod -x 120 -y 40 \
    'GAMEDAY_LOG_REQUESTS=1 ./target/release/gameday --config-dir /tmp/gd-dod 2>/tmp/gd-dod/reqs.log'
# default config → all nine leagues; :mlb, then z on the first game (zoom at t=49.3s)

$ awk '$1>=49.3 && $1<=649.3' /tmp/gd-dod/reqs.log | grep -c .
149                      # budget ceiling 440
scoreboard 89   summary 40   stats 20   standings 0   dated 0
```

Sample lines:

```
    0.205 Scoreboard(Nfl)
   46.683 Scoreboard(Epl)
   49.342 Summary(Mlb, "401816759")
   49.425 Stats(Mlb, "401816759")
   79.394 Stats(Mlb, "401816759")
```

What the cadence rules predict for the observed state (nothing live → 60 s
scoreboard cadence, one zoom → 15 s summary / 30 s stats, standings TTL
600 s and the RECORDS rail already warm): 9 × 10 = 90 scoreboards, 40
summaries, 20 stats, 0–1 standings ≈ 150. Observed 149 (89/40/20/0/0) — one
scoreboard slot fell just outside the window edge from ±20% jitter. 34% of
the 440 ceiling.

### CLI

```
$ ./target/release/gameday --help | head -3
gameday — terminal sports board. Pin games, they tile.

USAGE

$ ./target/release/gameday --version
gameday 0.1.0

$ ./target/release/gameday --bogus; echo "exit=$?"
gameday: unknown argument "--bogus", valid: --demo|--help|-h|--version|-V|--config-dir <path>|dump [--tick N]|probe <league>
exit=2

$ bash -c './target/release/gameday | cat; echo "exit=${PIPESTATUS[0]}"'
gameday: gameday needs a terminal (stdout is not a tty); try --help
exit=1
```

### PNG gate

`out/` gallery regenerated by the `dump` run above and reviewed by eye,
including `home-live.png` (live tiles, alerts, top plays), `offline.png`,
`stale.png`, `config-error.png`.
