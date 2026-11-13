# Changelog

All notable changes to gameday are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.1.0] — 2026-11-13

A new board order when nothing is live, dated results from earlier days, a closed terminal that ends the board, and the fixes from a pre-launch security review. It is a minor release because a script can see three of the changes: the order of `--once` output, a partial `config.toml` that used to be rejected now loading, and the exit status after a closed terminal.

### Added
- An ESPN drift canary in CI (`.github/workflows/espn-canary.yml`, `scripts/espn-canary.sh`). Twice a day it runs the real binary against all nine leagues' live scoreboards and fails in any of four cases: a league doesn't fetch, the mapper drops an event, ESPN sends a status gameday hasn't been checked against, or gameday's status disagrees with ESPN's own `state` + `completed`. Budget: 9 scoreboard requests per run. `docs/dev.md` has what to do when it fails.
- `SECURITY.md`: how to report a vulnerability privately (GitHub's private vulnerability reporting), which versions get fixes (the latest release only), and what gameday touches on the network, on disk and through other processes.
- A bug-report issue template. It asks for the version, the OS and terminal, the install method, and the last lines of `gameday.log`.

### Changed
- With nothing live, LATER is drawn above FINAL: on the board, in `--once` text, and in the order of `games` in `--json`. At 9:50 AM on 2026-11-13, `--once --top 5` printed five of last night's finals while 18 upcoming games sat below them; `--top N` now counts the upcoming games (after MY GAMES) before the finals. One live game restores the order IN PLAY, FINAL, LATER. Whether anything is live is decided before a `/` filter applies, so typing a filter never flips the order. The JSON schema is unchanged.
- LATER is in start-time order, with a game that has no start time last. It was in league order, which put Sunday's NFL games above that evening's 7:00 PM kickoff.
- A final or called-off game that started before yesterday shows its date in the clock cell (`FINAL OCT 31`, `FT NOV 8`), on the board and in `--once` text, and sorts after the current rows of its section, MY GAMES included. ESPN's scoreboard keeps serving a league's last game day between rounds and out of season: on the morning of 2026-11-13 the board listed a WNBA final from Oct 31, an MLB final from Oct 28 and five Premier League finals from Nov 8, undated, beside last night's games. Yesterday's results stay undated because ESPN's NBA, NHL and college basketball scoreboards still served Nov 12 at 10:15 ET on Nov 13. The day is decided each time a scoreboard arrives, never when the board draws, so after midnight the first scoreboard to arrive (within one poll interval, even with unchanged bytes) dates the row and moves it. Nothing is dropped, and the `--json` fields are unchanged. A called-off game from an earlier day prints ESPN's word (`POSTPONED`) in the row's text column instead, because its clock cell holds the date.
- The live board deletes old cache files when it starts. Before, the cache only grew: one real cache held 422 files and 186 MB going back to September. It deletes per-game `<league>-<id>-summary` and `<league>-<id>-stats` files, per-date `<league>-scoreboard-YYYYMMDD` files, and their `.etag` files, when nothing has rewritten them in 14 days. It also deletes a temp file that a killed write left behind (`.<name>.<pid>-<n>.tmp`, see Fixed) once it is 14 days old. 14 days is a guess, stated in the code: a pinned game drops off 6 hours after its final, date travel reaches 7 days out, and the cache serves a file only when a fetch fails, so a wrong deletion costs one refetch. It never deletes the nine `<league>-scoreboard` files (the last good payload behind `STALE`), the `<league>-standings` files, or a file name it does not recognize. It runs only at the start of the live board, never in `--once`, `--demo` or `probe`. A failure is one line in `gameday.log` naming the first file it could not remove. The first run of 1.1.0 deletes the old cache files 1.0.x left behind.
- `gameday --help` opens with "a terminal sports board that ranks live games by watchability." It said "Pin games, they tile.", which described a layout gameday no longer has.
- `cargo update`: 34 crates updated and 2 added (`ar_archive_writer` 0.5.5, `object` 0.39.1). Among them: `ureq` 3.5.0 → 3.5.2 and `ureq-proto` 0.6.5 → 0.7.1; `rustls-webpki` 0.103.16 → 0.103.17, `rustls-pki-types` 1.15.1 → 1.16.0, and `webpki-roots` 1.0.11 → 1.0.12 (the root certificates compiled into the binary). `rustix` is listed under Security. MSRV 1.88 still builds (`cargo +1.88 check --all-targets --locked`).
- `cargo-deny` checks unsound advisories for every crate in the dependency graph (`unsound = "all"` in `deny.toml`). Its default checks direct dependencies only, which is how the `rustix` advisory below, on a transitive dependency, went unflagged. `BSD-2-Clause` and `CC0-1.0` leave the license allow-list: no crate in the lockfile uses either.
- CI: `actions/checkout` v5.1.0 → v7.0.1, pinned by SHA, in `ci.yml` and `espn-canary.yml`, with `persist-credentials: false` on all seven checkout steps. No later step pushes or fetches. `release.yml` is unchanged; it moves when cargo-dist is bumped.

### Fixed
- Closing the terminal ends the board. Before, a closed window, `tmux kill-session` or the end of a `vhs` recording left the board running at full CPU, and the live board kept polling ESPN (two orphaned boards polled for about 2 h 20 min on 2026-09-28). The cause is in crossterm 0.29: once the terminal hangs up, its read loop gets EOF on every read and never checks its timeout, so `event::poll` never returns and the board never reads the quit flag that SIGHUP set. Now a signal thread sets the flag and waits 500 ms for the board to quit. If it has not, the thread restores the terminal and exits with status 128 + the signal number (129 for SIGHUP, 143 for SIGTERM). A board that quits inside the 500 ms exits 0, as before; a healthy board quits 12–66 ms after the signal (10 runs), so 500 ms is about 7 times the slowest. Measured with `tmux kill-session`: 1.0.1 was still running 8 s later at 93% CPU; 1.1.0 was gone in 0.52 s. Not covered: a terminal that hangs up without delivering a signal still leaves the board spinning; that needs a fix in crossterm.
- A `config.toml` that sets only some keys loads, and each missing key takes its default from the README's table. Before, a file holding only `theme = "nord"` failed with `missing field enabled_tabs`, and the board ran on defaults and saved nothing. Unknown keys are still ignored.
- League names are accepted in any case: `"nfl"` in `config.toml` and `pins.json`, `--league NFL`, `gameday probe NFL`. Before, `league = "nfl"` failed with `unknown variant nfl`, and `--league NFL` was "not a league". Saved files still write `"Nfl"`, as before.
- `gameday --demo --once` is an argument error (exit 2) that names both flags. It used to ignore `--demo` and fetch live scores.
- `--once` on a terminal that reports 0 columns (under `script`, for example) prints at 100 columns, the width a pipe gets. It printed blank lines.
- README install block: each method is its own copy-pasteable line, with Windows separate. The PowerShell installer had been pasted into the middle of the shell one-liner, so pasting the block failed. The crates.io page shows the fixed block from this release on.
- README status-bar recipe no longer names `--league mlb`, which prints nothing from November to March.
- A failed fetch on the live board writes its full error to `gameday.log`, with the URL and the cause, once per distinct error. Before, only the short form in the footer existed, so the URL and the cause were never recorded. `--once` still prints only the short form on stderr and logs nothing: a status bar runs it every few seconds, and the log has no rotation.
- The highlighted row stays on the game you put it on. Before, the selection was a row number: when the board reordered (a score, a game starting or going final, a pin), the highlight stayed on the row and another game moved into it, so `space`, `t` and `z` pinned, favorited or zoomed a game you had not picked. Now, once you move the selection (`j`/`k`, paging, a click), it follows that game to its new row and the board scrolls to keep it in view. A selection you have not moved stays on the top row, and `g` or `Home` puts it back there: the highlight then stays on the top row whatever game leads. A `/` filter that hides the game keeps it: the highlight waits on the nearest row and goes back to the game when the filter clears. When the game drops off the board or you step to another date, the highlight stays at the same row number and follows the game now in that row, or the top of the board if that row is the first. Changing tab starts again from the top row.
- Tonight's games show in the morning. ESPN's scoreboard for a league serves ESPN's own "current day", which rolls over late in the morning: on 2026-11-13 the NBA, NHL and college basketball scoreboards still served Nov 12 at 10:15 ET and had rolled by 11:46 ET. Until then the league tab, Home and `--once` showed only last night's results, and no `[`/`]` date reached today. Now, while a league's scoreboard says it is serving yesterday, gameday also fetches that league's scoreboard for today and shows both. It costs at most one more request a minute for each league that is behind (7 at most; NFL and college football never are). A game from tonight that starts before the rollover updates once a minute instead of every 15 s until then. If the extra fetch fails, the board is what it was before: no `STALE`, no footer error.
- Saving `config.toml` or `pins.json`, or writing a cache file or its ETag, replaces the file whole. Before, each write emptied the file and then filled it, so a process killed in between left an empty or partial file: favorites, settings or pins gone, or a last good scoreboard that `STALE` could not serve. This release made that likelier, because a closed terminal now ends the board 500 ms after the signal whatever the poll thread is writing. Now the bytes go to a temp file beside the target, `.<name>.<pid>-<n>.tmp`, which is then renamed over it. A `config.toml` that is a symlink (GNU stow, for example) stays a symlink and the file it points to gets the new bytes, or is created there if it does not exist yet. A file's permissions (`chmod 600`) are kept, and a file you cannot write (`chmod 444`) is still refused. A hard-linked `config.toml` no longer updates its other name, because the rename gives the path a new file. Not covered: a power loss just after a save, because nothing is flushed to disk.

### Security
Found by a pre-launch security review, each reproduced before it was fixed.
- A team color with a non-ASCII byte in ESPN's payload (`"aé3456"`) no longer panics. In the board the panic killed the poll thread for the rest of the session; in `--once` it aborted every league with exit 101. A color is now six ASCII hex digits after an optional `#`, or it is treated as absent.
- An event id must be ASCII digits. The id becomes part of a cache file name and of the summary URL, and on Windows an id like `/../../x` would have reached outside the cache directory. An event with any other id is skipped and logged with the id; the rest of the scoreboard still maps. All 603 events in the committed scoreboard fixtures have ASCII-digit ids.
- Response bodies are capped at 16 MiB after gzip decoding (`MAX_BODY_BYTES`). ureq's 10 MB limit counts compressed bytes, so 65 KB of gzip could decode to 64 MiB. Past the cap the fetch fails: the footer, or `--once`'s stderr, says `ESPN too big nfl scoreboard`, and the board's line in `gameday.log` names the cap and `MAX_BODY_BYTES`. The largest real payload in the owner's cache is 1,057,405 bytes (an MLB summary); 16 MiB is about 15 times that.
- `--once --json` and `gameday probe` strip control characters (C1 and DEL among them) and bidirectional overrides (U+202A–U+202E, U+2066–U+2069) from ESPN's text. serde_json escapes only U+0000–U+001F. A tab or line break becomes one space, so `Line\nTwo` prints as `Line Two`.
- `notify-send` gets `--` before the title and body, and `&`, `<` and `>` in the body are escaped. Before, a play text such as `--wait` was read as an option, and `<` or `&` as notification markup. The macOS `osascript` path is unchanged.
- Requests are HTTPS only: a redirect to plain HTTP is refused (ureq 3's default followed it). The footer, or `--once`'s stderr, says `ESPN not https nfl scoreboard`, and the board's line in `gameday.log` names the URL it refused. DNS resolution now has a 10 s timeout; before, a stalled lookup had no bound. Resolve, connect, response and body each get their own 10 s, so the worst case for one request is the sum of the four, not 10 s.
- `rustix` 1.1.5 → 1.1.6 clears RUSTSEC-2026-0486 (unsound: the extended-attribute functions in `rustix::fs` could mark unwritten memory as initialized). gameday never compiled that code: the build enables rustix's `std`, `stdio` and `termios` features, not `fs`. `ureq` 3.5.2 also applies its body limit after decoding; the 16 MiB cap above does not rely on it.

## [1.0.1] — 2026-09-28

Security and dependency patch, plus one fix: canceled and postponed games.

### Security
- `rustls` 0.23.43 → 0.23.45: RUSTSEC-2026-0285 (TLS 1.3 handshake messages accepted across encryption-level boundaries). `cargo-deny` was failing every CI run on the advisory, so this is the reason 1.0.1 exists.

### Fixed
- A canceled or postponed game no longer shows as `FINAL 0-0`. ESPN marks one `state: "post"` with `completed: false` (BAL @ NYY, 2026-09-27, `STATUS_CANCELED`); gameday read only the state. It now prints ESPN's word (`CANCELED`, `POSTPONED`) where the clock goes, with no score, at the bottom of FINAL; a pin on it expires like a final's, and no "went final" notification fires. `--json` reports it as `"status": "off"` — a new value in the documented set.

### Changed
- `toml` 0.8 → 1.1 (config and theme files parse and serialize as before; the round-trip tests cover the shapes gameday writes).
- `signal-hook` 0.3 → 0.4 for the SIGTERM/SIGHUP flags. `crossterm` still pins 0.3, so both build until it moves; `cargo-deny` reports the duplicate as a warning, not an error.
- `Cargo.lock` refreshed to the latest semver-compatible release of every other dependency (35 crates, all patch or minor — `ureq` 3.4.2 among them, Dependabot #5). The release binaries now build from the same versions a plain `cargo install gameday` resolves; MSRV 1.88 still builds (`cargo +1.88 check --all-targets --locked`).
- Dependabot ignores `actions/checkout`, `actions/upload-artifact` and `actions/download-artifact`: `cargo-dist` generates `.github/workflows/release.yml` and its `plan` job refuses a hand-edited copy (PR #1 could never go green). Those actions move when cargo-dist itself is bumped and `dist generate --mode ci` is re-run.

## [1.0.0] — 2026-09-12

First public release. No 0.x version was ever published, so this section is the feature list, not a delta.

### Added
- Installable four ways: prebuilt binaries on GitHub Releases for macOS (arm64, x86_64), Linux (x86_64, aarch64) and Windows (x86_64, built but untested); a shell installer and a PowerShell installer; `brew install WallyMagill/tap/gameday`; `cargo install gameday`. `docs/dev.md` holds the release procedure and the contributor tooling.
- Continuous integration: `cargo fmt --check`, `cargo clippy -- -D warnings`, the test suite on Linux and macOS, an MSRV check against the `rust-version` in `Cargo.toml`, `cargo-deny` (bans, licenses, advisories, sources), and a `cargo package` step that fails if the crate exceeds 5 MB.
- Dependabot, weekly, for Cargo dependencies and GitHub Actions.
- `AGENTS.md` — how to work in this repo.
- Licensed MIT OR Apache-2.0.
- `scripts/capture-replay.sh` — captures consecutive real scoreboard polls (and the closing summary) into `fixtures/replay/` for the replay test harness; dev-only, excluded from the packaged crate.
- Replay fixtures `fixtures/replay/mlb-20260907-0334`, `fixtures/replay/mlb-20260907-0355` and `fixtures/replay/nfl-20260911-0210` (SF at LAR, week one: a touchdown and its kick), exercised by `tests/replay.rs`; dev-only, excluded from the packaged crate.
- Paging: PgDn/PgUp and ctrl-d/ctrl-u move half a page, g/G and Home/End jump to the ends, in the board, feeds, standings and zoom lists.
- `:help` opens the help overlay.
- The `/` filter matches whole words by prefix (team name or abbreviation), a league slug at the start scopes the search, and the footer shows a match count.
- A day with no games for a league names the next scheduled game, or says none is scheduled in the loaded window.
- Desktop notifications for a favorite's own score and for a pinned or favorite game going final (`notify = […]` in config, on by default; `:notify test` sends a check).
- `gameday --once` fetches once and prints the ranked board, as text or `--json`, with `--league`, `--live`, `--top` and `--color`.
- A README demo GIF (`docs/demo.gif`) and its `vhs` tape (`docs/demo.tape`).

### Changed
- Team marks for every FBS school and eight D-I basketball conferences (ACC, Big East, Big Ten, Big 12, SEC, Atlantic 10, Mountain West, American), not just the AP/coaches top 25 — a game needs both teams marked before the hero draws either one, so most college games showed none before.
- A promoted row's long last play ends on a clause boundary instead of truncating mid-word.
- At 160 columns and wider, a live row carries its last play on the same line as its situation fragment instead of dropping it.
- ratatui 0.30, crossterm 0.29, ureq 3 (proxy env vars are now honored), dirs 7, tui-big-text 0.8.
- SIGTERM and SIGHUP quit through the normal restore path; the terminal is never left in raw mode.
- A 304 from ESPN is recognized as "cache is current" again (ureq 3 delivers it as a normal response), and a server that accepts but never answers now times out after 10 s instead of blocking the poll thread.
- The board's watchability order now weighs ranked matchups and, for football (NFL and college), ESPN's live win probability; situation chips count in proportion to how close the game is. The footer names why the selected game leads. A football game keeps its last win probability across a poll that lacks one (the extra-point row carries none), so a kick never reorders the board.
- Toasts (pin, favorite, sort, `:pin`) sit right of the key hints and clear after three seconds instead of sitting sticky forever.
- `c` opens the theme picker.
- The help overlay is sectioned by mode, with the mode you're in shown first, and closes with a glyph legend.

### Fixed
- The scoring cut names the actual scoring play instead of whatever the poll happened to catch, via a one-shot summary catch-up when the scoreboard's own last play isn't the scoring one.
- MLB play-result rows show the feed's sentence instead of `Play Result — <batter>`.
- College rows no longer print the field position twice.
- A red zone chip is shown only when a team is possessing.
- In football, the point-after no longer fires a second scoring cut after the touchdown: ESPN folds the kick into the touchdown's scoring row, and the board now does too (verified on the 2026 NFL opener, `fixtures/replay/nfl-20260911-0210`).
- Zoom rows show period and clock together, and never print the clock twice.
- A 0-0 record is hidden while the game is live or final, instead of printed as if it meant something.
- An unknown theme name is now reported in the footer instead of silently falling back.
- Standings say PRESEASON or POSTSEASON instead of leaving the season type unlabeled.
- A game starting more than six days out now prints its full date instead of getting clipped.
- The broadcast name and the odds now have air between them instead of running together.
- A rising game's `↑n` no longer glues onto its four-letter code.
- The STATS leaders column keeps a gap after a four-letter code, matching the board and plays feed.
- The zoom overview fills its whole pane instead of leaving a blank band under short feeds.
- The config editor's content starts under its header instead of overlapping it.
- A feed section rule no longer draws over an empty section.
- The stray block beside the hero's win-loss record is gone.
- A cached slate shown at launch is ranked from the very first frame, instead of jumping into order on the first fresh poll.
- A second cached league is ranked from its first frame instead of appended in the feed's own order (wave 3 residual).
