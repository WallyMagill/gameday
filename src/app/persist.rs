//! Config and pin persistence, and the config-error gate that refuses saves
//! while `config.toml` does not parse.

use super::App;
use crate::config::save_pins;

impl App {
    /// The only place config.toml is written. A config we could not parse is
    /// never overwritten: the save is skipped and the footer says why, every
    /// time, so the user can go fix the file.
    pub fn persist_config(&mut self) {
        if let Some(err) = &self.config_error {
            self.status_line = Some(format!("not saving: {err}"));
            return;
        }
        if let Err(e) = self.config.save_to(&self.config_dir) {
            self.status_line = Some(format!("config save failed: {e}"));
        }
    }

    /// The only place pins.json is written from a key the user pressed; same
    /// refusal as `persist_config`, and it says so.
    pub fn persist_pins(&mut self) {
        if let Some(err) = &self.config_error {
            self.status_line = Some(format!("not saving: {err}"));
            return;
        }
        self.persist_pins_quiet();
    }

    /// The same write from a background path (the prune inside `apply_boards`,
    /// which runs on every poll). A broken config skips it in silence: the
    /// startup status line already says saving is off, and re-toasting it
    /// every merge would stomp whatever the user's last key said.
    pub fn persist_pins_quiet(&mut self) {
        if self.config_error.is_some() {
            return;
        }
        if let Err(e) = save_pins(&self.config_dir, &self.pins) {
            self.status_line = Some(format!("pins save failed: {e}"));
        }
    }

    /// Record a config/pins parse failure: it blocks every save and takes the
    /// footer once, at startup, so the reason is on screen and not only on the
    /// stderr that the alternate screen swallowed.
    pub fn set_config_error(&mut self, err: Option<String>) {
        self.status_line = err
            .as_ref()
            .map(|e| format!("config error: {e} — not saving until fixed"));
        self.config_error = err;
    }
}
