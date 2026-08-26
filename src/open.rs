//! The `open` subcommand — the server-side half (no TTY). Captures the
//! still-focused origin pane/tab/workspace BEFORE the popup steals focus and
//! forwards them via `--env`; the popup process must never re-resolve them.
//! The popup-launch pattern follows Jan Tvrdík's jt.command-palette (MIT,
//! https://github.com/JanTvrdik/herdr-command-palette).

use std::os::unix::process::CommandExt;
use std::path::Path;

use serde_json::Value;

use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};

pub fn run() -> Result<std::process::ExitCode, Fatal> {
    let herdr = HerdrClient::from_env();
    let output = herdr
        .raw(["pane", "current"])
        .map_err(|e| Fatal(format!("command-palette: pane current failed: {e}")))?;
    let merged = merged_output(&output);
    if !output.status.success() {
        return Err(Fatal(format!(
            "command-palette: pane current failed: {}",
            merged.trim_end()
        )));
    }

    let pane = serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|value| value.pointer("/result/pane").cloned());
    let field = |name: &str| -> String {
        pane.as_ref()
            .and_then(|pane| pane.get(name))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let pane_id = field("pane_id");
    let tab_id = field("tab_id");
    let workspace_id = field("workspace_id");
    // Same fallback chain as the bash version: foreground_cwd, then cwd.
    let mut cwd = field("foreground_cwd");
    if cwd.is_empty() {
        cwd = field("cwd");
    }

    if pane_id.is_empty() || tab_id.is_empty() || workspace_id.is_empty() {
        return Err(Fatal(format!(
            "command-palette: could not resolve origin context from: {}",
            merged.trim_end()
        )));
    }

    let mut command = herdr.command();
    command.args([
        "plugin",
        "pane",
        "open",
        "--plugin",
        "vjeantet.palette",
        "--entrypoint",
        "palette",
        "--focus",
    ]);
    command
        .arg("--env")
        .arg(format!("ORIGIN_PANE_ID={pane_id}"));
    command.arg("--env").arg(format!("ORIGIN_TAB_ID={tab_id}"));
    command
        .arg("--env")
        .arg(format!("ORIGIN_WORKSPACE_ID={workspace_id}"));
    command.arg("--env").arg(format!("ORIGIN_CWD={cwd}"));
    if !cwd.is_empty() && Path::new(&cwd).is_dir() {
        command.arg("--cwd").arg(&cwd);
    }

    // exec() replaces this process; it only returns on error.
    let err = command.exec();
    Err(Fatal(format!(
        "command-palette: failed to exec herdr: {err}"
    )))
}
