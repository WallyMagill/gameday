# gameday v4 Wave 4 — Differentiators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The board can tap you on the shoulder (a favorite scores, a game you pinned ends) and can answer a script (`gameday --once --json`) — the two things golazo has that gameday did not.

**Architecture:** Notifications ride the event paths that already exist: `alerts.check` (a favorite's own score moved) and the status transition `apply_boards` already sees through `prev_board`; a `Notifier` trait with three backends is owned by `App`, defaulting to a silent no-op so tests and the gallery never spawn a process, and `main` installs the OS backend. One-shot mode is a second entry point beside the TUI: it loads the same config and pins, fetches every requested league through the same provider and cache (in parallel, cache-first), builds the same `App`, derives the same ranked lists, and prints them — as the board's own tier rows rendered into a buffer, or as JSON with a pinned schema.

**Tech Stack:** Rust 2021; `serde_json` (already a dependency) for JSON; `std::process::Command` for `osascript`/`notify-send`; `std::thread::scope` for the parallel fetch; no new crates.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §6 (Wave 4), Direction 4 (scope B), Direction 8 (notifications on for favorites + pins), §1 N1–N2.

## Global Constraints

- Branch `v4-wave4` from `main` at `4fda92a`. Commit after every task with the trailer; never push.
- Suite green after every task with the count reported (618 at the start; measured numbers win); `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean before every commit. No `#[allow(...)]`.
- Numbers carry receipts: `NOTIFY_MIN_GAP`, `ONCE_WIDTH`, and the parallel fetch's bound each say what they hold and why.
- Failures name the value, the expectation, and the knob: every CLI error names the flag and the valid set; the golden tests name `UPDATE_GOLDEN=1`.
- No new request load without a budget line: `--once` reads a cache entry younger than `poll::SCOREBOARD_LIVE` (15 s) without the network — but a 304 never rewrites the cache file, so `cache_age` is the file's mtime and the gate only helps a caller polling faster than 15 s. A status bar polling every 30 s still makes one conditional request per requested league per run; that is at most the TUI's own cadence for the same leagues, not free of new load.
- Fresh payloads only: no notification fires from a stale (cached) board, from a first sighting, or from the gallery/tests (their notifier is the silent default).
- Pinned strings: notification title for a score `KC 27 · TB 24` (`<AWAY> <a> · <HOME> <h>`, the favorite's side not moved to the front — the title is the scoreboard line), title for a final `FINAL` with body `KC 27 · TB 24`; `:notify test` sends title `test`, body `gameday notifications are on`; footer `notified via osascript` / `notify failed: <err>`; JSON `status` values `live` | `pre` | `final`.
- Rulings made while planning:
  - **The gap is per (game, kind), not per game:** `NOTIFY_MIN_GAP = 30 s` of live ticks keyed by `(game_id, kind)`, so a walk-off's FINAL is not swallowed by the score ten seconds before it. Kinds: `Score`, `Final`, `Test` (`Test` ignores the gap).
  - **Backends spawn and do not block the frame:** `send` spawns the process and returns; a failure to *spawn* (binary missing, not executable) is the error that demotes the backend to no-op; a non-zero exit is reaped on a detached thread and logged. A notification during a live frame therefore costs a fork, not the ~100 ms `osascript` takes to finish.
  - **A once-mode board is a snapshot, so every league applies with its real `stale` flag** and the first-sighting sort (wave 3) ranks it; the notifier stays the silent default, so nothing fires.
  - **Text width:** the terminal's width when stdout is a tty, else `ONCE_WIDTH = 100` (a guess: `TEXT_X` is 43, so 100 leaves 57 cells of prose, enough for every review-day fragment; reopens if a status bar wants narrower — `--top` is the tool until then).
  - **Partial failure exits 0:** a league that failed with nothing cached is reported on stderr (`gameday: <short error>`) and absent from the output; exit 1 only when *every* requested league failed with nothing cached.
  - **The DoD's live receipts are opportunistic:** `:notify test` end-to-end on this Mac (`screencapture` of the notification) is the wave's own receipt; a real favorite score and the tmux status-line capture are taken during the next live window and recorded in §9 whichever way they land.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `src/notify.rs` (new) | `Notifier` trait; `OsaScript`, `NotifySend`, `Noop`, `Recording`; `os_backend()`; `applescript_literal`; `Kind`; `NotifyState` (gap ledger) |
| `src/config.rs` | `notify: Vec<String>` (default `["favorites", "pins"]`), `notify_favorites()`, `notify_pins()` |
| `src/app/mod.rs`, `src/app/merge.rs`, `src/alerts.rs` | `App.notifier`, `App.notify_state`, `App::set_notifier`, `App::notify`, `App::notify_test`; the two event sites; `Alert.game_id` |
| `src/command.rs`, `src/input.rs`, `src/keymap.rs` | `:notify test` |
| `src/once.rs` (new) | `Opts`, `fetch`, `build_app`, `render_text`, `render_json`, `run` |
| `src/main.rs` | `--once --json --league --live --top --color`; `load_state` shared by the TUI and once paths; HELP |
| `src/board/mod.rs`, `src/board/rows.rs`, `src/dump.rs` | `pub(crate) draw_rule`, `pub(crate) situation_summary`, `pub fn buffer_to_text` |
| `tests/once.rs`, `tests/golden/once-demo.txt`, `tests/golden/once-demo.json`, `tests/config.rs`, `src/app/tests.rs`, `src/notify.rs` tests, `src/main.rs` tests | the teeth |

---

### Task 1: The notifier — config key, trait, backends, `App` wiring

**Files:**
- Create: `src/notify.rs`
- Modify: `src/lib.rs` (`pub mod notify;` between `log` and `poll`), `src/config.rs`, `src/app/mod.rs`, `src/main.rs` (install the OS backend)
- Test: `src/notify.rs` tests, `tests/config.rs`, `src/app/tests.rs`

**Interfaces (produces):**
```rust
// src/notify.rs
pub trait Notifier {
    /// Deliver one notification. `Err` means the backend could not even try
    /// (binary missing, spawn refused); a delivery that starts is `Ok`.
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
    /// The name the footer prints after `:notify test`.
    fn name(&self) -> &'static str;
}
pub struct OsaScript;                 // macOS
pub struct NotifySend;                // Linux, when `notify-send` is on PATH
pub struct Noop { pub reason: &'static str }  // logs `reason` once, then stays silent
pub struct Recording { pub sent: std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>> } // tests
pub fn os_backend() -> Box<dyn Notifier>;
pub fn applescript_literal(s: &str) -> String;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum Kind { Score, Final, Test }
#[derive(Default)] pub struct NotifyState { sent_at: HashMap<(String, Kind), u64> }
impl NotifyState { pub fn allows(&mut self, game_id: &str, kind: Kind, tick: u64) -> bool; }
pub const NOTIFY_MIN_GAP: u64 = 30 * crate::app::LIVE_TICKS_PER_SEC;
// src/config.rs
pub notify: Vec<String>;            // serde default ["favorites", "pins"]
impl Config { pub fn notify_favorites(&self) -> bool; pub fn notify_pins(&self) -> bool; }
// src/app/mod.rs
pub fn set_notifier(&mut self, n: Box<dyn Notifier>);
pub(crate) fn notify(&mut self, game_id: &str, kind: Kind, title: &str, body: &str) -> bool; // true when sent
```

- [ ] **Step 1: Failing tests.** `tests/config.rs`:

```rust
#[test]
fn notify_defaults_to_favorites_and_pins_and_an_empty_list_disables() {
    let dir = tmp("notify-default");
    std::fs::write(dir.join("config.toml"), "enabled_tabs = [\"nfl\"]\nfavorites = []\n").unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert_eq!(c.notify, vec!["favorites".to_string(), "pins".to_string()], "an old config gets both");
    assert!(c.notify_favorites() && c.notify_pins());
    std::fs::write(dir.join("config.toml"), "enabled_tabs = [\"nfl\"]\nfavorites = []\nnotify = []\n").unwrap();
    let c = Config::load_from(&dir).unwrap();
    assert!(!c.notify_favorites() && !c.notify_pins(), "notify = [] is off");
    // Round-trips, and an unknown word is kept (not an error) and means nothing.
    let mut c = Config::default_all();
    c.notify = vec!["pins".into(), "bogus".into()];
    c.save_to(&dir).unwrap();
    let back = Config::load_from(&dir).unwrap();
    assert_eq!(back.notify, c.notify);
    assert!(back.notify_pins() && !back.notify_favorites());
}
```

`src/notify.rs` tests:

```rust
    #[test]
    fn applescript_literals_escape_quotes_and_backslashes() {
        assert_eq!(applescript_literal(r#"He said "go" \ now"#), r#""He said \"go\" \\ now""#);
        assert_eq!(applescript_literal(""), r#""""#);
    }

    #[test]
    fn the_gap_is_per_game_and_kind() {
        let mut s = NotifyState::default();
        assert!(s.allows("g1", Kind::Score, 100));
        assert!(!s.allows("g1", Kind::Score, 100 + NOTIFY_MIN_GAP - 1), "inside the gap");
        assert!(s.allows("g1", Kind::Final, 105), "a different kind is its own gap");
        assert!(s.allows("g2", Kind::Score, 105), "a different game is its own gap");
        assert!(s.allows("g1", Kind::Score, 100 + NOTIFY_MIN_GAP), "the gap has passed");
        assert!(s.allows("g1", Kind::Test, 101) && s.allows("g1", Kind::Test, 102), "test ignores the gap");
    }

    #[test]
    fn a_recording_notifier_records_and_a_noop_is_ok() {
        let r = Recording::default();
        r.send("t", "b").unwrap();
        assert_eq!(r.sent.borrow().as_slice(), &[("t".to_string(), "b".to_string())]);
        assert!(Noop { reason: "test" }.send("t", "b").is_ok());
    }
```

`src/app/tests.rs`:

```rust
#[test]
fn app_notify_honors_the_gap_and_demotes_a_failing_backend_to_noop() {
    use crate::notify::{Kind, Notifier, Recording};
    let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
    let rec = Recording::default();
    let log = rec.sent.clone();
    app.set_notifier(Box::new(rec));
    app.tick = 1_000;
    assert!(app.notify("1", Kind::Score, "KC 7 · TB 0", "TD"));
    assert!(!app.notify("1", Kind::Score, "KC 14 · TB 0", "TD again"), "inside the gap");
    app.tick += crate::notify::NOTIFY_MIN_GAP;
    assert!(app.notify("1", Kind::Score, "KC 14 · TB 0", "TD again"));
    assert_eq!(log.borrow().len(), 2);
    // A backend that cannot send is replaced by the silent no-op after one failure.
    struct Broken;
    impl Notifier for Broken {
        fn send(&self, _: &str, _: &str) -> Result<(), String> { Err("no such binary".into()) }
        fn name(&self) -> &'static str { "broken" }
    }
    app.set_notifier(Box::new(Broken));
    assert!(!app.notify("2", Kind::Score, "t", "b"));
    assert_eq!(app.notifier_name(), "noop", "demoted after the first failure");
}
```

- [ ] **Step 2: Implement.** `src/notify.rs`:

```rust
//! Desktop notifications: a favorite scored, a game you pinned ended.
//! Backends spawn the OS tool and return — a notification must never hold
//! a live frame for the ~100 ms `osascript` takes — so `send` reports only
//! whether the delivery could start. A backend that cannot start one is
//! replaced by [`Noop`] after that first failure (`App::notify`), which logs
//! one line and stays silent: a missing `notify-send` is not something to
//! nag about at every score.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

/// Quiet window per (game, kind): one notification per possession-length
/// is enough, and a walk-off's FINAL must not be swallowed by the score
/// ten seconds before it — hence per kind, not per game. A guess.
pub const NOTIFY_MIN_GAP: u64 = 30 * crate::app::LIVE_TICKS_PER_SEC;

pub trait Notifier {
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
    fn name(&self) -> &'static str;
}

/// An AppleScript string literal: backslashes and double quotes escaped,
/// wrapped in double quotes.
pub fn applescript_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Spawn `cmd`, feed it `stdin`, and reap it on a detached thread so a slow
/// tool never blocks the caller; a non-zero exit is logged there.
fn spawn_and_forget(mut cmd: Command, stdin: Option<String>, what: &'static str) -> Result<(), String> {
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(|e| format!("{what}: {e}"))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    std::thread::spawn(move || match child.wait_with_output() {
        Ok(out) if !out.status.success() => crate::log::note(&format!(
            "{what} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => crate::log::note(&format!("{what}: wait failed: {e}")),
        _ => {}
    });
    Ok(())
}

pub struct OsaScript;
impl Notifier for OsaScript {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        let script = format!(
            "display notification {} with title \"gameday\" subtitle {}",
            applescript_literal(body),
            applescript_literal(title)
        );
        spawn_and_forget(Command::new("osascript"), Some(script), "osascript")
    }
    fn name(&self) -> &'static str { "osascript" }
}

pub struct NotifySend;
impl Notifier for NotifySend {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        let mut cmd = Command::new("notify-send");
        cmd.arg(format!("gameday · {title}")).arg(body);
        spawn_and_forget(cmd, None, "notify-send")
    }
    fn name(&self) -> &'static str { "notify-send" }
}

/// No backend (Windows, or one that failed): the first `send` logs `reason`
/// to gameday.log so the silence is explained once, and nothing else happens.
pub struct Noop { pub reason: &'static str }
impl Notifier for Noop {
    fn send(&self, _: &str, _: &str) -> Result<(), String> {
        crate::log::note_once("notify-noop", &format!("notifications off: {}", self.reason));
        Ok(())
    }
    fn name(&self) -> &'static str { "noop" }
}

/// The test double: every send is appended to a shared log the test holds.
#[derive(Default)]
pub struct Recording { pub sent: std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>> }
impl Notifier for Recording {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        self.sent.borrow_mut().push((title.to_string(), body.to_string()));
        Ok(())
    }
    fn name(&self) -> &'static str { "recording" }
}

/// The backend this OS gets. Linux needs `notify-send` on PATH; Windows and
/// anything else is the no-op, with the reason it stays quiet.
pub fn os_backend() -> Box<dyn Notifier> {
    if cfg!(target_os = "macos") {
        return Box::new(OsaScript);
    }
    if cfg!(target_os = "linux") {
        let on_path = std::env::var_os("PATH").is_some_and(|p| {
            std::env::split_paths(&p).any(|d| d.join("notify-send").is_file())
        });
        return if on_path { Box::new(NotifySend) } else { Box::new(Noop { reason: "notify-send is not on PATH" }) };
    }
    Box::new(Noop { reason: "no notification backend on this OS" })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind { Score, Final, Test }

/// The gap ledger: game id and kind → the tick a notification last went out.
#[derive(Default)]
pub struct NotifyState { sent_at: HashMap<(String, Kind), u64> }
impl NotifyState {
    /// Records the send when it allows it.
    pub fn allows(&mut self, game_id: &str, kind: Kind, tick: u64) -> bool {
        if kind == Kind::Test {
            return true;
        }
        let key = (game_id.to_string(), kind);
        if self.sent_at.get(&key).is_some_and(|&at| tick < at + NOTIFY_MIN_GAP) {
            return false;
        }
        self.sent_at.insert(key, tick);
        true
    }
}
```

`src/config.rs`: `#[serde(default = "default_notify")] pub notify: Vec<String>` with a doc comment (`notify = ["favorites", "pins"]` is the default; `[]` turns notifications off; words are case-insensitive; unknown words mean nothing), `fn default_notify() -> Vec<String>`, `default_all()` sets it, `notify_favorites()` / `notify_pins()` as `self.notify.iter().any(|w| w.eq_ignore_ascii_case("favorites"))` etc. Check every `Config { .. }` literal in `src/` and `tests/` and add the field (or use `..Config::default_all()`).

`src/app/mod.rs`: fields `notifier: Box<dyn crate::notify::Notifier>` (init `Box::new(Noop { reason: "no backend installed" })` — tests and the gallery stay silent) and `notify_state: crate::notify::NotifyState`; methods:

```rust
    pub fn set_notifier(&mut self, n: Box<dyn crate::notify::Notifier>) { self.notifier = n; }
    pub fn notifier_name(&self) -> &'static str { self.notifier.name() }

    /// One notification, gap-checked per (game, kind). A backend that cannot
    /// start the delivery is demoted to the silent no-op after that first
    /// failure, and the failure is logged with the backend's name.
    pub(crate) fn notify(&mut self, game_id: &str, kind: crate::notify::Kind, title: &str, body: &str) -> bool {
        if !self.notify_state.allows(game_id, kind, self.tick) {
            return false;
        }
        match self.notifier.send(title, body) {
            Ok(()) => true,
            Err(err) => {
                crate::log::note(&format!("notify via {} failed, switching off: {err}", self.notifier.name()));
                self.notifier = Box::new(crate::notify::Noop { reason: "the backend failed once" });
                false
            }
        }
    }
```

`src/main.rs`: after `App::new(...)` on the TUI path, `app.set_notifier(gameday::notify::os_backend());`.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(notify): the notifier — config key, trait, os backends, gap ledger`.

### Task 2: The two events and `:notify test`

**Files:**
- Modify: `src/alerts.rs` (`Alert.game_id`), `src/app/merge.rs` (`apply_boards`), `src/app/mod.rs` (`notify_test`), `src/command.rs`, `src/input.rs`
- Test: `src/app/tests.rs`, `src/command.rs` tests, `src/alerts.rs` tests (the `Alert` literal)

**Interfaces:** `Alert { text, until_tick, game_id: String }`; `Cmd::NotifyTest`; `App::notify_test(&mut self)`.

- [ ] **Step 1: Failing tests** in `src/app/tests.rs` (helpers `app_with`, `g`; `Favorite`, `Pin` from `crate::config`; `Recording` from `crate::notify`):

```rust
fn recording(app: &mut App) -> std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>> {
    let rec = crate::notify::Recording::default();
    let log = rec.sent.clone();
    app.set_notifier(Box::new(rec));
    log
}

#[test]
fn a_favorite_score_notifies_once_inside_the_gap_with_the_scoring_play_as_body() {
    let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
    app.config.favorites.push(Favorite { league: League::Nfl, team_abbr: "KC".into() });
    let log = recording(&mut app);
    app.tick = 1_000;
    let mut scored = g("1", "KC", "TB", true);
    scored.away_score = 34; // was 27
    scored.scoring_plays.push(Play { text: "Mahomes 12 Yd pass to Kelce".into(), team: "KC".into(), scoring: true, ..Default::default() });
    app.apply_boards(League::Nfl, vec![scored.clone()], false);
    assert_eq!(log.borrow().as_slice(), &[("KC 34 · TB 24".to_string(), "Mahomes 12 Yd pass to Kelce".to_string())]);
    // A second delta ten seconds later is inside the gap: banner yes, notification no.
    app.tick += 10 * LIVE_TICKS_PER_SEC;
    scored.away_score = 41;
    app.apply_boards(League::Nfl, vec![scored.clone()], false);
    assert_eq!(log.borrow().len(), 1, "inside NOTIFY_MIN_GAP");
    // The opponent scoring is not the favorite's notification.
    app.tick += crate::notify::NOTIFY_MIN_GAP;
    scored.home_score = 31;
    app.apply_boards(League::Nfl, vec![scored], false);
    assert_eq!(log.borrow().len(), 1, "TB scored, KC is the favorite");
}

#[test]
fn a_pinned_or_favorite_game_reaching_final_notifies_and_a_stale_or_first_sighting_never_does() {
    let pin = Pin { game_id: "1".into(), league: League::Nfl, final_at: None };
    let mut app = app_with(vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)], vec![pin]);
    app.config.favorites.push(Favorite { league: League::Nfl, team_abbr: "PHI".into() });
    let log = recording(&mut app);
    app.tick = 1_000;
    let mut over = g("1", "KC", "TB", true);
    over.status = Status::Final;
    let mut over2 = g("2", "DAL", "PHI", true);
    over2.status = Status::Final;
    app.apply_boards(League::Nfl, vec![over.clone(), over2.clone()], false);
    let sent = log.borrow().clone();
    assert_eq!(sent.len(), 2, "the pin and the favorite: {sent:?}");
    assert!(sent.iter().all(|(t, _)| t == "FINAL"), "{sent:?}");
    assert!(sent.iter().any(|(_, b)| b == "KC 27 · TB 24") && sent.iter().any(|(_, b)| b == "DAL 27 · PHI 24"), "{sent:?}");
    // Still final on the next poll: nothing new.
    app.tick += 1;
    app.apply_boards(League::Nfl, vec![over.clone(), over2.clone()], false);
    assert_eq!(log.borrow().len(), 2);
    // A stale payload never notifies, and a game first seen as final is not an event.
    let mut app = app_with(vec![], vec![Pin { game_id: "9".into(), league: League::Nfl, final_at: None }]);
    let log = recording(&mut app);
    let mut fresh_final = g("9", "GB", "CHI", true);
    fresh_final.status = Status::Final;
    app.apply_boards(League::Nfl, vec![fresh_final.clone()], false);
    assert!(log.borrow().is_empty(), "first sighting");
    let mut live = g("9", "GB", "CHI", true);
    app.apply_boards(League::Nfl, vec![live.clone()], false);
    live.status = Status::Final;
    app.apply_boards(League::Nfl, vec![live], true);
    assert!(log.borrow().is_empty(), "stale");
}

