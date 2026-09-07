# Changelog

All notable changes to gameday are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Continuous integration: `cargo fmt --check`, `cargo clippy -- -D warnings`, the test suite on Linux and macOS, an MSRV check against the `rust-version` in `Cargo.toml`, `cargo-deny` (bans, licenses, advisories, sources), and a `cargo package` step that fails if the crate exceeds 5 MB.
- Dependabot, weekly, for Cargo dependencies and GitHub Actions.
- `AGENTS.md` — how to work in this repo.
- Licensed MIT OR Apache-2.0.
- `scripts/capture-replay.sh` — captures consecutive real scoreboard polls (and the closing summary) into `fixtures/replay/` for the replay test harness; dev-only, excluded from the packaged crate.
- MLB replay fixtures `fixtures/replay/mlb-20260907-0334` and `fixtures/replay/mlb-20260907-0355`, exercised by `tests/replay.rs`; dev-only, excluded from the packaged crate.

### Fixed
- The scoring cut names the actual scoring play instead of whatever the poll happened to catch, via a one-shot summary catch-up when the scoreboard's own last play isn't the scoring one.
- College rows no longer print the field position twice.
- A red zone chip is shown only when a team is possessing.
- Zoom rows show period and clock together, and never print the clock twice.
- A 0-0 record is hidden while the game is live or final, instead of printed as if it meant something.
- An unknown theme name is now reported in the footer instead of silently falling back.
- Standings say PRESEASON or POSTSEASON instead of leaving the season type unlabeled.

### Changed
- ratatui 0.30, crossterm 0.29, ureq 3 (proxy env vars are now honored), dirs 7, tui-big-text 0.8.
- SIGTERM and SIGHUP quit through the normal restore path; the terminal is never left in raw mode.
- A 304 from ESPN is recognized as "cache is current" again (ureq 3 delivers it as a normal response), and a server that accepts but never answers now times out after 10 s instead of blocking the poll thread.
