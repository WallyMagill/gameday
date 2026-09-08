# Developing gameday

## Build, test, lint

```bash
cargo build --release          # target/release/gameday
cargo test                      # fast: no network, no sleeps
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

`git config blame.ignoreRevsFile .git-blame-ignore-revs` once, in a fresh clone — it skips the whole-tree rustfmt commit when you run `git blame`.

## `gameday dump`

```bash
gameday dump [--tick N]
```

Writes the fixed-name capture gallery — 23 stems — into `out/`: the ranked board in each of the four built-in themes (`board-broadcast`, `board-studio`, `board-gruvbox`, `board-daygame`) plus two more sizes (`board-narrow` 80×24, `board-sixty` 60×40); every other surface (`tv`, `cut-full`, `cut-band`, `zoom`, `plays-feed`, `standings`, `config`, `filter`, `theme-picker`, `help`); four state captures (`home-live`, `offline`, `stale`, `config-error`); and the three-frame re-sort sequence (`nudge-seq-1/-2/-3`). No network — it runs entirely off the scripted demo slate.

Each stem is written as `.html` + `.ansi`, plus `.png` when headless Chrome is installed and reachable. Set `GAMEDAY_DUMP_FONT=/path/to/CascadiaMono.ttf` (or any monospace font file) if you want the PNGs' rules and meter tracks to render as crisply as your real terminal does — the HTML capture embeds it as a `@font-face` and otherwise falls back to whatever the headless browser has.

## `gameday frame`

```bash
gameday frame --view V --theme T --size WxH --scenario S [--tick N] --out PATH
```

Renders exactly one surface — the parameterized sibling of `dump`, for the design loop (below). Every flag:

- `--view` — `board` (default) `| tv | zoom | cut-full | cut-band | standings | plays | config | help | theme-picker | filter`
- `--theme` — a loaded theme name, or a path to a theme `.toml` (a candidate that isn't a built-in works too)
- `--size` — `WxH`, default `120x36`, valid range 40×12 to 400×200
- `--scenario` — `full-slate | redzone | thin-slate | finals-only | empty | nudge-resort | review-slate`
- `--tick` — sim tick to render at (default: the scenario's own beat)
- `--out PATH` — writes `PATH` plus `.ansi`/`.html` beside it

`review-slate` is the one scenario that isn't purely the scripted demo: it reads `fixtures/review-slate-2026-09-05.json` (the captured 2026-09-05 CFB afternoon, 68 events/18 live, real win probabilities) at run time, so it must be run from the repo root. Every other scenario, every knob, and the sim tick are pure functions of their inputs — two runs of the same command produce the same bytes.

## The design loop

Visual decisions get made from rendered PNGs, never in prose: build every option for real (code on a branch, or a `gameday frame` invocation), label the file by option (`out/design/<task>-<option>.png`), and present a lettered menu. See [`docs/design-loop.md`](design-loop.md) for the full loop and how a decision gets recorded.

## `tools/gen-logos.sh`

Regenerates the committed team-mark art (`assets/logos/**/*.ans` dark-ground, `assets/logos-light/**/*.ans` light-ground) from ESPN's team art via `chafa`, and rewrites the embed table. Dev-time only — the app ships the committed `.ans` files and never fetches art at runtime. Needs `chafa` and `jq` on PATH.

```bash
tools/gen-logos.sh                      # every league, poll-scoped college (default)
LEAGUES="nba epl" tools/gen-logos.sh    # just these leagues
COLLEGE=all LEAGUES="cfb cbb" tools/gen-logos.sh   # every FBS school + the top 8 CBB conferences
```

The script's default is `COLLEGE=all`, the shipped set (every FBS school plus eight D-I basketball conferences: ACC, Big East, Big Ten, Big 12, SEC, Atlantic 10, Mountain West, American — 165 unique marks, both grounds), so a bare `LEAGUES="cfb cbb" tools/gen-logos.sh` regenerates what is committed. `COLLEGE=poll` is the older ranked-25 set (AP/coaches top 25 only).

Rerun it when:
- **EPL churn** — three clubs relegate and three promote every summer; rerun `LEAGUES="epl"` once promotion is settled.
- **A new season's conference realignment or FBS membership changes** — rerun `COLLEGE=all LEAGUES="cfb cbb"` to pick up the new group membership.

## Replay tests

```bash
scripts/capture-replay.sh <league> <minutes> [event_id]
```

Captures consecutive real scoreboard polls (every 15s, the live cadence) into `fixtures/replay/<league>-<UTC yyyymmdd-hhmm>/`, plus the closing summary for the live event(s) in the last payload. `tests/replay.rs` replays every fixture directory under `fixtures/replay/` through the real `apply_boards` path and asserts the one rule the 2026-09-05 review found broken on live data: when a score moves, the cut names the play that scored, not whatever the poll happened to catch. A poll that fails during capture is skipped, not fatal — a live window can't be re-run, so one 503 shouldn't throw the rest away. A sequence with no score delta isn't worth committing; the script's own output says which consecutive pairs carry one.

## `gameday probe <league>`

Fetches one real scoreboard for `<league>`, maps it through the real provider code, and prints the result. Dev-only — no fixture involved, so it's the fastest way to check ESPN hasn't changed a field shape under you.

## Release procedure

The pipeline is `cargo-dist` (0.32.0), configured in `dist-workspace.toml` and generated into `.github/workflows/release.yml` by `dist generate --mode ci`; `.github/workflows/publish-crate.yml` is a hand-written reusable workflow wired in as a custom publish job (`publish-jobs = ["homebrew", "./publish-crate"]`). Re-run `dist generate --mode ci` after editing `dist-workspace.toml`, and `dist plan` to sanity-check the announcement before tagging.

1. Bump the version in `Cargo.toml` and the `CHANGELOG.md` date, in one commit.
2. `git tag vX.Y.Z` and push the tag (Walter does the push — this repo has no remote wired up for anyone else to push to).
3. Watch the release workflow (`Release` in Actions) run on the tag push: it builds five targets (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`), creates the GitHub Release with those archives plus a source tarball and checksums, publishes two universal installers (shell, PowerShell) to it, pushes a Homebrew formula to `WallyMagill/homebrew-tap`, and — via the `custom-publish-crate` job, which only runs on a non-prerelease tag — publishes the crate to crates.io with `cargo publish --locked`.
4. Verify on a clean machine (no prior `gameday` install, no cached tap):
   - `brew install WallyMagill/tap/gameday`
   - `cargo install gameday --version X.Y.Z`
   - the shell installer: `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/WallyMagill/gameday/releases/latest/download/gameday-installer.sh | sh`
   - a downloaded archive from the release page, unpacked and run directly