#[test]
fn notify_off_sends_nothing_and_the_test_command_reports_the_backend() {
    let mut app = app_with(vec![g("1", "KC", "TB", true)], vec![]);
    app.config.favorites.push(Favorite { league: League::Nfl, team_abbr: "KC".into() });
    app.config.notify.clear();
    let log = recording(&mut app);
    let mut scored = g("1", "KC", "TB", true);
    scored.away_score = 34;
    app.apply_boards(League::Nfl, vec![scored], false);
    assert!(log.borrow().is_empty(), "notify = []");
    app.notify_test();
    assert_eq!(log.borrow().as_slice(), &[("test".to_string(), "gameday notifications are on".to_string())], "`:notify test` ignores the config switch and the gap");
    assert_eq!(app.status_line.as_deref(), Some("notified via recording"));
}
```

`src/command.rs` tests: `parse("notify test") == Ok(Cmd::NotifyTest)`; `parse("notify")` and `parse("notify foo")` are `Err` naming `test`; `complete("notify ")` yields `["notify test"]`.

- [ ] **Step 2: Implement.** `src/alerts.rs`: add `pub game_id: String` to `Alert` (set from `game.id` where it fires; the banner ignores it). `src/app/merge.rs`, inside the `if !stale` block, after the alert is stored:

```rust
            // The favorite's score, as a notification: the scoreboard line
            // for the title and the scoring play (when the feed already
            // carries it) for the body. Same event the banner fires on, so
            // the two can never disagree about what happened.
            if self.config.notify_favorites() {
                if let Some(alert) = self.active_alert.clone().filter(|a| a.until_tick == self.tick + crate::alerts::BANNER_TICKS) {
                    if let Some(game) = self.game_by_id(&alert.game_id) {
                        let title = format!("{} {} · {} {}", game.away.abbr, game.away_score, game.home.abbr, game.home_score);
                        let body = game.scoring_plays.last().map(|p| p.text.clone()).unwrap_or_else(|| alert.text.clone());
                        self.notify(&game.id, crate::notify::Kind::Score, &title, &body);
                    }
                }
            }
