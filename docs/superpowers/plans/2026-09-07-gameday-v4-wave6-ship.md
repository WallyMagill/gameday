# gameday v4 Wave 6 — Ship Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** gameday 1.0.0 installs four ways on a clean account (`brew install WallyMagill/tap/gameday`, `cargo install gameday`, the shell installer, a downloaded archive), with a README that opens on the GIF and a public history that carries no third-party screenshots.

**Architecture:** Four autonomous tasks land on `v4-wave6`: the crate package finalized and asserted in CI; the README and `docs/dev.md` written for a stranger; the cargo-dist release pipeline (`dist-workspace.toml`, `release.yml`, a `publish-crate` job); and the history-rewrite preparation (mirror backup, research moved out, `docs/research` removed from HEAD, the filter-repo command and size report ready). The rest is sequenced by real dependencies, not dates: Walter's owner actions (repos, tokens, the rewrite trigger, the first push, the tags), the NFL week-one live window (replay fixtures, the probability-field check, the budget receipt), and the rc dry run on a clean account.

**Tech Stack:** cargo-dist 0.32 (not installed — Brewfile addition is Walter's), `git-filter-repo` (installed), `gh` 2.100 (two accounts; `gh auth switch --user WallyMagill` before every command on this repo, switch back after), Homebrew tap `WallyMagill/homebrew-tap`, crates.io.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §8 (Wave 6), Directions 1, 3, 5, 9; §1 O1–O6, H3, T3, Q3; §9's carry-forward lines from waves 0, 3, 4, 5.

## Global Constraints

- Branch `v4-wave6` from `main` at `bb62351`. Commit after every task with the trailer; never push (the first push is Walter's, after the rewrite).
- Suite green after every task with the count reported (646 at the start; measured numbers win); `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny check` clean; `cargo package --locked` under 5 MB.
- Nothing that leaves this machine without Walter: no push, no tag, no publish, no `gh repo create`, no `brew install` (the Brewfile edit + install for cargo-dist is asked for once, like vhs).
- Every README claim is true at the moment it will be read: install lines are marked "after 1.0.0" until then; terminal claims are verified on this Mac (Ghostty, Terminal.app, tmux; iTerm2 is not installed here — say "reported" or drop it).
- Failures name the value, the expectation, and the knob: the CI package assertion prints the offending path.
- Rulings made while planning:
  - **The crate keeps its library, with a disclaimer.** 25 `pub mod`s serve the integration tests and `frame`/`dump`; making the crate bin-only is a rewrite for no user. `src/lib.rs` and the README say the Rust API is an implementation detail with no semver promise; 1.0.0 covers the CLI, the config file, and the `--once --json` schema.
  - **`assets/candidates/` stays in the crate** — `src/theme.rs` embeds `gruvbox-warm.toml` with `include_str!`; the wave 5 carry note was wrong on that one. `fixtures/nfl_standings.json` stays for the same reason (`src/dump.rs`). Everything else under `fixtures/` (17 MB) and all of `tests/` leave.
  - **The research citations become descriptions.** The nine comments naming `docs/research/v3-identity/*.png` describe the reference frame in words ("the 120-column NFL Sunday reference frame, measured 2026-09-01") — the files leave the repo and a path nobody can open is worse than a sentence.
  - **The version bump is its own commit at rc time**, made by the controller when Walter says the repo exists: `1.0.0-rc.1` for the dry run, `1.0.0` for the tag. Until then `Cargo.toml` stays 0.1.0 so nothing on this branch claims a release that has not happened.
  - **The launch kit lives in the session scratchpad and the vault**, not the repo (spec §8.6): drafted at the end of this wave, posted when Walter says.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `Cargo.toml` | the final `exclude`; version bumps at rc/tag time |
| `.github/workflows/ci.yml` | the package job asserts the shipped and the excluded paths |
| `src/lib.rs` | the API disclaimer |
| `src/board/hero.rs`, `src/board/rows.rs`, `src/views/tv.rs`, `tests/draw.rs` | research citations rewritten |
| `README.md` | the §8.4 order |
| `docs/dev.md` (new) | dump, frame, the design loop pointer, logo generation, replay capture, the release procedure, blame-ignore-revs |
| `CHANGELOG.md` | `## [1.0.0] — unreleased` heading in place, dated at the tag |
| `dist-workspace.toml`, `.github/workflows/release.yml` | cargo-dist (targets, installers, the tap, `publish-crate`) |
| `~/personal-projects/_data/gameday-backup-<date>/`, `~/personal-projects/_data/gameday-research/` | the mirror and the moved research (outside the repo) |
| spec §9, `docs/superpowers/specs/...` Directions | receipts; the rc and tag directions |

---

### Task 1: The crate package (§8.1, O2, O4; the wave 0/3/4/5 carries)

**Files:** `Cargo.toml`, `.github/workflows/ci.yml`, `src/lib.rs`, the four files with research citations
**Test:** the CI package job's assertions (run locally with the same shell), the existing suite

- [ ] **Step 1: `Cargo.toml` `exclude`** becomes:
  ```toml
  exclude = [
      "docs/",
      "tests/",
      "fixtures/*",
      "!fixtures/nfl_standings.json",
      "scripts/",
      "tools/",
      ".github/",
      "*.tape",
      ".git-blame-ignore-revs",
      "deny.toml",
      "out/",
  ]
  ```
  with the comment saying what stays and why (`assets/` whole: logos and themes embedded; `fixtures/nfl_standings.json`: the gallery's standings). Verify the negation works: `cargo package --list --allow-dirty | grep '^fixtures/'` prints exactly `fixtures/nfl_standings.json`; `grep -c '^tests/'` → 0; `grep -c '^docs/'` → 0; `grep -c 'assets/candidates'` → 1. If Cargo rejects the `!` pattern, list the fixture directories and the large files explicitly and say so.
- [ ] **Step 2: CI asserts the list.** In `ci.yml`'s `package` job, after `cargo package --locked`:
  ```yaml
      - name: the crate ships what the binary embeds and nothing else
        run: |
          cargo package --list --locked > /tmp/pkg.txt
          for must in assets/logos/nfl/kc.ans assets/themes/broadcast.toml assets/candidates/gruvbox-warm.toml fixtures/nfl_standings.json src/main.rs; do
            grep -qx "$must" /tmp/pkg.txt || { echo "missing from the crate: $must"; exit 1; }
          done
          for never in '^tests/' '^docs/' '^fixtures/replay/' '^fixtures/live/' '^scripts/' '^tools/' '^deny.toml$' '\.tape$'; do
            if grep -qE "$never" /tmp/pkg.txt; then echo "must not ship: $(grep -E "$never" /tmp/pkg.txt | head -3)"; exit 1; fi
          done
  ```
  Run the same block locally (`bash -e`) and paste its output in the report. Keep the 5 MB size check; report the packed size (`ls -l target/package/*.crate`).
- [ ] **Step 3: `src/lib.rs` header** (a `//!` doc comment): "gameday is a binary. This library exists so the integration tests, `gameday frame` and `gameday dump` can reach the same code; it is not a stable API and carries no semver promise. What 1.0.0 promises: the CLI flags, `config.toml`'s keys, `pins.json`, and the `--once --json` schema."
- [ ] **Step 4: Rewrite the nine research citations** (`grep -rn 'docs/research' src tests` → 0 after): each becomes a description of the reference frame — e.g. `hero.rs:69` "the 16×10 logo study (2026-09-01): quadrant blocks read at that size, sextants tofu", `rows.rs:69` "the 120-column NFL Sunday reference frame (2026-09-01): `GB 13 CHI 10  Q3 4:20  NFL  GB 3RD & 2 AT CHI 41`", `tv.rs:77` "the TV reference frame lists five games per strip row". Keep every measured number.
- [ ] **Step 5:** `cargo test --release`, clippy, fmt, `cargo deny check bans licenses advisories sources`, commit `chore(package): the crate ships what the binary embeds; research citations become descriptions`.

### Task 2: README and `docs/dev.md` (§8.4, O1, O5)

**Files:** `README.md`, `docs/dev.md`, `CHANGELOG.md`

- [ ] **Step 1: `README.md` in §8.4's order.** (1) `# gameday`, the positioning line ("A terminal sports board that ranks live games by watchability. Nine leagues, no account, one binary."), the GIF (`![gameday](docs/demo.gif)` — the file is in the repo, so the relative link renders on GitHub); (2) Install: `brew install WallyMagill/tap/gameday`, `cargo install gameday`, the shell one-liner cargo-dist generates (`curl --proto '=https' --tlsv1.2 -LsSf https://github.com/WallyMagill/gameday/releases/latest/download/gameday-installer.sh | sh`), a link to Releases — under a line "Available from 1.0.0; until then: `cargo install --git https://github.com/WallyMagill/gameday`" (drop that line at the tag); (3) the sixty-second tour on one annotated capture (`docs/tour.png` — the wave 5 `marks-all` frame re-rendered at 120×40 via `frame --scenario review-slate`, saved with the repo? No: `docs/` is excluded from the crate, so a PNG there costs nothing on crates.io; keep it under 300 KB) naming the ranked list, the hero, the chip, the cut, pins, favorites, TV; (4) a keys table by mode (from `keymap.rs`'s groups: board, zoom, tv, config, standings & feed, paging, everywhere) plus `:` commands; (5) config with every key and default (`enabled_tabs`, `favorites`, `theme`, `sort`, `notify`) and where it lives (`~/.config/gameday/`, `$XDG_CONFIG_HOME`, `--config-dir`, pins.json, gameday.log, cache/, themes/); (6) notifications (macOS/Linux/no-op; `notify = []`; `:notify test`); (7) `--once` and `--json` with the schema verbatim from spec §9 wave 4, the status-bar recipe (`#(gameday --once --live --top 1 --league mlb | tail -1)`), the notes (`--json` drops color; empty output prints nothing; exit 0/1/2); (8) data and trademark notice (unofficial ESPN endpoints, polling not websockets, last good payload on disk, not affiliated with ESPN or any league; marks are quadrant-block renderings of league-owned logos used only to identify teams, removed on request); (9) terminals: verified on Ghostty, Terminal.app and tmux on this Mac (say which you actually ran — Task 2's implementer opens the release binary in each for one screen and reports), iTerm2 "should work, unverified", Windows builds untested; troubleshooting (digits look wrong → font fallback: the board uses only `▀▄█`; no notifications → OS settings; STALE → network); (10) contributing pointer to `docs/dev.md`; the license line. Keep the current README's true facts (themes, logos, keys) but rewrite them in this order; drop `cargo run` as the first thing a reader sees.
- [ ] **Step 2: `docs/dev.md`**: build/test/lint one-liners; `gameday dump` (the 23-stem gallery, `out/`, Chrome, `GAMEDAY_DUMP_FONT`); `gameday frame` (every flag, the `review-slate` scenario, where the fixture lives); the design loop (one paragraph + link to `docs/design-loop.md`); `tools/gen-logos.sh` (`COLLEGE=poll|all`, when to rerun); `scripts/capture-replay.sh` and `tests/replay.rs`; `gameday probe <league>`; the release procedure (below, verbatim from Task 3's outcome); `.git-blame-ignore-revs` (`git config blame.ignoreRevsFile .git-blame-ignore-revs`); the two `gh` accounts rule.
- [ ] **Step 3: `CHANGELOG.md`**: `## [Unreleased]` stays for what lands after; a new `## [1.0.0] — unreleased` section takes everything currently under Unreleased, grouped by wave in one line each under Added/Changed/Fixed (the date is filled at the tag). Commit `docs: README for a stranger; docs/dev.md; the 1.0.0 changelog section`.

### Task 3: The release pipeline (§8.2, Directions 1 and 3)

**Files:** `dist-workspace.toml` (or `[workspace.metadata.dist]`), `.github/workflows/release.yml`, `docs/dev.md` (release procedure), `Cargo.toml` (only what `dist init` requires)
**Precondition:** `cargo-dist` on PATH (Walter's Brewfile go-ahead; `brew "cargo-dist"` beside `vhs`). If absent when this task is dispatched, the task writes both files by hand from cargo-dist 0.32's documented shape and marks them "to be regenerated with `dist init` once installed".

- [ ] **Step 1:** `dist init --yes` with: targets `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`; installers `shell`, `powershell`, `homebrew`; `tap = "WallyMagill/homebrew-tap"`; `publish-jobs = ["homebrew"]`; `changelog` from `CHANGELOG.md`; CI `github`. Then `dist plan` (offline, no network beyond the toolchain) and paste its plan in the report.
- [ ] **Step 2:** In the generated `release.yml`, add a `publish-crate` job on `v*` tags (needs `[build-local-artifacts, host]` per cargo-dist's job graph; `cargo publish --locked` with `CARGO_REGISTRY_TOKEN`; skip on `-rc.` tags so the rc never hits crates.io — `if: ${{ !contains(github.ref, '-rc') }}`). Document the two secrets Walter creates (`HOMEBREW_TAP_TOKEN`: a fine-grained PAT with contents write on the tap repo; `CARGO_REGISTRY_TOKEN`) in `docs/dev.md`'s release procedure with the exact repo settings path.
- [ ] **Step 3:** The release procedure in `docs/dev.md`: bump `Cargo.toml` version and `CHANGELOG` date in one commit; `git tag vX.Y.Z`; push the tag (Walter); watch the release workflow; verify on a clean user (`brew install WallyMagill/tap/gameday`, `cargo install gameday --version X.Y.Z`, the shell installer, an archive); the rc form of the same. Commit `ci(release): cargo-dist — five targets, three installers, the tap, publish-crate on non-rc tags`.

### Task 4: History-rewrite preparation (§8.3 steps 1–2 and the command for step 3)

**Files:** none in the repo except the `git rm -r docs/research` commit; artifacts outside the repo
**This task is the controller's** (it touches `~/personal-projects/_data/` and the backup is the safety net for a destructive step Walter triggers).

- [ ] **Step 1:** `git clone --mirror ~/personal-projects/game-day ~/personal-projects/_data/gameday-backup-2026-09-07/`; confirm `git -C <mirror> for-each-ref | wc -l` equals the source's and the HEAD hashes match.
- [ ] **Step 2:** `cp -R docs/research ~/personal-projects/_data/gameday-research/` and compare file counts (`find … | wc -l` both sides, 101 tracked files plus any untracked); then `git rm -r docs/research` and commit `chore: docs/research leaves the repo (kept at ~/personal-projects/_data/gameday-research)`.
- [ ] **Step 3:** Prepare, do not run: the fresh-clone command sequence for step 3 (`git clone ~/personal-projects/_data/gameday-backup-2026-09-07 /tmp/gameday-rewrite && cd /tmp/gameday-rewrite && git filter-repo --path docs/research --invert-paths`), the before numbers (`git count-objects -vH` size-pack 21.36 MiB, `du -sh .git` 23 MB, 1136 tracked files), and the expected after (the research images gone; the 9.9 MB replay fixtures and the 1.5 MB of marks stay, so report the pack and the working tree separately). Present to Walter: the numbers, the command, and what "go" does (replaces the original checkout's history; the mirror is the undo).

### Owner sequence (Walter; each step reported before the next runs)

1. `brew "cargo-dist"` in the Brewfile + install (one line of go-ahead), so Task 3 runs `dist init` for real.
2. Trigger the history rewrite (§8.3 step 3) after reading Task 4's report; then the controller replaces the original checkout with the rewritten clone and re-runs the suite.
3. Create `WallyMagill/gameday` (public, no template files) and `WallyMagill/homebrew-tap` (public, empty); create the two secrets; `git remote add origin` and the first push (`gh auth switch --user WallyMagill` first).
4. The rc: the controller bumps to `1.0.0-rc.1` and commits; Walter tags and pushes `v1.0.0-rc.1`; CI builds; the four installs are verified on a clean user account on this Mac (a second macOS user, or `sudo -u`), receipts in §9.
5. The NFL week-one window (§8.5), sequenced by the calendar: `scripts/capture-replay.sh nfl 60` around a score; whether `lastPlay.probability` is on the NFL scoreboard (flip `rank::leverage_enabled(Nfl)` if so, with its own commit and test); a full live session in tmux with `GAMEDAY_LOG_REQUESTS=1` and the request rate with catch-ups; a Ghostty screenshot; also wave 1's Saturday CFB replay with a touchdown (`cfb 30`). A real favorite-score notification and a live status line (wave 4's open receipts) ride the same window.
6. README reviewed by Walter; the 1.0.0 bump + CHANGELOG date commit; Walter tags `v1.0.0`; the launch kit posted when he says.

### Task 5: Receipts and the launch kit

- [ ] §9 `### Wave 6` block as the steps land (package list and size, the rewrite's before/after, the rc installs, the NFL window's four receipts, the tag); the launch kit (§8.6) drafted into the session scratchpad and the vault's Game Day page: the GIF, the Terminal Trove submission (name, one-liner, repo, GIF), the r/rust and r/commandline posts (title: "gameday: a terminal sports board that ranks live games by watchability, nine leagues, no account"), the Show HN draft with the positioning line and `--once --json` as the hook. Commit `docs(v4): wave 6 receipts`.

## Self-review against spec §8

§8.1 package → Task 1 (the `assets/candidates` and `fixtures/nfl_standings.json` exceptions are spelled out); §8.2 pipeline → Task 3 (+ owner steps 1, 3, 4); §8.3 history → Task 4 (+ owner step 2); §8.4 README → Task 2; §8.5 NFL window → owner step 5 (calendar-sequenced); §8.6 launch kit → Task 5. O1–O6, H3 (already fixed: `USER_AGENT` names `WallyMagill/gameday`), T3, Q3 (the multi-hour budget receipt comes from step 5's session) each map to one of the above. The DoD (history rewritten and pushed; rc installed four ways; NFL fixtures and the leverage check; README reviewed; the budget receipt; `v1.0.0` tagged) is owner steps 2–6 with Tasks 1–5 as their inputs. Test-count ledger: 646 → 646 (T1 rewrites comments and a CI file; T2/T3 are docs and YAML); measured numbers win.
