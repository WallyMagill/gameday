# Changelog

All notable changes to gameday are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] — unreleased

### Added
- Continuous integration: `cargo fmt --check`, `cargo clippy -- -D warnings`, the test suite on Linux and macOS, an MSRV check against the `rust-version` in `Cargo.toml`, `cargo-deny` (bans, licenses, advisories, sources), and a `cargo package` step that fails if the crate exceeds 5 MB.
- Dependabot, weekly, for Cargo dependencies and GitHub Actions.
- `AGENTS.md` — how to work in this repo.
- Licensed MIT OR Apache-2.0.
- `scripts/capture-replay.sh` — captures consecutive real scoreboard polls (and the closing summary) into `fixtures/replay/` for the replay test harness; dev-only, excluded from the packaged crate.
- MLB replay fixtures `fixtures/replay/mlb-20260907-0334` and `fixtures/replay/mlb-20260907-0355`, exercised by `tests/replay.rs`; dev-only, excluded from the packaged crate.
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
- The board's watchability order now weighs ranked matchups and (college football) ESPN's live win probability; situation chips count in proportion to how close the game is. The footer names why the selected game leads.
- Toasts (pin, favorite, sort, `:pin`) sit right of the key hints and clear after three seconds instead of sitting sticky forever.
- `c` opens the theme picker.
- The help overlay is sectioned by mode, with the mode you're in shown first, and closes with a glyph legend.

### Fixed
- The scoring cut names the actual scoring play instead of whatever the poll happened to catch, via a one-shot summary catch-up when the scoreboard's own last play isn't the scoring one.
- MLB play-result rows show the feed's sentence instead of `Play Result — <batter>`.
- College rows no longer print the field position twice.
- A red zone chip is shown only when a team is possessing.
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