```

(The `until_tick` filter picks the alert this apply just created rather than one still showing from an earlier poll; `alerts.check` returns the new alert directly, so bind it — `if let Some(alert) = ... { self.active_alert = Some(alert.clone()); self.bell_pending = true; <notify from `alert`> }` — and drop the filter. Use whichever reads cleaner; the test decides.)

Finals, in the same `!stale` block, using the `prev_board` already computed at the top of `apply_boards`:

```rust
            // A game you follow just ended: pinned (config `pins`) or a
            // favorite's (config `favorites`). `prev_board` is this league's
            // last board, so a game first seen as final — startup, a new
            // id — is not a transition and stays quiet.
            for game in self.boards.get(&league).cloned().unwrap_or_default() {
                if game.status != Status::Final {
                    continue;
                }
                let was_live = prev_board.iter().any(|p| p.id == game.id && p.status != Status::Final);
                if !was_live {
                    continue;
                }
                let pinned = self.pins.iter().any(|p| p.game_id == game.id);
                let followed = (pinned && self.config.notify_pins()) || (self.favorited(&game) && self.config.notify_favorites());
                if followed {
                    let body = format!("{} {} · {} {}", game.away.abbr, game.away_score, game.home.abbr, game.home_score);
                    self.notify(&game.id, crate::notify::Kind::Final, "FINAL", &body);
                }
            }
