//! Running one user command: resolve its optional input, then hand the argv
//! to herdr, which opens a pane on this plugin's `runner` entrypoint.
//!
//! The argv never touches a shell. It travels as JSON in an environment
//! variable to `herdr-palette run` (src/runner.rs), which spawns it directly.
//! `herdr pane run` is deliberately not used: it joins its arguments with
//! spaces and types the result into the pane's shell without any quoting
//! (herdr 0.8.2, `cli/pane.rs`: `args[1..].join(" ")`), which would turn one
//! argument holding a space into two.

use std::path::Path;

use serde_json::Value;

use crate::custom::{Placement, UserCommand};
use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};
use crate::origin::Origin;
use crate::ui::{InputOutcome, InputScreen, Ui};

/// The manifest pane this plugin exposes purely to host user commands.
const RUNNER_ENTRYPOINT: &str = "runner";

/// Carries the argv to the runner process. Read back in src/runner.rs.
pub const ARGV_ENV: &str = "PALETTE_RUN_ARGV";
/// Set to `1` when the pane must survive a successful run.
pub const HOLD_ENV: &str = "PALETTE_RUN_HOLD";

pub enum Launch {
    Done,
    /// Esc on the input screen, or a required input left empty: exit 0 in
    /// silence, exactly like a cancelled catalog command.
    Cancelled,
}

pub fn run(
    command: &UserCommand,
    origin: &Origin,
    herdr: &HerdrClient,
    ui: &mut dyn Ui,
) -> Result<Launch, Fatal> {
    let mut argv = command.argv.clone();
    if let Some(input) = &command.input {
        let outcome = ui.input(&InputScreen {
            header: command.title.clone(),
            prompt: format!("{} > ", input.prompt),
            initial: String::new(),
        })?;
        let value = match outcome {
            InputOutcome::Submitted(value) => value,
            InputOutcome::Cancelled => return Ok(Launch::Cancelled),
        };
        if value.is_empty() {
            if input.required {
                return Ok(Launch::Cancelled);
            }
        } else {
            // One argv element, whatever it contains. No splitting, no
            // expansion: the runner spawns this array as-is.
            argv.push(value);
        }
    }

    let pane_id = open_pane(command, &argv, origin, herdr)?;
    rename_pane(&pane_id, &command.title, herdr);
    Ok(Launch::Done)
}

