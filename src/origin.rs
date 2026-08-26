//! Origin context captured server-side by `herdr-palette open` before the
//! popup takes focus, and forwarded through `--env`. The popup process must
//! never re-run `herdr pane current`: the popup does not hold workspace focus,
//! but the answer would still describe whatever pane the user focused, at a
//! later time than `open` captured it.

use crate::fatal::Fatal;

pub struct Origin {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    /// May be empty; only validated where it is used (`cwd` context and
    /// input prefills).
    pub cwd: String,
}

impl Origin {
    pub fn from_env() -> Result<Self, Fatal> {
        let get = |name: &str| std::env::var(name).unwrap_or_default();
        let origin = Origin {
            pane_id: get("ORIGIN_PANE_ID"),
            tab_id: get("ORIGIN_TAB_ID"),
            workspace_id: get("ORIGIN_WORKSPACE_ID"),
            cwd: get("ORIGIN_CWD"),
        };
        if origin.pane_id.is_empty() || origin.tab_id.is_empty() || origin.workspace_id.is_empty() {
            return Err(Fatal::new(
                "command-palette: missing origin context (ORIGIN_PANE_ID/ORIGIN_TAB_ID/ORIGIN_WORKSPACE_ID)",
            ));
        }
        Ok(origin)
    }
}