```

(`favorited` is a private fn in `derive.rs`; make it `pub(crate)`.) `src/app/mod.rs`:

```rust
    /// `:notify test`: one notification through whatever backend is
    /// installed, past the gap and the config switch, and the footer says
    /// which backend took it or why it could not.
    pub fn notify_test(&mut self) {
        let name = self.notifier.name();
        if self.notify("", crate::notify::Kind::Test, "test", "gameday notifications are on") {
            self.toast(format!("notified via {name}"));
        } else {
            self.sticky_status(format!("notify failed via {name}; see gameday.log"));
        }
    }
```

`src/command.rs`: `ArgSpec::NotifyTest` (the one accepted word is `test`; `arg_values` returns `vec!["test"]`; parse: `None | Some(other) => Err(format!("{name:?} takes \"test\", got {arg:?}"))` shape), registry `("notify", ArgSpec::NotifyTest)` before `help`, `Cmd::NotifyTest` with a doc line. `src/input.rs`: `Cmd::NotifyTest => app.notify_test()`. The `:notify` command is discoverable through completion; no key binding.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(notify): a favorite's score and a followed game's final notify; :notify test`.

### Task 3: One-shot mode — `--once`, `--json`, the parallel cache-first fetch, the goldens

**Files:**
- Create: `src/once.rs`, `tests/once.rs`, `tests/golden/once-demo.txt`, `tests/golden/once-demo.json`
- Modify: `src/lib.rs` (`pub mod once;`), `src/main.rs` (`Args`, `parse_args`, `HELP`, `main`, `load_state`), `src/board/mod.rs` (`pub(crate) fn draw_rule`), `src/board/rows.rs` (`pub(crate) fn situation_summary`), `src/dump.rs` (`pub fn buffer_to_text`), `src/provider/espn.rs` (`scoreboard_cached_within`)
- Test: `src/main.rs` tests (parse), `src/provider/espn.rs` tests (the cache gate), `tests/once.rs`

