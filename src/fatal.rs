//! The user-visible fatal-error path (the bash version's `die`).

use std::fmt;

/// A fatal error whose message is meant for the user's eyes: rendered on the
/// popup's error screen by the TUI (which then waits for one keypress so the
/// popup does not vanish unread), or printed to stderr in headless and `open`
/// modes. Always followed by exit status 1.
#[derive(Debug)]
pub struct Fatal(pub String);

impl Fatal {
    pub fn new(message: impl Into<String>) -> Self {
        Fatal(message.into())
    }
}

impl fmt::Display for Fatal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Fatal {}
