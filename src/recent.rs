//! The picker's one bit of state: the id of the row picked last time, so it
//! can lead the list on the next opening. It lives next to the user's own
//! entries — `$HERDR_PLUGIN_CONFIG_DIR/last-used` — because that directory is
//! the only one herdr hands the plugin, and it is already the test seam.
//!
//! Neither reading nor writing is ever fatal: losing the memory of one
//! selection must not cost an opening of the palette. No variable means no
//! state, silently, exactly like the user catalog.

use std::path::PathBuf;

const STATE_FILE: &str = "last-used";

fn state_path() -> Option<PathBuf> {
    Some(crate::custom::config_dir()?.join(STATE_FILE))
}

/// The id saved by the previous run, if any. An unreadable or empty file is
/// the same as no file.
pub fn load() -> Option<String> {
    let text = std::fs::read_to_string(state_path()?).ok()?;
    let id = text.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Best effort: a selection that fails to persist is simply not remembered.
pub fn save(id: &str) {
    let Some(path) = state_path() else { return };
    let _ = std::fs::write(path, format!("{id}\n"));
}