**Interfaces:**
```rust
// src/once.rs
pub struct Opts { pub json: bool, pub leagues: Vec<League>, pub live: bool, pub top: Option<usize>, pub color: bool, pub width: u16 }
pub const ONCE_WIDTH: u16 = 100;
/// Every league in parallel; each answer is (league, games, stale). Errors are the provider's short text.
pub fn fetch<P: SportsProvider + Sync>(provider: &P, leagues: &[League]) -> (Vec<(League, Vec<Game>, bool)>, Vec<(League, String)>);
pub fn build_app(config: Config, pins: Vec<Pin>, dir: PathBuf, offset: UtcOffset, now: OffsetDateTime, boards: Vec<(League, Vec<Game>, bool)>) -> App;
pub fn render_text(app: &mut App, opts: &Opts) -> String;
pub fn render_json(app: &mut App, opts: &Opts, stale: bool) -> serde_json::Value;
/// Runs the whole thing; returns the exit code (0 output, 1 nothing fetched and nothing cached).
pub fn run(config: Config, pins: Vec<Pin>, dir: PathBuf, offset: UtcOffset, provider: &EspnProvider, opts: Opts) -> i32;
// src/provider/espn.rs
impl EspnProvider { pub fn scoreboard_cached_within(&self, league: League, max_age: Duration) -> Result<(Vec<Game>, bool), ProviderError>; }
// src/dump.rs
pub fn buffer_to_text(buf: &Buffer) -> String;   // symbols only, one line per row, trailing spaces trimmed
```

JSON schema (pinned by the golden and by `every_game_has_exactly_the_schema_keys`):
```
{ "generated_at": "2026-08-31T21:30:01-04:00", "stale": false,
  "games": [ { "league": "nfl", "id": "…", "status": "live|pre|final",
               "period": "Q4", "clock": "1:27", "start": "2026-…"|null,
               "away": {"abbr": "KC", "name": "Chiefs", "score": 27, "record": "2-0", "rank": null},
               "home": {…},
               "watch": {"score": 63, "chip": "RED ZONE"|null, "why": "RED ZONE"},
               "situation": "KC 1ST & GOAL AT TB 3"|null, "last_play": "…"|null, "pinned": true } ] }
```
Games are `derive().selection` in board order (MY GAMES, IN PLAY, FINAL, LATER); `--live` keeps `status == Live`; `--top N` keeps the first N after that.

