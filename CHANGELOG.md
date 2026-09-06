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

### Changed
- ratatui 0.30, crossterm 0.29, ureq 3 (proxy env vars are now honored), dirs 7, tui-big-text 0.8.
- SIGTERM and SIGHUP quit through the normal restore path; the terminal is never left in raw mode.
- A 304 from ESPN is recognized as "cache is current" again (ureq 3 delivers it as a normal response), and a server that accepts but never answers now times out after 10 s instead of blocking the poll thread.
