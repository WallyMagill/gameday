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
    /// Deliver one notification. `Err` means the backend could not even try
    /// (binary missing, spawn refused); a delivery that starts is `Ok`.
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
    /// The name the footer prints after `:notify test`.
    fn name(&self) -> &'static str;
    /// Why this backend never sends anything — [`Noop`]'s only override.
    /// `App::notify_test` reads this to tell "notifications off, here's why"
    /// apart from "sent, but nobody saw it".
    fn reason(&self) -> Option<&'static str> {
        None
    }
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
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Spawn `cmd`, feed it `stdin`, and reap it on a detached thread so a slow
/// tool never blocks the caller; a non-zero exit is logged there.
fn spawn_and_forget(
    mut cmd: Command,
    stdin: Option<String>,
    what: &'static str,
) -> Result<(), String> {
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
    fn name(&self) -> &'static str {
        "osascript"
    }
}

pub struct NotifySend;
impl Notifier for NotifySend {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        let mut cmd = Command::new("notify-send");
        cmd.arg(format!("gameday · {title}")).arg(body);
        spawn_and_forget(cmd, None, "notify-send")
    }
    fn name(&self) -> &'static str {
        "notify-send"
    }
}

/// No backend (Windows, or one that failed): the first `send` logs `reason`
/// to gameday.log so the silence is explained once, and nothing else happens.
pub struct Noop {
    pub reason: &'static str,
}
impl Notifier for Noop {
    fn send(&self, _: &str, _: &str) -> Result<(), String> {
        // Keyed by the reason, not a flat "notify-noop": a backend demoted
        // after a live failure and a platform with no backend at all are two
        // different reasons, and each must still log once — a flat key would
        // let the first reason's note silently swallow the second's.
        crate::log::note_once(
            &format!("notify-noop:{}", self.reason),
            &format!("notifications off: {}", self.reason),
        );
        Ok(())
    }
    fn name(&self) -> &'static str {
        "noop"
    }
    fn reason(&self) -> Option<&'static str> {
        Some(self.reason)
    }
}

/// The test double: every send is appended to a shared log the test holds.
#[derive(Default)]
pub struct Recording {
    pub sent: std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>>,
}
impl Notifier for Recording {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        self.sent
            .borrow_mut()
            .push((title.to_string(), body.to_string()));
        Ok(())
    }
    fn name(&self) -> &'static str {
        "recording"
    }
}

/// The backend this OS gets. Linux needs `notify-send` on PATH; Windows and
/// anything else is the no-op, with the reason it stays quiet.
pub fn os_backend() -> Box<dyn Notifier> {
    if cfg!(target_os = "macos") {
        return Box::new(OsaScript);
    }
    if cfg!(target_os = "linux") {
        let on_path = std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("notify-send").is_file()));
        return if on_path {
            Box::new(NotifySend)
        } else {
            Box::new(Noop {
                reason: "notify-send is not on PATH",
            })
        };
    }
    Box::new(Noop {
        reason: "no notification backend on this OS",
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Score,
    Final,
    Test,
}

/// The gap ledger: game id and kind → the tick a notification last went out.
#[derive(Default)]
pub struct NotifyState {
    sent_at: HashMap<(String, Kind), u64>,
}
impl NotifyState {
    /// Records the send when it allows it.
    pub fn allows(&mut self, game_id: &str, kind: Kind, tick: u64) -> bool {
        if kind == Kind::Test {
            return true;
        }
        let key = (game_id.to_string(), kind);
        if self
            .sent_at
            .get(&key)
            .is_some_and(|&at| tick < at + NOTIFY_MIN_GAP)
        {
            return false;
        }
        self.sent_at.insert(key, tick);
        true
    }

    /// Drop ledger entries for games no board carries any more — the same
    /// bound `apply_boards` already gives `last_scores`: unbounded growth
    /// over a days-long session otherwise.
    pub fn retain(&mut self, alive: impl Fn(&str) -> bool) {
        self.sent_at.retain(|(id, _), _| alive(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_literals_escape_quotes_and_backslashes() {
        assert_eq!(
            applescript_literal(r#"He said "go" \ now"#),
            r#""He said \"go\" \\ now""#
        );
        assert_eq!(applescript_literal(""), r#""""#);
        assert_eq!(
            applescript_literal("line one\r\nline two"),
            r#""line one\r\nline two""#
        );
    }

    #[test]
    fn the_gap_is_per_game_and_kind() {
        let mut s = NotifyState::default();
        assert!(s.allows("g1", Kind::Score, 100));
        assert!(
            !s.allows("g1", Kind::Score, 100 + NOTIFY_MIN_GAP - 1),
            "inside the gap"
        );
        assert!(
            s.allows("g1", Kind::Final, 105),
            "a different kind is its own gap"
        );
        assert!(
            s.allows("g2", Kind::Score, 105),
            "a different game is its own gap"
        );
        assert!(
            s.allows("g1", Kind::Score, 100 + NOTIFY_MIN_GAP),
            "the gap has passed"
        );
        assert!(
            s.allows("g1", Kind::Test, 101) && s.allows("g1", Kind::Test, 102),
            "test ignores the gap"
        );
    }

    #[test]
    fn a_recording_notifier_records_and_a_noop_is_ok() {
        let r = Recording::default();
        r.send("t", "b").unwrap();
        assert_eq!(
            r.sent.borrow().as_slice(),
            &[("t".to_string(), "b".to_string())]
        );
        assert!(Noop { reason: "test" }.send("t", "b").is_ok());
    }

    #[test]
    fn retain_drops_ledger_entries_for_games_no_board_carries_any_more() {
        let mut s = NotifyState::default();
        assert!(s.allows("g1", Kind::Score, 100));
        s.retain(|id| id != "g1");
        assert!(
            s.allows("g1", Kind::Score, 100),
            "g1's ledger entry is gone, so the gap no longer applies"
        );
    }

    /// M11: the no-op path logs its reason once, even across repeated sends
    /// — `note_once` dedupes on the reason-specific key. `log::set_file` is
    /// process-global; this is the only test in the suite that calls it, so
    /// there is nothing else to race it (see the module doc on `Noop::send`).
    #[test]
    fn the_noop_path_logs_its_reason_once() {
        let dir = std::env::temp_dir().join(format!("gd-notify-noop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gameday.log");
        crate::log::set_file(path.clone());
        let reason = "m11-noop-reason-once-only";
        let n = Noop { reason };
        n.send("t", "b").unwrap();
        n.send("t", "b").unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let matching: Vec<&str> = contents
            .lines()
            .filter(|l| l.contains(&format!("notifications off: {reason}")))
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "the reason must be logged exactly once, got: {contents:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