- [ ] **Step 1: Failing tests.** `src/main.rs` tests:

```rust
    #[test]
    fn once_flags_parse_and_the_rest_need_once() {
        let a = parsed(&["gameday", "--once", "--json", "--league", "nfl", "--league", "mlb", "--live", "--top", "3", "--color"]);
        assert!(a.once && a.json && a.live && a.color);
        assert_eq!(a.leagues, vec![League::Nfl, League::Mlb]);
        assert_eq!(a.top, Some(3));
        assert!(!parsed(&["gameday", "--once"]).json);
        for bad in [vec!["gameday", "--json"], vec!["gameday", "--league", "nfl"], vec!["gameday", "--top", "3"], vec!["gameday", "--live"]] {
            let owned: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            let e = parse_args(&owned).unwrap_err();
            assert!(e.contains("--once"), "{e}");
        }
        let e = err(&["gameday", "--once", "--league", "xfl"]);
        assert!(e.contains("xfl") && e.contains("nfl|cfb"), "{e}");
        let e = err(&["gameday", "--once", "--top", "many"]);
        assert!(e.contains("--top") && e.contains("many"), "{e}");
    }
```

`src/provider/espn.rs` tests (mirror `a_fresh_standings_cache_short_circuits_http`, whatever it is named near line 476):

```rust
    #[test]
    fn a_young_scoreboard_cache_is_served_without_the_network() {
        let dir = tmp("once-cache");
        // The mapped body of any league: the committed NFL fixture.
        cache_write(&dir, "nfl-scoreboard", include_str!("../../fixtures/nfl-scoreboard.json")).unwrap();
        // A provider whose HTTP cannot succeed: an unroutable proxy-less agent is
        // overkill — point the URL at a closed local port via `with_base_url` if the
        // provider has one; otherwise use `with_timeouts(.., 1ms)` against the real
        // host and assert the result came from the cache with `stale == false`.
        let p = EspnProvider::with_timeouts(dir.clone(), time::UtcOffset::UTC, Duration::from_millis(1));
        let (games, stale) = p.scoreboard_cached_within(League::Nfl, Duration::from_secs(60)).unwrap();
        assert!(!games.is_empty() && !stale, "young cache: fresh, no HTTP");
        // Past the age the gate fetches (and here fails) → the cache is served stale.
        let (_, stale) = p.scoreboard_cached_within(League::Nfl, Duration::ZERO).unwrap();
        assert!(stale, "old cache: HTTP attempted and failed, cache served stale");
    }
```

(Pick the fixture the existing scoreboard tests use — `grep -n 'include_str!' src/provider/espn.rs` — and the same `tmp` helper.)

`tests/once.rs`:

```rust
use gameday::app::App;
use gameday::demo;
use gameday::domain::*;
use gameday::once::{self, Opts};
use gameday::provider::{ProviderError, SportsProvider};

fn demo_once() -> App {
    let dir = std::env::temp_dir().join(format!("gd-once-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let boards: Vec<(League, Vec<Game>, bool)> = demo::demo_boards().into_iter().map(|(l, g)| (l, g, false)).collect();
    once::build_app(
        demo::demo_config(),
        demo::demo_pins(),
        dir,
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
        time::macros::datetime!(2026-08-31 21:30:01 -4),
        boards,
    )
}

fn opts() -> Opts {
    Opts { json: false, leagues: vec![], live: false, top: None, color: false, width: once::ONCE_WIDTH }
}

/// Compare to the committed golden; `UPDATE_GOLDEN=1 cargo test --test once` rewrites it.
fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e} — run with UPDATE_GOLDEN=1 to create it", path.display()));
    assert_eq!(actual, want, "{} differs from the golden; if the change is intended: UPDATE_GOLDEN=1 cargo test --test once", path.display());
}

#[test]
fn the_text_form_matches_the_golden_and_is_the_boards_own_grid() {
    let mut app = demo_once();
    let text = once::render_text(&mut app, &opts());
    golden("once-demo.txt", &text);
    assert!(text.contains("IN PLAY") && text.contains("LATER"), "{text}");
    assert!(!text.contains('\x1b'), "no color without --color: {text}");
    // The same columns the board draws: the clock column sits where rows.rs puts it.
    let live = text.lines().find(|l| l.contains("Q4")).expect("a live row");
    assert_eq!(live.find("Q4").map(|b| live[..b].chars().count()), Some(23), "CLOCK_X\n{text}");
}

#[test]
fn top_live_and_league_narrow_the_text() {
    let mut app = demo_once();
    let top = once::render_text(&mut app, &Opts { top: Some(3), ..opts() });
    let rows = top.lines().filter(|l| l.contains(" @ ") || l.contains("Q") || l.contains("FINAL")).count();
    assert!(rows <= 3, "--top 3 keeps three game rows:\n{top}");
    let live = once::render_text(&mut app, &Opts { live: true, ..opts() });
    assert!(!live.contains("LATER") && !live.contains("FINAL ─"), "--live drops the other sections:\n{live}");
}

#[test]
fn the_json_form_matches_the_golden_and_pins_the_schema() {
    let mut app = demo_once();
    let v = once::render_json(&mut app, &opts(), false);
    golden("once-demo.json", &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["generated_at"], "2026-08-31T21:30:01-04:00");
    assert_eq!(v["stale"], false);
    let games = v["games"].as_array().expect("games");
    assert!(!games.is_empty());
    let keys: Vec<&str> = vec!["league", "id", "status", "period", "clock", "start", "away", "home", "watch", "situation", "last_play", "pinned"];
    for g in games {
        let mut have: Vec<&str> = g.as_object().unwrap().keys().map(String::as_str).collect();
        have.sort();
        let mut want = keys.clone();
        want.sort();
        assert_eq!(have, want, "schema drift in {g}");
        assert!(["live", "pre", "final"].contains(&g["status"].as_str().unwrap()));
        for side in ["away", "home"] {
            for k in ["abbr", "name", "score", "record", "rank"] {
                assert!(g[side].get(k).is_some(), "{side}.{k} missing in {g}");
            }
        }
        for k in ["score", "chip", "why"] {
            assert!(g["watch"].get(k).is_some(), "watch.{k} missing in {g}");
        }
    }
    let pinned = games.iter().filter(|g| g["pinned"] == true).count();
    assert_eq!(pinned, demo::demo_pins().len(), "pins are marked");
}

struct Failing;
impl SportsProvider for Failing {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        Err(ProviderError::Http { status: 0, key: format!("{}-scoreboard", league.slug()), url: String::new(), detail: "connection refused".into() })
    }
    fn scoreboard_on(&self, l: League, _: time::Date) -> Result<(Vec<Game>, bool), ProviderError> { self.scoreboard(l) }
    fn summary(&self, _: League, _: &str) -> Result<(Summary, bool), ProviderError> { unreachable!() }
    fn stats(&self, _: League, _: &str) -> Result<(GameStats, bool), ProviderError> { unreachable!() }
    fn standings(&self, _: League) -> Result<(StandingsTable, bool), ProviderError> { unreachable!() }
}

struct Slow;
impl SportsProvider for Slow {
    fn scoreboard(&self, league: League) -> Result<(Vec<Game>, bool), ProviderError> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        Ok((demo::demo_boards().remove(&league).unwrap_or_default(), false))
    }
    fn scoreboard_on(&self, l: League, _: time::Date) -> Result<(Vec<Game>, bool), ProviderError> { self.scoreboard(l) }
    fn summary(&self, _: League, _: &str) -> Result<(Summary, bool), ProviderError> { unreachable!() }
    fn stats(&self, _: League, _: &str) -> Result<(GameStats, bool), ProviderError> { unreachable!() }
    fn standings(&self, _: League) -> Result<(StandingsTable, bool), ProviderError> { unreachable!() }
}

#[test]
fn every_league_fails_with_nothing_cached_is_all_errors_and_the_fetch_runs_in_parallel() {
    let (boards, errors) = once::fetch(&Failing, &League::ALL);
    assert!(boards.is_empty());
    assert_eq!(errors.len(), League::ALL.len());
    assert!(errors[0].1.contains("ESPN unreachable"), "{:?}", errors[0]);
    let t = std::time::Instant::now();
    let (boards, errors) = once::fetch(&Slow, &League::ALL);
    assert!(errors.is_empty());
    assert_eq!(boards.len(), League::ALL.len());
    assert!(t.elapsed() < std::time::Duration::from_millis(900), "nine 300 ms fetches took {:?}: not parallel", t.elapsed());
}
```

