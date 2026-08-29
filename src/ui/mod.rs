//! The palette's modal screens, behind one trait so the ratatui
//! implementation and the headless test driver are interchangeable. The flow
//! is sequential modal screens (as the fzf calls were): each call runs its
//! own event loop and returns one outcome.

pub mod headless;
pub mod tui;

use crate::fatal::Fatal;

#[derive(Debug)]
pub struct Row {
    /// Hidden key (field 1 of the old fzf lines): catalog id, `plugin:<qid>`,
    /// or the literal choice on the confirm screen.
    pub id: String,
    /// What the picker displays and fuzzy-matches on (field 2).
    pub label: String,
    /// Keybinding hint, right-aligned and dim, display-only — never matched
    /// on. Empty means none; dropped when the row is too narrow for both.
    pub hint: String,
}

impl Row {
    /// A row whose id is its label (the confirm screen's `No`/`Yes`).
    pub fn plain(text: &str) -> Self {
        Row {
            id: text.to_string(),
            label: text.to_string(),
            hint: String::new(),
        }
    }
}

pub struct PickScreen {
    pub header: String,
    pub prompt: String,
    /// Dim hint shown in place of the query while it is empty. Empty means
    /// no hint (argument selectors and confirms leave it out).
    pub placeholder: String,
    pub rows: Vec<Row>,
    /// The header is a warning, not a description: the TUI renders it so it
    /// cannot be missed (bold, yellow, marked) instead of dimmed like the
    /// rest of the chrome. Ignored by the headless driver.
    pub warning: bool,
}

pub enum PickOutcome {
    /// The id of the chosen row.
    Selected(String),
    /// Esc/Ctrl-C, or Enter with nothing to accept: the palette exits 0
    /// silently, exactly as fzf's rc 130 and rc 1 did.
    Cancelled,
}

/// A free-text line editor (the old `fzf --print-query` over an empty
/// candidate list).
pub struct InputScreen {
    pub header: String,
    pub prompt: String,
    /// Prefill from `default_context`; the user edits or replaces it.
    pub initial: String,
}

pub enum InputOutcome {
    /// The line as submitted — possibly empty, which the caller interprets
    /// (required → silent cancel; optional → an empty argv element).
    Submitted(String),
    /// Esc/Ctrl-C.
    Cancelled,
}

pub trait Ui {
    fn pick(&mut self, screen: &PickScreen) -> Result<PickOutcome, Fatal>;

    fn input(&mut self, screen: &InputScreen) -> Result<InputOutcome, Fatal>;

    /// The bash version's `die`: show `message`, keep the popup readable
    /// (the TUI waits for one keypress), then exit 1. Never returns.
    fn fatal(&mut self, message: &str) -> !;
}

/// `PALETTE_STUB=1` selects the headless driver; anything else, the TUI.
pub fn from_env() -> Result<Box<dyn Ui>, Fatal> {
    if std::env::var_os("PALETTE_STUB").is_some_and(|v| v == "1") {
        Ok(Box::new(headless::HeadlessUi::new()))
    } else {
        Ok(Box::new(tui::TuiUi::new()?))
    }
}
