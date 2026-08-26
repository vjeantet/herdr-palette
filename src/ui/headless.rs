//! Headless driver for the bats suite, replacing the old `tests/stubs/fzf`.
//! Same contract, renamed variables:
//!
//!   PALETTE_STUB_SELECT_ID  — applied to every pick screen: select the row
//!                             whose id matches; no match → cancelled;
//!                             unset → the first row.
//!   PALETTE_STUB_SELECT_IDS — newline-separated answers consumed one per
//!                             pick screen (takes precedence; lets one test
//!                             answer differently on successive screens,
//!                             which the fzf stub could never express).
//!   PALETTE_STUB_DUMP       — file APPENDED with every list a pick screen
//!                             was offered, one `id\tlabel` line per row
//!                             (plain `id` when the label is the id, as on
//!                             the confirm screen) — byte-compatible with
//!                             the old FZF_STUB_DUMP assertions.

use std::collections::VecDeque;
use std::io::Write;

use super::{PickOutcome, PickScreen, Ui};
use crate::fatal::Fatal;

pub struct HeadlessUi {
    select_queue: Option<VecDeque<String>>,
}

impl HeadlessUi {
    pub fn new() -> Self {
        let select_queue = std::env::var("PALETTE_STUB_SELECT_IDS")
            .ok()
            .map(|ids| ids.lines().map(str::to_string).collect());
        HeadlessUi { select_queue }
    }

    fn dump(&self, screen: &PickScreen) {
        let Some(path) = std::env::var_os("PALETTE_STUB_DUMP") else {
            return;
        };
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        for row in &screen.rows {
            let line = if row.id == row.label {
                format!("{}\n", row.id)
            } else {
                format!("{}\t{}\n", row.id, row.label)
            };
            let _ = file.write_all(line.as_bytes());
        }
    }
}

impl Ui for HeadlessUi {
    fn pick(&mut self, screen: &PickScreen) -> Result<PickOutcome, Fatal> {
        self.dump(screen);
        let wanted = match &mut self.select_queue {
            Some(queue) => queue.pop_front(),
            None => std::env::var("PALETTE_STUB_SELECT_ID").ok(),
        };
        Ok(match wanted {
            Some(id) => {
                if screen.rows.iter().any(|row| row.id == id) {
                    PickOutcome::Selected(id)
                } else {
                    PickOutcome::Cancelled
                }
            }
            None => match screen.rows.first() {
                Some(row) => PickOutcome::Selected(row.id.clone()),
                None => PickOutcome::Cancelled,
            },
        })
    }

    fn fatal(&mut self, message: &str) -> ! {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
