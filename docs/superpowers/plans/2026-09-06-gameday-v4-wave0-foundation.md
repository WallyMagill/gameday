# gameday v4 Wave 0 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put a green two-OS pipeline, current dependencies, a license, repo metadata, and a split `app` module under every later wave, with zero behavior change.

**Architecture:** Mechanical work only. rustfmt lands in one commit; dependency upgrades are compiler-driven with the 526-test suite as the safety net; `src/app/mod.rs` splits into `merge`, `order`, `persist`, `draw`, `keys/*`, and `tests` as child modules of `app` (child modules can reach `App`'s private fields, so no field visibility changes); CI, license, and metadata files are added verbatim from this plan.

**Tech Stack:** Rust 2021, ratatui 0.30, crossterm 0.29, ureq 3, tui-big-text 0.8, dirs 7, signal-hook 0.3, GitHub Actions, cargo-deny, cargo-audit.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§2 Wave 0, §8.1 exclude list, §9 receipts).

## Global Constraints

- Work on branch `v4-ship`. Commit after every task; never push (Walter pushes).
- Behavior is unchanged by this wave. The test count is 526 before the wave; every task ends with `cargo test` green and the count reported. New tests only add.
- Dependency versions (spec §2.2): `ratatui = "0.30"`, `tui-big-text = "0.8"`, `crossterm = "0.29"`, `ureq = "3"`, `dirs = "7"`, `signal-hook = "0.3"` (unix only). Exactly one `crossterm` in `cargo tree`.
- Timeouts stay at `HTTP_TIMEOUT` (10 s) for connect and body read.
- License: `MIT OR Apache-2.0`. Repository URL: `https://github.com/WallyMagill/gameday`. Version stays `0.1.0` until wave 6.
- Package: `cargo package` output under 5 MB (spec §8.1). `assets/` must be packaged (it is `include_str!`'d). `fixtures/nfl_standings.json` must be packaged (`src/dump.rs` includes it).
- Every `gh` command in this repo is preceded by `gh auth switch --user WallyMagill`. This wave needs none.
- Commit messages end with the session trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `Cargo.toml` | metadata, upgraded deps, `exclude`, `rust-version` |
| `.rustfmt.toml` | none (defaults); the format commit hash goes in `.git-blame-ignore-revs` |
| `.git-blame-ignore-revs` | the rustfmt commit |
| `LICENSE-MIT`, `LICENSE-APACHE` | license texts |
| `.github/workflows/ci.yml` | fmt, clippy, test matrix, msrv, audit, deny, package |
| `.github/dependabot.yml` | weekly cargo + actions |
| `deny.toml` | cargo-deny policy |
| `AGENTS.md`, `CLAUDE.md`, `CHANGELOG.md` | repo agent rules, changelog |
| `src/provider/espn.rs` | ureq 3 client |
| `src/main.rs` | signal flag → `should_quit` |
| `src/app/mod.rs` | `App` struct, `new`, small accessors, `Tab`, constants |
| `src/app/merge.rs` | `apply_boards`, `merge_*`, `note_*`, `aux_error`, `stale_after`, `stats_target`, `standings_target`, `league_of` |
| `src/app/order.rs` | `RankFingerprint`, `live_all`, `maybe_reorder`, `force_reorder` |
| `src/app/persist.rs` | `persist_config`, `persist_pins`, `persist_pins_quiet`, `set_config_error` |
| `src/app/draw.rs` | `draw`, `draw_frame` |
| `src/app/keys/mod.rs` | `on_key` dispatcher, declares the per-view key modules |
| `src/app/keys/{board,zoom,tv,config,standings,feed,theme}.rs` | each view's `on_key_*` and cursor helpers |
| `src/app/tests.rs` | the existing `app` test module, moved verbatim |
| `src/theme.rs` | unchanged API; a no-panic proof test added |

---

### Task 1: rustfmt the tree

**Files:**
- Modify: every `src/**/*.rs`, `tests/*.rs` (formatting only)
- Create: `.git-blame-ignore-revs`

**Interfaces:**
- Produces: a formatted tree that `cargo fmt --check` accepts; all later tasks assume it.

- [ ] **Step 1: Confirm the baseline is green and count tests**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `526 tests` (all `ok`).

- [ ] **Step 2: Format**

Run: `cargo fmt && cargo fmt --check && echo FMT_CLEAN`
Expected: `FMT_CLEAN`.

- [ ] **Step 3: Verify nothing but whitespace changed**

Run: `git diff --stat | tail -1 && cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: many files changed; `526 tests`.

- [ ] **Step 4: Commit the format, then record its hash**

```bash
git add -A
git commit -q -m "style: rustfmt the tree (no behavior change)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
HASH=$(git rev-parse HEAD)
printf '# Whitespace-only commits, skipped by git blame when configured with:\n#   git config blame.ignoreRevsFile .git-blame-ignore-revs\n%s  style: rustfmt the tree\n' "$HASH" > .git-blame-ignore-revs
git config blame.ignoreRevsFile .git-blame-ignore-revs
git add .git-blame-ignore-revs
git commit -q -m "chore: ignore the rustfmt commit in blame

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 2: Upgrade ratatui, tui-big-text, crossterm, dirs

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, any `src/**/*.rs` the compiler names

**Interfaces:**
- Produces: a tree that builds on ratatui 0.30 / crossterm 0.29 with one `crossterm` in the graph.

- [ ] **Step 1: Bump the versions**

In `Cargo.toml` replace the four lines:

```toml
crossterm = "0.29"
ratatui = "0.30"
dirs = "7"
tui-big-text = "0.8"
```

- [ ] **Step 2: Update the lockfile and build; let the compiler list the API breaks**

Run: `cargo update -p ratatui -p crossterm -p dirs -p tui-big-text && cargo build --all-targets 2>&1 | grep -E '^(error|warning)' | sort | uniq -c | sort -rn | head -30`
Expected: a short list of errors naming renamed items. Known renames in this range: `Frame::size()` → `Frame::area()` (already not used); `ratatui::prelude` unchanged; `tui_big_text::BigText::builder()` unchanged. Fix each error at the site the compiler names, changing nothing else. If an error is about two `crossterm` versions (a type from `crossterm 0.28` meeting `crossterm 0.29`), go to Step 3 first.

- [ ] **Step 3: Assert one crossterm in the tree**

Run: `cargo tree -d 2>/dev/null | grep -A2 '^crossterm' ; cargo tree -i crossterm | head -5`
Expected: `cargo tree -d` prints nothing for crossterm (no duplicates). If ratatui 0.30 pulls a different crossterm than 0.29, set `crossterm` in `Cargo.toml` to the version `cargo tree -i crossterm` shows ratatui using, so there is exactly one.

- [ ] **Step 4: Run the suite and clippy**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}' && cargo clippy --all-targets 2>&1 | grep -c '^warning' `
Expected: `526 tests`, then `0` warnings. Fix any new clippy lint at its site (the 0.30 upgrade commonly surfaces `needless_borrow` on `Rect`); do not `allow` anything.

