//! Where the app's notes go once the alternate screen owns the terminal.
//!
//! The mapper's skip lines used to `eprintln!` straight into the TUI, which
//! paints garbage over the board and is lost the moment the screen scrolls.
//! `main` points this at `<config-dir>/gameday.log` right before entering the
//! alternate screen; every other path (`probe`, `dump`, `--help`) never sets a
//! file, so its notes still land on stderr where a script can read them.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Distinct `note_once` keys remembered for the life of the process. One key
/// per (league, event id), so 4096 is more skipped events than nine leagues
/// can produce in a session — the cap exists so a drifted feed with rotating
/// ids can't grow the set without bound, not because 4096 is a measured need.
const MAX_ONCE_KEYS: usize = 4096;

fn sink() -> &'static Mutex<Option<File>> {
    static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(None))
}

fn seen() -> &'static Mutex<HashSet<String>> {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Send notes to `path` (append mode) instead of stderr. A path we cannot
/// open leaves the sink unset — a log file is never worth failing a launch
/// for, and stderr is a working fallback until the TUI covers it.
pub fn set_file(path: PathBuf) {
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    if let Ok(mut sink) = sink().lock() {
        *sink = file;
    }
}

/// One line: `<rfc3339> <msg>`. To the log file when one is set, else stderr.
pub fn note(msg: &str) {
    let stamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    if let Ok(mut sink) = sink().lock() {
        if let Some(file) = sink.as_mut() {
            let _ = writeln!(file, "{stamp} {msg}");
            return;
        }
    }
    eprintln!("{msg}");
}

/// [`note`], but only the first time this `key` is seen. For notes that
/// describe a fact rather than an event — one unmappable game repeats every
/// poll, and saying so 240 times an hour is noise, not information.
pub fn note_once(key: &str, msg: &str) {
    if let Ok(mut seen) = seen().lock() {
        if !seen.insert(key.to_string()) {
            return;
        }
        if seen.len() > MAX_ONCE_KEYS {
            seen.clear();
        }
    }
    note(msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: a note that repeats every poll is written once.
    /// (`set_file` is process-global, so this test never sets it — it asserts
    /// the dedupe, and the stderr fallback simply must not panic.)
    #[test]
    fn note_once_writes_a_key_once_and_note_before_set_file_does_not_panic() {
        let key = "test:note-once";
        note_once(key, "first");
        assert!(seen().lock().unwrap().contains(key));
        let n = seen().lock().unwrap().len();
        note_once(key, "second");
        assert_eq!(seen().lock().unwrap().len(), n, "a repeat adds no key");
        // No file set in the test process: notes go to stderr, quietly.
        note("plain note with no sink");
        assert!(sink().lock().unwrap().is_none(), "no test sets a log file");
    }
}