fn open_pane(
    command: &UserCommand,
    argv: &[String],
    origin: &Origin,
    herdr: &HerdrClient,
) -> Result<String, Fatal> {
    let argv_json = serde_json::to_string(argv)
        .map_err(|err| Fatal(format!("command-palette: cannot encode command: {err}")))?;
    let args = open_args(command, &argv_json, origin);

    // On failure, name the entry and show herdr's own output — never the argv,
    // which may end in a value the user just typed.
    let failed = |detail: String| {
        Fatal(format!(
            "command-palette: cannot run {}:\n{detail}",
            command.title
        ))
    };
    let output = herdr.raw(&args).map_err(|err| failed(err.to_string()))?;
    if !output.status.success() {
        return Err(failed(merged_output(&output)));
    }

    Ok(serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|value| {
            value
                .pointer("/result/plugin_pane/pane/pane_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default())
}

/// The full `herdr plugin pane open …` argv.
///
/// **The origin flags are mutually exclusive, per placement** — herdr rejects
/// the pair outright (0.8.2, `api/plugins/mod.rs`): a `split` or `zoomed` pane
/// takes `--target-pane` and refuses `--workspace`; a `tab` pane takes
/// `--workspace` and refuses `--target-pane` (and `--direction`). Passing both
/// fails every launch with `invalid_params`. Whichever one applies is always
/// passed explicitly, never left to herdr's "active pane" fallback — the popup
/// makes that notion unreliable.
fn open_args(command: &UserCommand, argv_json: &str, origin: &Origin) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "plugin".to_string(),
        "pane".to_string(),
        "open".to_string(),
        "--plugin".to_string(),
        plugin_id(),
        "--entrypoint".to_string(),
        RUNNER_ENTRYPOINT.to_string(),
        "--placement".to_string(),
        command.placement.as_str().to_string(),
        "--focus".to_string(),
    ];
    match command.placement {
        Placement::Split | Placement::Zoomed => {
            if !origin.pane_id.is_empty() {
                args.push("--target-pane".to_string());
                args.push(origin.pane_id.clone());
            }
        }
        Placement::Tab => {
            if !origin.workspace_id.is_empty() {
                args.push("--workspace".to_string());
                args.push(origin.workspace_id.clone());
            }
        }
    }
    if let Some(cwd) = launch_cwd(command, origin) {
        args.push("--cwd".to_string());
        args.push(cwd);
    }
    args.push("--env".to_string());
    args.push(format!("{ARGV_ENV}={argv_json}"));
    if command.hold {
        args.push("--env".to_string());
        args.push(format!("{HOLD_ENV}=1"));
    }
    args
}

/// The entry's own `cwd`, else the origin pane's. An entry that declared one
/// is honoured or nothing: it was checked at load (absolute, and an existing
/// directory — `custom.rs`), so falling back here would be running the command
/// somewhere the user never named. Neither path being usable leaves the flag
/// out, and herdr falls back to the plugin root.
fn launch_cwd(command: &UserCommand, origin: &Origin) -> Option<String> {
    command.cwd.clone().or_else(|| {
        Some(origin.cwd.clone()).filter(|cwd| !cwd.is_empty() && Path::new(cwd).is_dir())
    })
}

/// Every runner pane would otherwise carry the manifest's static title, since
/// herdr labels a plugin pane from its entrypoint (0.8.2,
/// `finish_plugin_pane_open`). Cosmetic, so a failure here is swallowed: the
/// command is already running.
fn rename_pane(pane_id: &str, title: &str, herdr: &HerdrClient) {
    if pane_id.is_empty() {
        return;
    }
    let _ = herdr.raw(["pane", "rename", pane_id, title]);
}

/// herdr sets HERDR_PLUGIN_ID for every plugin process; the literal is the
/// same fallback `open` uses.
fn plugin_id() -> String {
    std::env::var("HERDR_PLUGIN_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "vjeantet.palette".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::{Placement, UserCommand};

    fn origin() -> Origin {
        Origin {
            pane_id: "w1:p1".to_string(),
            tab_id: "w1:t2".to_string(),
            workspace_id: "w1".to_string(),
            cwd: String::new(),
        }
    }

    fn command(placement: Placement) -> UserCommand {
        UserCommand {
            id: "x".to_string(),
            title: "X".to_string(),
            argv: vec!["true".to_string()],
            placement,
            hold: false,
            cwd: None,
            input: None,
        }
    }

    fn args_for(placement: Placement) -> Vec<String> {
        open_args(&command(placement), "[\"true\"]", &origin())
    }

    // herdr refuses a split or zoomed plugin pane that also names a workspace,
    // and a tab pane that names a target pane. Passing both flags failed every
    // launch with invalid_params.
    #[test]
    fn a_split_targets_the_origin_pane_and_never_names_a_workspace() {
        let args = args_for(Placement::Split);
        assert!(args.windows(2).any(|w| w == ["--target-pane", "w1:p1"]));
        assert!(!args.iter().any(|arg| arg == "--workspace"));
    }

    #[test]
    fn a_zoomed_pane_targets_the_origin_pane_and_never_names_a_workspace() {
        let args = args_for(Placement::Zoomed);
        assert!(args.windows(2).any(|w| w == ["--target-pane", "w1:p1"]));
        assert!(!args.iter().any(|arg| arg == "--workspace"));
    }

    #[test]
    fn a_tab_names_the_origin_workspace_and_never_targets_a_pane() {
        let args = args_for(Placement::Tab);
        assert!(args.windows(2).any(|w| w == ["--workspace", "w1"]));
        assert!(!args.iter().any(|arg| arg == "--target-pane"));
    }

    #[test]
    fn no_placement_ever_asks_for_a_split_direction() {
        for placement in [Placement::Split, Placement::Tab, Placement::Zoomed] {
            assert!(!args_for(placement).iter().any(|arg| arg == "--direction"));
        }
    }

    #[test]
    fn the_placement_travels_as_declared() {
        assert!(args_for(Placement::Zoomed)
            .windows(2)
            .any(|w| w == ["--placement", "zoomed"]));
    }

    #[test]
    fn the_argv_travels_as_one_json_environment_variable() {
        let args = args_for(Placement::Split);
        assert!(args.iter().any(|arg| arg == "PALETTE_RUN_ARGV=[\"true\"]"));
    }

    #[test]
    fn a_declared_cwd_travels_instead_of_the_origin_one() {
        let mut command = command(Placement::Split);
        command.cwd = Some("/".to_string());
        let mut origin = origin();
        origin.cwd = "/usr".to_string();
        let args = open_args(&command, "[]", &origin);
        assert!(args.windows(2).any(|w| w == ["--cwd", "/"]));
    }

    #[test]
    fn an_entry_without_a_cwd_runs_where_the_origin_pane_was() {
        let mut origin = origin();
        origin.cwd = "/usr".to_string();
        let args = open_args(&command(Placement::Split), "[]", &origin);
        assert!(args.windows(2).any(|w| w == ["--cwd", "/usr"]));
    }

    #[test]
    fn an_unusable_origin_cwd_leaves_the_flag_out() {
        let mut origin = origin();
        origin.cwd = "/no/such/directory/here".to_string();
        let args = open_args(&command(Placement::Split), "[]", &origin);
        assert!(!args.iter().any(|arg| arg == "--cwd"));
    }

    #[test]
    fn hold_is_sent_only_when_declared() {
        let args = args_for(Placement::Split);
        assert!(!args.iter().any(|arg| arg.starts_with("PALETTE_RUN_HOLD")));
        let mut held = command(Placement::Split);
        held.hold = true;
        let args = open_args(&held, "[]", &origin());
        assert!(args.iter().any(|arg| arg == "PALETTE_RUN_HOLD=1"));
    }
}