Note: `once::fetch` is generic over `SportsProvider + Sync` so the test doubles work; `run` takes the real `EspnProvider` because only it has the cache-age gate.

- [ ] **Step 2: Implement.**

`src/provider/espn.rs`:

```rust
    /// `--once`'s scoreboard: a cache entry younger than `max_age` is served
    /// as fresh without touching the network (a status bar polling every
    /// 30 s must not cost more than the TUI's own cadence); anything older
    /// goes through `scoreboard`, which falls back to the cache, marked
    /// stale, when the fetch fails.
    pub fn scoreboard_cached_within(&self, league: League, max_age: Duration) -> Result<(Vec<Game>, bool), ProviderError> {
        let key = format!("{}-scoreboard", league.slug());
        if cache_age(&self.cache_dir, &key).is_some_and(|age| age < max_age) {
            if let Ok(body) = cache_read(&self.cache_dir, &key) {
                if let Ok(games) = map_scoreboard(league, &body, self.offset) {
                    return Ok((games, false));
                }
            }
        }
        self.scoreboard(league)
    }
```

`src/dump.rs`: `pub fn buffer_to_text(buf: &Buffer) -> String` (symbols only, each row `trim_end()`ed, `\n`-joined) — and make the test module's `text_of` call it.

`src/once.rs`:

```rust
//! `gameday --once`: fetch, rank, print, exit. The same config, pins,
//! provider, cache and ranking as the board; no terminal, no poll thread,
//! no alternate screen. Text is the board's own tier rows rendered into a
//! buffer; JSON is a pinned schema for scripts and status bars.

pub const ONCE_WIDTH: u16 = 100; // doc comment: the receipt from the plan's rulings

pub struct Opts { … }

pub fn fetch<P: SportsProvider + Sync>(provider: &P, leagues: &[League]) -> (Vec<(League, Vec<Game>, bool)>, Vec<(League, String)>) {
    // One thread per league, joined together: the slowest league bounds
    // the whole call at one HTTP_TIMEOUT instead of nine in a row.
    let results: Vec<(League, Result<(Vec<Game>, bool), ProviderError>)> = std::thread::scope(|s| {
        let handles: Vec<_> = leagues.iter().map(|&l| s.spawn(move || (l, provider.scoreboard(l)))).collect();
        handles.into_iter().map(|h| h.join().expect("a fetch thread panicked")).collect()
    });
    let mut boards = Vec::new();
    let mut errors = Vec::new();
    for (league, r) in results {
        match r {
            Ok((games, stale)) => boards.push((league, games, stale)),
            Err(e) => errors.push((league, e.short())),
        }
    }
    (boards, errors)
}
```

`run` uses the same shape but calls `provider.scoreboard_cached_within(l, crate::poll::SCOREBOARD_LIVE)` — implement `fetch` over a closure `impl Fn(League) -> Result<(Vec<Game>, bool), ProviderError> + Sync` so both callers share it (`fetch_with(leagues, |l| provider.scoreboard(l))`); keep `fetch` as the generic-provider wrapper the tests call.

