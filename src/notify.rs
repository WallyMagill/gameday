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
        crate::log::note_once(
            "notify-noop",
            &format!("notifications off: {}", self.reason),
        );
        Ok(())
    }
    fn name(&self) -> &'static str {
        "noop"
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
}