- [ ] **Step 5: Drive the release binary in tmux to catch a runtime regression the tests cannot**

```bash
cargo build --release
tmux kill-session -t w0 2>/dev/null; tmux new-session -d -s w0 -x 120 -y 36
tmux send-keys -t w0 "./target/release/gameday --demo" Enter; sleep 4
tmux capture-pane -t w0 -p | head -20
tmux send-keys -t w0 z; sleep 1; tmux capture-pane -t w0 -p | sed -n 2,3p
tmux send-keys -t w0 Escape v; sleep 1; tmux capture-pane -t w0 -p | tail -1
tmux send-keys -t w0 Escape q; sleep 1; tmux kill-session -t w0 2>/dev/null
```
Expected: the demo board draws with the big digits, `z` shows `OVERVIEW │ PLAYS │ STATS`, `v` shows the TV footer `space lock  n next  esc board  q quit`, and the terminal is restored after `q` (the shell prompt is usable).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -q -m "build: ratatui 0.30, crossterm 0.29, tui-big-text 0.8, dirs 7

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 3: ureq 3 migration

**Files:**
- Modify: `Cargo.toml` (`ureq = "3"`), `src/provider/espn.rs` (`EspnProvider::new`, `http`)

**Interfaces:**
- Consumes: `ProviderError::Http { status, key, url, detail }`, `Fetched::{Body, NotModified}`, `HTTP_TIMEOUT`, `USER_AGENT` (unchanged).
- Produces: the same `http(&self, url, etag) -> Result<Fetched, ProviderError>` contract; proxy env vars honored (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`).

- [ ] **Step 1: Write the failing test for the 304 arm and the timeout wording**

Append to the `tests` module in `src/provider/espn.rs`:

```rust
    /// ureq 3 reports non-2xx as `Error::StatusCode(u16)`; the 304 arm must
    /// keep mapping to `Fetched::NotModified`, and a transport error must
    /// keep `status: 0` with the detail text (which `short()` reads for the
    /// word "timeout").
    #[test]
    fn transport_errors_keep_status_zero_and_the_detail_text() {
        let dir = tmp("transport");
        let p = provider(&dir);
        // An unroutable address: TEST-NET-1 is reserved and never answers.
        // 10s connect timeout is the real HTTP_TIMEOUT; the test tolerates it.
        let err = p.http("http://192.0.2.1:9/never", None).unwrap_err();
        match err {
            ProviderError::Http { status, detail, url, .. } => {
                assert_eq!(status, 0, "transport failures carry status 0");
                assert!(!detail.is_empty(), "detail must carry the transport error text");
                assert_eq!(url, "http://192.0.2.1:9/never");
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Bump ureq and watch the build fail on the old API**

Set `ureq = "3"` in `Cargo.toml`. Run: `cargo update -p ureq && cargo build 2>&1 | grep -c '^error'`
Expected: a non-zero count (the `AgentBuilder`, `set`, `into_string`, `Error::Status` sites).

- [ ] **Step 3: Rewrite the client**

Replace the `agent` field type and the two methods in `src/provider/espn.rs`:

```rust
pub struct EspnProvider {
    pub cache_dir: PathBuf,
    /// Local UTC offset, read once on the main thread (`text::startup_offset`)
    /// and carried here: the poll thread can't read the TZ database itself.
    pub offset: time::UtcOffset,
    agent: ureq::Agent,
}
```

```rust
impl EspnProvider {
    pub fn new(cache_dir: PathBuf, offset: time::UtcOffset) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(HTTP_TIMEOUT))
            .timeout_recv_body(Some(HTTP_TIMEOUT))
            .user_agent(USER_AGENT)
            // Non-2xx surfaces as `Error::StatusCode(code)` so the 304 and
            // the error arms below stay distinct from a body read.
            .http_status_as_error(true)
            // HTTPS_PROXY / ALL_PROXY / NO_PROXY from the environment. This
            // is what makes `HTTPS_PROXY=http://127.0.0.1:9 gameday` the
            // offline test (spec §2.2, §9).
            .proxy(ureq::Proxy::try_from_env())
            .build();
        Self {
            cache_dir,
            offset,
            agent: ureq::Agent::new_with_config(config),
        }
    }

    fn etag_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.etag"))
    }

    pub(crate) fn http(&self, url: &str, etag: Option<&str>) -> Result<Fetched, ProviderError> {
        let mut req = self.agent.get(url).header("Accept", "application/json");
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        match req.call() {
            // `key` is left empty here and filled in by `stamp`: this layer
            // only knows the URL it asked for.
            Ok(mut r) => {
                let etag = r
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let body = r.body_mut().read_to_string().map_err(|e| ProviderError::Http {
                    status: 0,
                    key: String::new(),
                    url: url.into(),
                    detail: format!("body read: {e}"),
                })?;
                Ok(Fetched::Body { body, etag })
            }
            Err(ureq::Error::StatusCode(304)) => Ok(Fetched::NotModified),
            Err(ureq::Error::StatusCode(code)) => Err(ProviderError::Http {
                status: code,
                key: String::new(),
                url: url.into(),
                detail: String::new(),
            }),
            Err(e) => Err(ProviderError::Http {
                status: 0,
                key: String::new(),
                url: url.into(),
                detail: e.to_string(),
            }),
        }
    }
```

`http` becomes `pub(crate)` only so the new test can call it; nothing outside the provider uses it.

- [ ] **Step 4: Build, run the provider tests, run the whole suite**

Run: `cargo test --release provider:: 2>&1 | grep -E '^test result'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: provider tests `ok`; `527 tests`.

- [ ] **Step 5: Prove the proxy env var is honored (the offline test the review could not run)**

```bash
cargo build --release
S=$(mktemp -d)
tmux kill-session -t w0 2>/dev/null; tmux new-session -d -s w0 -x 120 -y 36
tmux send-keys -t w0 "HTTPS_PROXY=http://127.0.0.1:9 ./target/release/gameday --config-dir $S" Enter
sleep 15; tmux capture-pane -t w0 -p | sed -n 1,3p; tmux capture-pane -t w0 -p | tail -2
tmux send-keys -t w0 q; sleep 1; tmux kill-session -t w0 2>/dev/null; rm -rf "$S"
```
Expected: the header carries the offline/error chip (`ESPN unreachable nfl scoreboard` or similar) and no board rows; the footer shows a retry countdown. Save the two captured fragments; Task 15 records them.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/provider/espn.rs
git commit -q -m "build: ureq 3 — config builder, StatusCode arms, proxy from env

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 4: SIGTERM and SIGHUP restore the terminal

**Files:**
- Modify: `Cargo.toml`, `src/main.rs` (`run_ui` loop)

**Interfaces:**
- Produces: `fn install_signal_flag() -> Arc<AtomicBool>` in `main.rs`; the UI loop sets `app.should_quit` when it is raised, so the existing `RestoreTerminal` Drop guard runs.

- [ ] **Step 1: Add the dependency (unix only)**

Append to `Cargo.toml`:

```toml
[target.'cfg(unix)'.dependencies]
signal-hook = "0.3"
```

- [ ] **Step 2: Add the flag installer next to `install_panic_hook` in `src/main.rs`**

```rust
/// SIGTERM / SIGHUP (a closed terminal, `kill`, a session manager) set this
/// flag; the UI loop treats it as `q`, so the one restore path runs and the
/// terminal is never left in raw mode. Ctrl-C arrives as a key event through
/// crossterm and is handled by the keymap, not here.
#[cfg(unix)]
fn install_signal_flag() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP] {
        // Registration only fails for signals that cannot be caught; TERM
        // and HUP can, and a failure here would just leave the old behavior.
        let _ = signal_hook::flag::register(sig, Arc::clone(&flag));
    }
    flag
}

#[cfg(not(unix))]
fn install_signal_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
```

- [ ] **Step 3: Wire it into `run_ui`**

At the top of `run_ui`, after `install_panic_hook();`:

```rust
    let term_signal = install_signal_flag();
```

Inside the `'ui` loop, just before `if app.should_quit {`:

```rust
        if term_signal.load(Ordering::Relaxed) {
            app.should_quit = true;
        }
```

- [ ] **Step 4: Build and test**

Run: `cargo build --release 2>&1 | grep -E '^(warning|error)' ; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: no warnings; `527 tests`.

- [ ] **Step 5: Receipt — `kill -TERM` leaves a working shell**

```bash
tmux kill-session -t w0 2>/dev/null; tmux new-session -d -s w0 -x 100 -y 30
tmux send-keys -t w0 "./target/release/gameday --demo; echo EXIT=\$?; stty -a | head -1" Enter
sleep 3
PID=$(pgrep -f 'target/release/gameday --demo' | head -1); kill -TERM "$PID"; sleep 2
tmux capture-pane -t w0 -p | tail -4
tmux send-keys -t w0 "echo TYPED_OK" Enter; sleep 1; tmux capture-pane -t w0 -p | tail -2
tmux kill-session -t w0
```
Expected: `EXIT=0`, the `stty -a` line does not contain `-echo` or `-icanon` (cooked mode), and `TYPED_OK` echoes. Save the capture for Task 15.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -q -m "fix: SIGTERM/SIGHUP quit through the restore path (signal-hook)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 5: Declare and verify the MSRV

**Files:**
- Modify: `Cargo.toml` (`rust-version`)

**Interfaces:**
- Produces: `package.rust-version`, consumed by the CI `msrv` job in Task 8.

- [ ] **Step 1: Find the floor the dependency graph demands**

Run:
```bash
cargo metadata --format-version 1 | jq -r '[.packages[].rust_version | select(. != null)] | unique | sort_by(split(".") | map(tonumber)) | last'
```
Expected: a version like `1.85.0`. Call it `DEP_MSRV`.

- [ ] **Step 2: Try it; bump until the crate itself builds**

The code uses `u64::is_multiple_of` (stable since 1.87), so the floor is at least 1.87 regardless of Step 1.

```bash
MSRV=1.87.0   # max(DEP_MSRV, 1.87.0)
rustup toolchain install $MSRV --profile minimal
cargo +$MSRV check --locked --all-targets 2>&1 | tail -3
```
Expected: `Finished`. If it fails on a language feature, raise `MSRV` by one minor and repeat; record the feature that forced it in the `Cargo.toml` comment.

- [ ] **Step 3: Declare it**

In `[package]` add:

```toml
# Floor: `u64::is_multiple_of` (1.87) and the dependency graph's own
# rust-version fields; verified by the CI `msrv` job.
rust-version = "1.87"
```

(Use the version Step 2 settled on.)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -q -m "build: declare rust-version

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 6: Cargo metadata, package exclude, User-Agent URL

**Files:**
- Modify: `Cargo.toml`, `src/provider/espn.rs` (`USER_AGENT`)

**Interfaces:**
- Produces: a publishable manifest; `cargo package` output under 5 MB.

- [ ] **Step 1: Write the failing test for the User-Agent URL**

In `src/provider/espn.rs` tests, replace `user_agent_names_the_project_and_a_contact` with:

```rust
    #[test]
    fn user_agent_names_the_project_and_the_public_repo() {
        assert!(USER_AGENT.starts_with("gameday/"));
        assert!(
            USER_AGENT.ends_with("(+https://github.com/WallyMagill/gameday)"),
            "the contact URL must be the public repo: {USER_AGENT}"
        );
    }
```

Run: `cargo test --release user_agent 2>&1 | grep -E 'test result|panicked' | head -2`
Expected: FAIL (the UA still says `game-day`).

- [ ] **Step 2: Fix the constant**

```rust
pub const USER_AGENT: &str = concat!(
    "gameday/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/WallyMagill/gameday)"
);
```

- [ ] **Step 3: Replace the `[package]` table**

```toml
[package]
name = "gameday"
version = "0.1.0"
edition = "2021"
# Floor: `u64::is_multiple_of` (1.87) and the dependency graph's own
# rust-version fields; verified by the CI `msrv` job.
rust-version = "1.87"
description = "Terminal sports board: the game worth watching is at the top. Nine leagues, no account, one binary."
license = "MIT OR Apache-2.0"
repository = "https://github.com/WallyMagill/gameday"
readme = "README.md"
keywords = ["sports", "tui", "terminal", "scores", "espn"]
categories = ["command-line-utilities"]
# Everything the binary embeds stays in (assets/, fixtures/nfl_standings.json
# via src/dump.rs). Docs, research, tooling and captures stay out.
exclude = [
    "docs/",
    "fixtures/replay/",
    "scripts/",
    "tools/",
    ".github/",
    "*.tape",
    ".git-blame-ignore-revs",
]
```

(Keep the `rust-version` Task 5 chose.)

- [ ] **Step 4: Measure the package**

```bash
cargo package --allow-dirty --no-verify 2>&1 | tail -2
ls -l target/package/gameday-0.1.0.crate | awk '{print $5 " bytes"}'
cargo package --list --allow-dirty | grep -c . 
```
Expected: `.crate` under 5,000,000 bytes. If it is over: add `"tests/"` and `"fixtures/live/"` and every `fixtures/*.json` except `nfl_standings.json` to `exclude` (tests and their fixtures leave together, spec §8.1), re-measure, and note the final size in the commit message. Then `cargo package --allow-dirty` (with verify) must print `Finished`.

- [ ] **Step 5: Tests and commit**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `527 tests`.

```bash
git add Cargo.toml src/provider/espn.rs
git commit -q -m "build: crate metadata, package exclude list, public repo in the User-Agent

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 7: License files

**Files:**
- Create: `LICENSE-MIT`, `LICENSE-APACHE`

- [ ] **Step 1: Write `LICENSE-MIT`**

```
MIT License

Copyright (c) 2026 Walter Magill

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Fetch the canonical Apache text and verify it**

```bash
curl -sSfL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE-APACHE
head -3 LICENSE-APACHE; grep -c 'Apache License' LICENSE-APACHE; wc -l LICENSE-APACHE
```
Expected: the first line reads `Apache License`, second `Version 2.0, January 2004`, and the file is about 200 lines. The canonical text carries no copyright line; the `Cargo.toml` `license` field and the README license line name the holder.

- [ ] **Step 3: Commit**

```bash
git add LICENSE-MIT LICENSE-APACHE
git commit -q -m "chore: MIT OR Apache-2.0 license texts

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 8: CI, cargo-deny, Dependabot

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/dependabot.yml`, `deny.toml`

**Interfaces:**
- Consumes: `rust-version` (Task 5), the package exclude (Task 6).
- Produces: the pipeline every later wave's DoD cites. It cannot run until Walter pushes; each job's command is run locally here so the first remote run is green.

- [ ] **Step 1: Write `.github/workflows/ci.yml`**

```yaml
name: ci

on:
  push:
    branches: [main, v4-ship]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --check

  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --all-targets --locked -- -D warnings

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --locked

  msrv:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - id: msrv
        run: echo "version=$(grep '^rust-version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')" >> "$GITHUB_OUTPUT"
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ steps.msrv.outputs.version }}
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --locked --all-targets

  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check licenses advisories sources

  package:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo package --locked
      - name: crate size under 5 MB
        run: |
          size=$(stat -c %s target/package/*.crate)
          echo "crate size: $size bytes"
          test "$size" -lt 5000000
```

- [ ] **Step 2: Write `deny.toml`**

```toml
# cargo-deny policy. `cargo deny check licenses advisories sources` runs in CI.

[graph]
all-features = true

[advisories]
version = 2
yanked = "deny"
ignore = []

[licenses]
version = 2
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Zlib",
    "MPL-2.0",
    "CC0-1.0",
]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [ ] **Step 3: Write `.github/dependabot.yml`**

```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    groups:
      cargo-minor:
        update-types: [minor, patch]
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
```

- [ ] **Step 4: Run every job's command locally**

```bash
cargo install cargo-deny cargo-audit --locked 2>&1 | tail -2
cargo fmt --check && echo FMT_OK
RUSTFLAGS="-D warnings" cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -1
cargo test --locked 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'
cargo deny check licenses advisories sources 2>&1 | tail -3
cargo audit 2>&1 | tail -3
cargo package --locked 2>&1 | tail -1 && stat -f %z target/package/*.crate
```
Expected: `FMT_OK`; clippy `Finished`; `527 tests`; deny prints `advisories ok, licenses ok, sources ok`; audit prints no vulnerabilities (if an advisory fires for a transitive crate, add it to `[advisories].ignore` with a one-line reason and open a note in `CHANGELOG.md` Unreleased under "Known"); the crate size under 5,000,000. If `deny` rejects a license not in the allow list, add it only if it is a permissive license; otherwise stop and report.

- [ ] **Step 5: Commit**

```bash
git add .github deny.toml
git commit -q -m "ci: fmt, clippy, test matrix, msrv, audit, deny, package size; dependabot

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 9: AGENTS.md, CLAUDE.md, CHANGELOG.md

**Files:**
- Create: `AGENTS.md`, `CLAUDE.md`, `CHANGELOG.md`

- [ ] **Step 1: Write `AGENTS.md`**

```markdown
# gameday

Terminal sports board in Rust (ratatui). One ranked list: the game most worth
watching leads it. Nine leagues from unofficial ESPN JSON, polled every 15s
while anything is live. No account, no key.

## Build, test, run

- `cargo build --release` → `target/release/gameday`
- `cargo test` (fast; no network, no sleeps) · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --check`
- `gameday --demo` scripted slate, no network · `gameday dump` capture gallery into `out/` · `gameday frame …` one surface (see `docs/dev.md`)
- Config lives in `~/.config/gameday/` (`--config-dir` overrides). Tests never touch it: they use temp dirs.

## Standing rules (the reasons, not the ruling numbers)

- **Structure over parsing.** Read the field ESPN publishes; never derive state from prose. Presentation-only normalization is allowed and labeled.
- **The board reorders only on a real event** (a score, a status change, a chip flipping, a leverage band crossing). A clock that merely advanced never moves a row. New ranking inputs enter the fingerprint set deliberately.
- **One score formatter; logos never move a digit.** The hero and the takeover agree on every digit cell.
- **Quadrant blocks only** (`▀▄█`) for digits and marks: every mono font has them; sextants tofu.
- **Numbers carry receipts.** A timeout, cap, or weight is measured or says it is a guess and why.
- **Failures name the value, the expectation, and the knob.**
- **No new request load without a measured budget line.** Scoreboards every 15s live / 60s idle; summaries for the zoomed game and one-shot catch-ups only.

## Design work

Looks are decided from rendered PNGs, never in prose: `docs/design-loop.md`. Specs and plans live in `docs/superpowers/`; decisions are recorded there as directions (what won, why, what reopens it).

## Repo hygiene

- Branch from `main`; the push, merge, tag, and publish are Walter's.
- `git config blame.ignoreRevsFile .git-blame-ignore-revs` (the rustfmt commit).
- Before any `gh` command: `gh auth switch --user WallyMagill` (this is a personal repo; the machine's default `gh` account is the work one). Switch back afterwards.
- Release procedure: `docs/dev.md`.
```

- [ ] **Step 2: Write `CLAUDE.md`**

```markdown
@AGENTS.md
```

- [ ] **Step 3: Write `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to gameday are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- ratatui 0.30, crossterm 0.29, ureq 3 (proxy env vars are now honored), dirs 7, tui-big-text 0.8.
- SIGTERM and SIGHUP quit through the normal restore path; the terminal is never left in raw mode.
- Licensed MIT OR Apache-2.0.
```

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md CLAUDE.md CHANGELOG.md
git commit -q -m "docs: AGENTS.md, CLAUDE.md, CHANGELOG.md

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 10: Split `app/mod.rs` — persist, order, merge, draw

**Files:**
- Create: `src/app/persist.rs`, `src/app/order.rs`, `src/app/merge.rs`, `src/app/draw.rs`
- Modify: `src/app/mod.rs`

**Interfaces:**
- Consumes: the private `App` fields (`last_scores`, `rank_fingerprints`, `flashes`, `alerts`, `dated_boards`, `frame_cache`), reachable from child modules without visibility changes.
- Produces: the same public methods on `App` at the same names; private helpers called across files become `pub(super)`.

How moving works in this codebase: each new file is `use super::*;` at the top (pulling `App`, `Tab`, the constants, and every `use` in `mod.rs` that is re-exported as `pub(super) use` or simply repeated), then one `impl App { … }` block containing the moved methods cut verbatim from `mod.rs`. Because `mod.rs`'s `use` lines are private to `mod.rs`, repeat the imports each file needs (the compiler lists the missing ones). Private methods that another file in `app/` calls get `pub(super)`; methods called from outside `app/` are already `pub` and stay so.

- [ ] **Step 1: `persist.rs`**

Create `src/app/persist.rs`:

```rust
//! Config and pin persistence, and the config-error gate that refuses saves
//! while `config.toml` does not parse.

use super::App;
use crate::config::save_pins;

impl App {
    // moved verbatim from mod.rs: persist_config, persist_pins,
    // persist_pins_quiet, set_config_error
}
```

Cut the four methods out of `impl App` in `mod.rs` and paste them into the block. In `mod.rs` add `mod persist;` under `mod derive;`. Run `cargo build 2>&1 | grep -E '^error' | head`; add the imports the errors name (typically `crate::config::Config`); if a moved method is called from elsewhere in `app/` and was private, mark it `pub(super)`.

- [ ] **Step 2: `order.rs`**

Create `src/app/order.rs` with the `RankFingerprint` type alias (cut from `mod.rs`, made `pub(super) type`), and an `impl App` holding `live_all`, `maybe_reorder`, `force_reorder`, plus any private fingerprint helper defined beside them. Add `mod order;` and `pub(super) use order::RankFingerprint;` is unnecessary if only `order.rs` and the `App` field type use it: change the field to `rank_fingerprints: HashMap<String, order::RankFingerprint>`. Build and fix imports (`crate::rank`, `Status`, `Extras` as the errors name).

- [ ] **Step 3: `merge.rs`**

Create `src/app/merge.rs` with an `impl App` holding, verbatim: `apply_boards`, `merge_dated_board`, `merge_summary`, `stats_target`, `merge_stats`, `note_failure`, `standings_target`, `stale_after`, `merge_standings`, `note_aux_failure`, `clear_aux_error`, `aux_error`, `league_of`. Add `mod merge;`. Build; add imports; `pub(super)` where a sibling calls a private one (`league_of` is used by zoom/tv code, so it becomes `pub(super)`).

- [ ] **Step 4: `draw.rs`**

Create `src/app/draw.rs` with an `impl App` holding `draw` and `draw_frame` verbatim. Add `mod draw;`. Build; the imports are the ratatui layout/widget ones from `mod.rs` plus `crate::views`, `crate::board`, `crate::ticker` as the errors name. Remove from `mod.rs` any `use` that is now unused (the compiler warns; warnings are errors in CI).

- [ ] **Step 5: Suite unchanged**

Run: `cargo build --all-targets 2>&1 | grep -cE '^(warning|error)'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'; wc -l src/app/mod.rs`
Expected: `0`; `527 tests`; `mod.rs` well under 2,000 lines (the tests are still in it until Task 12).

- [ ] **Step 6: Commit**

```bash
git add src/app
git commit -q -m "refactor(app): split persist, order, merge, draw out of app/mod.rs (no behavior change)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 11: Split `app/mod.rs` — `keys/`

**Files:**
- Create: `src/app/keys/mod.rs`, `src/app/keys/board.rs`, `src/app/keys/zoom.rs`, `src/app/keys/tv.rs`, `src/app/keys/config.rs`, `src/app/keys/standings.rs`, `src/app/keys/feed.rs`, `src/app/keys/theme.rs`
- Modify: `src/app/mod.rs`

**Interfaces:**
- Produces: `App::on_key` (public, unchanged signature) in `keys/mod.rs`; each view's handler in its file. `FEED_PAGE_JUMP` moves to `keys/feed.rs`.

- [ ] **Step 1: `keys/mod.rs`**

```rust
//! Key handling, one file per view. `on_key` is the dispatcher `input.rs`
//! calls; each view's handler and its cursor helpers live beside it.

mod board;
mod config;
mod feed;
mod standings;
mod theme;
mod tv;
mod zoom;

use super::App;
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    // moved verbatim from mod.rs: on_key
}
```

Add `mod keys;` to `app/mod.rs`. Cut `on_key` into the block.

- [ ] **Step 2: Per-view files**

Each file is `use super::super::App;` (or `use crate::app::App;`) plus the imports the compiler names, and one `impl App` block. Move verbatim:

- `keys/board.rs`: `on_key_board`, `move_selected`, `toggle_pin`, `toggle_favorite`, `cycle_sort`, `cycle_tab`, `set_tab`, `clamp_selected`, `viewed_date`, `step_viewed_date`, `dated_target`, `tab_list`, `current_tab_index`.
- `keys/zoom.rs`: `on_key_zoom`, `zoom_selected`, `cycle_zoom_tab`, `move_zoom_scroll`, and `zoom_game_id` (and any zoom accessor defined beside it).
- `keys/tv.rs`: `on_key_tv`, `tv_step`, `tv_follow`, `tv_hygiene`, `open_tv`, `tv_next_cut_in`, `tv_shown_in`, `cut_suppressed`, `cut_is_full`.
- `keys/config.rs`: `on_key_config`, `on_key_config_edit`, `move_config_cursor`, `config_remove_favorite`, `config_add_favorite`, `config_cycle`.
- `keys/standings.rs`: `on_key_standings`, `move_standings_scroll`.
- `keys/feed.rs`: `const FEED_PAGE_JUMP` (from `mod.rs`), `on_key_plays_feed`, `move_feed_scroll`.
- `keys/theme.rs`: `open_theme_picker`, `on_key_theme_picker`, `revert_theme_preview`, `move_theme_cursor`, `cycle_theme`.

After each file: `cargo build 2>&1 | grep -E '^error' | head -5`; add imports; mark methods `pub(super)` when a sibling or `merge.rs`/`draw.rs` calls them (the compiler says "private method"). `pub(super)` inside `keys/board.rs` means visible in `keys`; a method `merge.rs` needs must be `pub(in crate::app)` instead. Use `pub(in crate::app)` for anything called from outside `keys/`.

- [ ] **Step 3: Suite unchanged, mod.rs small**

Run: `cargo build --all-targets 2>&1 | grep -cE '^(warning|error)'; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'; wc -l src/app/mod.rs src/app/keys/*.rs`
Expected: `0`; `527 tests`; `mod.rs` ≈ 1,700 lines (struct + `new` + accessors + the still-attached tests).

- [ ] **Step 4: Commit**

```bash
git add src/app
git commit -q -m "refactor(app): key handling into app/keys/<view>.rs (no behavior change)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 12: Move the `app` test module to `app/tests.rs`

**Files:**
- Create: `src/app/tests.rs`
- Modify: `src/app/mod.rs`

**Interfaces:**
- Produces: `#[cfg(test)] mod tests;` in `mod.rs`; the module body unchanged apart from imports.

The spec says tests move with their subject; the `app` test module is one 1,500-line block whose helpers (`team`, `my_game_and_a_better_one`, `ordering_app`, `ranked`, `ord`) are shared across subjects. Moving it whole to `app/tests.rs` keeps `mod.rs` to the struct and constructor, which is the spec's aim; splitting the tests by subject is deferred to whichever feature wave first edits each group. Recorded here so the deviation is visible.

- [ ] **Step 1: Move the block**

Cut everything from the line `#[cfg(test)]` that opens `mod tests {` in `src/app/mod.rs` to the end of the file. Create `src/app/tests.rs` from it: drop the outer `#[cfg(test)] mod tests {` and its closing `}`, dedent one level, keep `use super::*;` as the first line. In `mod.rs` add at the bottom:

```rust
#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Build the tests; fix the paths**

Run: `cargo test --release --no-run 2>&1 | grep -E '^error' | head`
Expected: errors only about items that were private to `mod.rs` and are now in sibling modules (e.g. `RankFingerprint`, `FEED_PAGE_JUMP`). Fix by importing them (`use super::order::RankFingerprint;`, `use super::keys::…` after making the item `pub(super)`), never by weakening visibility beyond `pub(super)`.

- [ ] **Step 3: Suite unchanged**

Run: `cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'; wc -l src/app/mod.rs`
Expected: `527 tests`; `mod.rs` under 400 lines.

- [ ] **Step 4: Commit**

```bash
git add src/app
git commit -q -m "refactor(app): tests to app/tests.rs; mod.rs is the struct and constructor

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 13: Comments say the reason, not the ruling number

**Files:**
- Modify: any `src/**/*.rs` and `tests/*.rs` with a ruling reference in a comment

**Interfaces:** none (comments only).

- [ ] **Step 1: List every reference**

```bash
grep -rnE '^\s*(//|///|//!).*(\bR[0-9]{1,2}\b|spec v[0-9]\.[0-9]|§[0-9]+|\bgap R[0-9]+|\bT[0-9]{1,2} review|\(T[0-9]{1,2}\)|\bTask [0-9]+\b)' src tests | wc -l
grep -rnE '^\s*(//|///|//!).*(\bR[0-9]{1,2}\b|spec v[0-9]\.[0-9]|§[0-9]+|\bgap R[0-9]+|\bT[0-9]{1,2} review|\(T[0-9]{1,2}\)|\bTask [0-9]+\b)' src tests > /tmp/refs.txt; head -40 /tmp/refs.txt
```
Expected: a count (the review saw dozens) and the list.

- [ ] **Step 2: Rewrite each by rule**

Rule: delete the pointer, keep the reason, keep any receipt (a measured number, a fixture name, a date). Two examples of the transformation:

Before:
```rust
// Score-change flash fires ONLY here — from data. A first sighting
// (startup, new game) seeds last_scores without flashing.
```
(no reference: unchanged)

Before:
```rust
/// One live game's ordering identity (R24): away score, home score, status,
/// the hot flag, and soccer's on-field count. Two equal fingerprints mean
/// nothing the board sorts on has moved, so the order is left alone.
```
After:
```rust
/// One live game's ordering identity: away score, home score, status, the
/// hot flag, and soccer's on-field count. Two equal fingerprints mean
/// nothing the board sorts on has moved, so the order is left alone — a
/// board that re-sorted on every clock tick would slide out from under the
/// eye, so only a change in one of these is an event.
```

Before:
```rust
// (verified `14` for USC 2026-08-31). None for pro leagues and unranked teams.
```
(a receipt, not a ruling: unchanged)

Before:
```rust
// 120s = the two-minute warning window, Q2/Q4 only. An
// unparseable clock (None) never triggers this — only a
// confirmed reading under 2:00 does (gap R23).
```
After:
```rust
// 120s = the two-minute warning window, Q2/Q4 only. An unparseable
// clock (None) never triggers this — only a confirmed reading under
// 2:00 does; a bad clock string once inflated lateness and led the
// board, which is why the parse returns None instead of 0.
```

Where a comment only says `(R49)` with the reason already written beside it, delete the tag. Where a `spec v3.4 §3` pointer is the only explanation, write the one-sentence reason from the spec's own text.

- [ ] **Step 3: Verify zero references remain and the build is clean**

Run: `grep -rnE '^\s*(//|///|//!).*(\bR[0-9]{1,2}\b|spec v[0-9]\.[0-9]|§[0-9]+|\bgap R[0-9]+|\bT[0-9]{1,2} review|\(T[0-9]{1,2}\)|\bTask [0-9]+\b)' src tests | wc -l; cargo test --release 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'`
Expected: `0`; `527 tests`. (Identifiers such as `T1_CHIP_W` are not comments and are untouched.)

- [ ] **Step 4: Commit**

```bash
git add src tests
git commit -q -m "docs(code): comments carry the reason, not the ruling id

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 14: theme.rs — prove a user file cannot panic

**Files:**
- Modify: `src/theme.rs` (tests module only)

**Interfaces:** none.

The review counted 27 unwrap/index sites in `theme.rs`. Reading them: the array indexes are on fixed-size color arrays, and every `expect`/`panic!` is on a compile-time constant (built-in and candidate TOML, `League::ALL`) with a message saying so. User files already flow through `parse_theme` → `Result<_, String>` with messages naming the key and the expected form. So the spec's `ThemeError` type is not needed (YAGNI: the strings already name file, key, and form); what is missing is proof. This task adds the proof.

- [ ] **Step 1: Write the test**

Append to the `tests` module in `src/theme.rs`:

```rust
    /// Every malformed shape a user could write must come back as an error
    /// line, never a panic. The test passing IS the proof: a panic here
    /// fails it. Each case is a real mistake, not fuzz.
    #[test]
    fn malformed_user_themes_are_errors_not_panics() {
        let cases: [(&str, &str); 9] = [
            ("empty file", ""),
            ("not toml", "this is not toml at all ["),
            ("no name", "[palette]\nbg = \"#000000\"\n"),
            ("empty name", "name = \"\"\n[palette]\nbg = \"#000000\"\n"),
            ("missing palette", "name = \"x\"\n"),
            ("short hex", "name = \"x\"\n[palette]\nbg = \"#12\"\nfg = \"#ffffff\"\nbright = \"#ffffff\"\nmuted = \"#888888\"\ndim = \"#444444\"\nborder = \"#333333\"\nlive = \"#ff0000\"\ngreen = \"#00ff00\"\ncyan = \"#00ffff\"\nmagenta = \"#ff00ff\"\nstar = \"#ffaa00\"\n"),
            ("unknown league", "name = \"x\"\n[palette]\nbg = \"#000000\"\nfg = \"#ffffff\"\nbright = \"#ffffff\"\nmuted = \"#888888\"\ndim = \"#444444\"\nborder = \"#333333\"\nlive = \"#ff0000\"\ngreen = \"#00ff00\"\ncyan = \"#00ffff\"\nmagenta = \"#ff00ff\"\nstar = \"#ffaa00\"\n[palette.league]\nxfl = \"#123456\"\n"),
            ("bad role", "name = \"x\"\n[palette]\nbg = \"#000000\"\nfg = \"#ffffff\"\nbright = \"#ffffff\"\nmuted = \"#888888\"\ndim = \"#444444\"\nborder = \"#333333\"\nlive = \"#ff0000\"\ngreen = \"#00ff00\"\ncyan = \"#00ffff\"\nmagenta = \"#ff00ff\"\nstar = \"#ffaa00\"\n[roles]\nground = \"nope\"\n"),
            ("bad team scope", "name = \"x\"\n[palette]\nbg = \"#000000\"\nfg = \"#ffffff\"\nbright = \"#ffffff\"\nmuted = \"#888888\"\ndim = \"#444444\"\nborder = \"#333333\"\nlive = \"#ff0000\"\ngreen = \"#00ff00\"\ncyan = \"#00ffff\"\nmagenta = \"#ff00ff\"\nstar = \"#ffaa00\"\n[roles]\nteam = \"everywhere\"\n"),
        ];
        for (label, text) in cases {
            let err = parse_theme(text).expect_err(label);
            assert!(!err.is_empty(), "{label}: the error must say something");
        }
        // Through the directory loader too: one broken file beside one good
        // built-in copy must yield one entry and one error line naming the file.
        let dir = std::env::temp_dir().join(format!("gd-theme-nopanic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes/broken.toml"), "name = \"\"\n").unwrap();
        std::fs::write(
            dir.join("themes/copy.toml"),
            BUILTIN_TOML[0].replace("name = \"broadcast\"", "name = \"copy\""),
        )
        .unwrap();
        let (entries, errors) = load_user_themes(&dir);
        assert_eq!(entries.len(), 1, "the good file loads");
        assert_eq!(errors.len(), 1, "the broken file is one error line");
        assert!(errors[0].contains("broken.toml"), "the error names the file: {}", errors[0]);
        std::fs::remove_dir_all(&dir).ok();
    }
```

If a palette key name in the cases does not match `PALETTE_KEYS` (`bg fg bright muted dim border live green cyan magenta star`), the test's own error message names the missing key; adjust the case text, not the parser.

- [ ] **Step 2: Run it**

Run: `cargo test --release malformed_user_themes 2>&1 | grep -E 'test result|panicked'`
Expected: `ok`. If any case panics instead of erroring, that site is a real bug: convert it to an `Err` naming the key and the expected form, and keep the test.

- [ ] **Step 3: Commit**

```bash
git add src/theme.rs
git commit -q -m "test(theme): malformed user files are errors, never panics

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

### Task 15: Wave 0 receipts into the spec

**Files:**
- Modify: `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§9), `CHANGELOG.md`

- [ ] **Step 1: Final DoD run**

```bash
cargo fmt --check && echo FMT_OK
cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -1
cargo test --locked 2>&1 | grep -E '^test result' | awk '{s+=$4} END {print s " tests"}'
cargo deny check licenses advisories sources 2>&1 | tail -1
cargo package --locked 2>&1 | tail -1; stat -f %z target/package/*.crate
cargo tree -d | grep -c crossterm
grep -E '^(ratatui|crossterm|ureq|dirs|tui-big-text)' Cargo.toml
wc -l src/app/mod.rs src/app/*.rs src/app/keys/*.rs | tail -1
```
Expected: `FMT_OK`; clippy `Finished`; `528 tests`; deny ok; crate under 5,000,000; `0` duplicate crossterm; the five version lines; total line count unchanged within ±5% of the pre-split `mod.rs`.

- [ ] **Step 2: Append to §9 of the spec**

Under `## §9 Verification`, add:

```markdown
### Wave 0 — landed <date>

- Suite: 526 → 528 tests (transport-error arm, malformed-theme proof); fmt, clippy `-D warnings`, deny, package (<crate bytes> bytes) all green locally; CI file runs the same commands and awaits the first push.
- Deps: ratatui 0.30.x, crossterm 0.29.x, ureq 3.x, dirs 7.x, tui-big-text 0.8.x; one crossterm in the graph; rust-version <MSRV>.
- `kill -TERM` receipt: <paste the three captured lines from Task 4 step 5>.
- Offline receipt (`HTTPS_PROXY=http://127.0.0.1:9`): <paste the header and footer lines from Task 3 step 5>.
- app/mod.rs: 3,329 → <n> lines; merge/order/persist/draw/keys/tests split, test count unchanged by the split.
- Ruling references in comments: <count> → 0.
```

Fill every angle-bracket with the measured value; none may remain.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md CHANGELOG.md
git commit -q -m "docs(v4): wave 0 receipts

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj"
```

---

## Self-review against spec §2

- §2.1 rustfmt + blame ignore → Task 1. §2.2 deps, ureq 3 proxy, signal-hook, MSRV → Tasks 2–5. §2.3 CI jobs (fmt, clippy, test matrix, msrv, audit, deny, package), dependabot → Task 8. §2.4 licenses, metadata, User-Agent → Tasks 6–7. §2.5 AGENTS/CLAUDE/CHANGELOG → Task 9. §2.6 split, comment rewrite, theme → Tasks 10–14 (theme deviation recorded in Task 14). DoD receipts → Tasks 3/4 steps and Task 15.
- Names used across tasks: `install_signal_flag` (Task 4 only); `http` becomes `pub(crate)` (Task 3, used by its own test); `RankFingerprint` becomes `pub(super) type` in `order.rs` (Tasks 10, 12); `FEED_PAGE_JUMP` moves to `keys/feed.rs` (Tasks 11, 12).
- Test count ledger: 526 → 527 (Task 3) → 527 (Task 6 replaces one) → 528 (Task 14).