`build_app`: `App::new(config, pins, dir, offset)`, `now_override = Some(now)`, `apply_boards(league, games, stale)` for each board (the wave 3 first-sighting rule ranks a stale first apply), `tab = Tab::Home`.

`render_text`: `let d = app.derive();` — sections `[("MY GAMES", my_games), ("IN PLAY", in_play), ("FINAL", finals), ("LATER", later)]`, each filtered by `--live` (`status == Live`) and then the running `--top` budget (count game rows across sections; rules are free); a `TestBackend` terminal of `(opts.width, rows_needed.max(1))`; for each section with rows: `board::draw_rule(frame, rect_row(y), label, caption)` (caption `SORTED BY WATCHABILITY` for IN PLAY when sort is watch — reuse `board::sort_phrase` made `pub(crate)`; empty for the rest), then per game `rows::draw_tier2` (Live) or `rows::draw_tier3` (Final/Pre) with `RowCtx { hot: w.hot, chip: w.chip, nudge: None, selected: false, pinned: app.pins.iter().any(..), league_tag: d.mixed, now: app.now(), leaders_line: None }`; theme `broadcast` (`dump::with_theme`); then `dump::buffer_to_text` or, when `opts.color`, `dump::buffer_to_ansi`.

`render_json`: build with `serde_json::json!`; `generated_at` = `app.now().format(&time::format_description::well_known::Rfc3339)`; `start` likewise or `null`; `situation` = `rows::situation_summary(&game)`; `last_play` = `game.last_plays.first().map(|p| p.text.clone())`; `watch` from `rank::watchability(&game, now)` (`chip` null when `None`); `rank` = `team.rank`; `pinned` from `app.pins`.

`run`: `fetch` via the cache gate → if `boards.is_empty()`: `eprintln!("gameday: {}", errors.first().map(|(_, e)| e.as_str()).unwrap_or("nothing to fetch"))`, return 1; else every error → one stderr line `gameday: {short}`; build the app; print text or JSON (`--json` implies no color; `--color` applies only when `std::io::stdout().is_terminal()`); return 0.

`src/main.rs`: `Args` gains `once, json, leagues: Vec<League>, live, top: Option<usize>, color`; `parse_args` handles `--once`, `--json`, `--league <slug>` (repeatable; unknown slug → `format!("--league {slug:?} is not a league, valid: {}", League::ALL.map(League::slug).join("|"))`), `--live`, `--top N` (`--top expects a non-negative integer, got {raw:?}`), `--color`; after the loop, any of them without `--once` → `format!("{flag} is a `gameday --once` flag; valid: {VALID}")`. `VALID` and `HELP` gain the once form:

```
  gameday --once [--json] [--league L]... [--live] [--top N] [--color]
                          fetch once, print the ranked board (text, or JSON for scripts), exit
```

`main`: extract the config/pins/dir loading (from `resolve_dir` through `let pins = pins_loaded.value;`, minus the theme lines) into `fn load_state(config_dir: Option<PathBuf>) -> std::io::Result<(PathBuf, Config, Vec<Pin>, Option<String>)>`; the TUI path calls it and continues with the theme logic; the once path (placed after `probe`, before `require_tty`) calls it, builds `EspnProvider::new(dir.join("cache"), offset)`, and `std::process::exit(once::run(...))`. `gameday::log::set_file(dir.join("gameday.log"))` before the fetch so mapper notes go to the log, not stdout.

- [ ] **Step 3: Generate the goldens** with `UPDATE_GOLDEN=1 cargo test --release --test once`, read both files (the text must look like the board's rows; the JSON must be the schema above), commit them with the code. Suite, clippy, fmt, commit `feat(once): --once prints the ranked board as text or JSON; parallel cache-first fetch`.

### Task 4: Receipts and the notification capture

**Files:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§9), `CHANGELOG.md`

- [ ] **Step 1 (controller, before dispatch):** the wave's own end-to-end receipt on this Mac: `target/release/gameday` in tmux with a scratch `--config-dir`, type `:notify test` + Enter, `screencapture -x` of the screen within two seconds, confirm the notification is in the image, and the footer line `notified via osascript`. A status-line receipt: `tmux set -g status-right '#(gameday --once --live --top 1 --league mlb)'` on a scratch server for one refresh, captured. The live-window receipts (a real favorite score; the status line over a live window) are attempted in the next window and recorded either way.
- [ ] **Step 2:** `### Wave 4 — landed <date>` under §9: suite counts; the events and their pinned strings; the gap rule; the backend table (macOS `osascript`, Linux `notify-send` when on PATH, else no-op with its logged reason); the once schema verbatim; the cache gate and the parallel bound; exit codes; the receipts from Step 1 with what is still open. `CHANGELOG.md` `### Added`: desktop notifications (favorites' scores, pinned or favorite games ending; `notify = […]` in config; `:notify test`); `gameday --once` with `--json`, `--league`, `--live`, `--top`, `--color`.
- [ ] **Step 3: Commit** `docs(v4): wave 4 receipts`.

## Self-review against spec §6

§6.1 trait + backends → Task 1; `App` ownership + gap → Task 1; events (a)(b) on fresh payloads → Task 2; config key and default → Task 1; `:notify test` → Task 2; the five tests → Tasks 1–2 (`with_notifier` is `set_notifier` + `Recording`). §6.2 CLI → Task 3; fetch through the cache with the 15 s gate → Task 3 (`scoreboard_cached_within`); text in the board's grid → Task 3 (`render_text` through `rows.rs`); JSON schema → Task 3 (pinned by test and golden); exit codes → Task 3; the four tests → Task 3. DoD receipts → Task 4. Names: `Kind`/`NotifyState`/`Recording` (Task 1) used by Task 2's tests; `favorited` made `pub(crate)` in Task 2; `draw_rule`/`situation_summary`/`sort_phrase` visibility and `buffer_to_text` in Task 3 only. Test-count ledger: 618 → 622 (T1) → 626 (T2) → 632 (T3); measured numbers win.