5. The rc form (`vX.Y.Z-rc.N`) exercises the same workflow — build, GitHub Release, shell/PowerShell installers, Homebrew tap — but skips the crates.io publish twice over: cargo-dist's own prerelease gate (`announcement_is_prerelease`, true for any `-rc.N` suffix) skips `custom-publish-crate` at the call site in `release.yml`, and `publish-crate.yml`'s own job carries `if: ${{ !contains(github.ref, '-rc') }}` as a second, explicit check. Push an rc tag to test the pipeline itself before cutting a real one.

Two repo secrets the pipeline needs, set at **Settings → Secrets and variables → Actions** on the `gameday` repo (repository secrets, not environment secrets):

- `HOMEBREW_TAP_TOKEN` — a fine-grained personal access token scoped to `WallyMagill/homebrew-tap` only, with repository permission **Contents: Read and write**. Create it at github.com → Settings → Developer settings → Personal access tokens → Fine-grained tokens.
- `CARGO_REGISTRY_TOKEN` — from crates.io → Account Settings → API Tokens → New Token, scoped to `publish-update` for the `gameday` crate (not a blanket publish-any token).

## Two `gh` accounts

This machine's default `gh` account is the work one. Before any `gh` command in this repo: `gh auth switch --user WallyMagill`. Switch back to the work account when you're done.
