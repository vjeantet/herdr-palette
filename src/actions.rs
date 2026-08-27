//! The plugin-action half of the palette: rows sourced from
//! `herdr plugin action list`, and their dispatch through
//! `herdr plugin action invoke` plus log polling.
//!
//! Adapted from Jan Tvrdík's jt.command-palette (MIT,
//! https://github.com/JanTvrdik/herdr-command-palette).

use std::time::Duration;

use serde_json::Value;

use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};
use crate::keys::KeyHints;
use crate::ui::Row;

/// Rows are keyed `plugin:<qualified_id>`, which no catalog id can collide
/// with — commands.schema.json restricts ids to `^[a-z0-9._-]+$`, and that
/// excludes the colon. Labels read `<Name>: <title>` with the name derived
/// from the plugin id, so typing a plugin name still finds the row (the
/// picker matches on what it shows) without the raw qualified id as noise.
///
/// Any failure here — the call, its exit status, the JSON — returns no rows
/// and is deliberately not fatal: the built-in half must stay usable on a
/// herdr too old to list plugin actions, or when none are installed. (The
/// bash/jq version dropped the whole plugin half when a single entry was
/// malformed; skipping just that entry is strictly closer to the additive
/// intent.)
pub fn plugin_rows(herdr: &HerdrClient, hints: &KeyHints) -> Vec<Row> {
    let Ok(output) = herdr.raw(["plugin", "action", "list"]) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let self_plugin = std::env::var("HERDR_PLUGIN_ID").unwrap_or_default();
    rows_from_json(
        &String::from_utf8_lossy(&output.stdout),
        &self_plugin,
        host_platform(),
        hints,
    )
}

/// Actions are filtered to this host's platform: without it a Linux user is
/// offered the powershell twins several plugins declare for Windows, which
/// can only fail. An action that declares no platforms at all runs anywhere.
fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        ""
    }
}

fn rows_from_json(raw: &str, self_plugin: &str, platform: &str, hints: &KeyHints) -> Vec<Row> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(actions) = value.pointer("/result/actions").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for action in actions {
        let (Some(plugin_id), Some(action_id), Some(title)) = (
            action.get("plugin_id").and_then(Value::as_str),
            action.get("action_id").and_then(Value::as_str),
            action.get("title").and_then(Value::as_str),
        ) else {
            continue;
        };
        if !self_plugin.is_empty() && plugin_id == self_plugin {
            continue;
        }
        let offered = match action.get("platforms") {
            None | Some(Value::Null) => true,
            Some(Value::Array(platforms)) => {
                platform.is_empty()
                    || platforms
                        .iter()
                        .any(|entry| entry.as_str() == Some(platform))
            }
            Some(_) => false,
        };
        if !offered {
            continue;
        }
        let qid = format!("{plugin_id}.{action_id}");
        rows.push(Row {
            id: format!("plugin:{qid}"),
            label: format!("{}: {title}", plugin_display_name(plugin_id)),
            hint: hints.plugin(&qid),
        });
    }
    // Plugin rows are sorted among themselves by display label; catalog rows
    // keep catalog order ahead of them.
    rows.sort_by(|a, b| a.label.cmp(&b.label));
    rows
}

/// A human plugin name derived from its id: the segment after the last dot
/// (vendor prefixes are not names), minus any `herdr-` prefix, first letter
/// capitalized. `herdr-file-viewer` → `File-viewer`, `jt.command-palette` →
/// `Command-palette`.
fn plugin_display_name(plugin_id: &str) -> String {
    let base = plugin_id.rsplit('.').next().unwrap_or(plugin_id);
    let base = base.strip_prefix("herdr-").unwrap_or(base);
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Invoke one plugin action and wait for the dispatched run to reach a
/// terminal state.
///
/// `plugin action invoke` is fire-and-forget: a zero exit means "accepted",
/// not "succeeded". An action whose command dies afterwards (a moved script
/// exiting 127, say) would otherwise vanish silently along with the popup.
/// Polling that run's own log entry surfaces the failure instead.
pub fn run_plugin_action(qid: &str, herdr: &HerdrClient) -> Result<(), Fatal> {
    let invoke_failed =
        |detail: String| Fatal(format!("command-palette: failed to invoke {qid}\n{detail}"));
    let output = herdr
        .raw(["plugin", "action", "invoke", qid])
        .map_err(|e| invoke_failed(e.to_string()))?;
    if !output.status.success() {
        return Err(invoke_failed(merged_output(&output)));
    }

    // Read plugin_id back from the response rather than splitting it off the
    // action id: a plugin id carries dots of its own (jt.command-palette), so
    // the split is ambiguous. An older herdr that reports no log is left
    // alone — a working invoke must never be made to look broken.
    let response = serde_json::from_slice::<Value>(&output.stdout).unwrap_or(Value::Null);
    let log_field = |name: &str| {
        response
            .pointer(&format!("/result/log/{name}"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let log_id = log_field("log_id");
    let plugin_id = log_field("plugin_id");
    if log_id.is_empty() || plugin_id.is_empty() {
        return Ok(());
    }

    let interval = poll_interval();
    for _ in 0..25 {
        // ~5s at the default 0.2s per turn
        if let Some(entry) = poll_log_entry(herdr, &plugin_id, &log_id) {
            match entry.get("status").and_then(Value::as_str) {
                Some("succeeded") => return Ok(()),
                Some("failed") => {
                    let code = match entry.get("exit_code") {
                        Some(Value::Number(code)) => code.to_string(),
                        Some(Value::String(code)) => code.clone(),
                        _ => "?".to_string(),
                    };
                    let stderr = entry
                        .get("stderr")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    return Err(Fatal(format!(
                        "command-palette: {qid} failed (exit {code})\n{stderr}"
                    )));
                }
                _ => {}
            }
        }
        std::thread::sleep(interval);
    }

    // Still running at the deadline: assume a healthy long-running action.
    Ok(())
}

/// Default 200 ms; HERDR_PALETTE_POLL_INTERVAL_MS overrides it so tests can
/// drain the 25-turn budget in milliseconds.
fn poll_interval() -> Duration {
    Duration::from_millis(
        std::env::var("HERDR_PALETTE_POLL_INTERVAL_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .unwrap_or(200),
    )
}

/// One polling turn. Every failure mode — the call, its status, the JSON, a
/// missing entry — is "nothing yet": keep polling.
fn poll_log_entry(herdr: &HerdrClient, plugin_id: &str, log_id: &str) -> Option<Value> {
    let output = herdr
        .raw([
            "plugin", "log", "list", "--plugin", plugin_id, "--limit", "20",
        ])
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = serde_json::from_slice::<Value>(&output.stdout).ok()?;
    value
        .pointer("/result/logs")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("log_id").and_then(Value::as_str) == Some(log_id))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"result":{"actions":[
        {"plugin_id":"herdr-scratchpad","action_id":"open-scratchpad","title":"Toggle scratchpad","platforms":["linux","macos"]},
        {"plugin_id":"herdr-file-viewer","action_id":"open-file-viewer-windows","title":"Open file viewer","platforms":["windows"]},
        {"plugin_id":"some.plugin","action_id":"bare","title":"No platforms declared"}
    ]}}"#;

    #[test]
    fn actions_for_this_platform_and_platformless_ones_are_offered() {
        let rows = rows_from_json(LIST, "", "linux", &KeyHints::empty());
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "plugin:some.plugin.bare",
                "plugin:herdr-scratchpad.open-scratchpad"
            ]
        );
    }

    #[test]
    fn rows_are_sorted_by_display_label() {
        let rows = rows_from_json(LIST, "", "linux", &KeyHints::empty());
        assert!(rows[0].label < rows[1].label);
    }

    #[test]
    fn a_row_label_reads_plugin_name_colon_action_title() {
        let rows = rows_from_json(LIST, "", "linux", &KeyHints::empty());
        assert_eq!(rows[1].label, "Scratchpad: Toggle scratchpad");
    }

    #[test]
    fn a_plugin_row_shows_its_configured_keybinding_hint() {
        let hints = KeyHints::from_toml(
            "[[keys.command]]\nkey = \"prefix+a\"\ntype = \"plugin_action\"\ncommand = \"herdr-scratchpad.open-scratchpad\"\n",
        );
        let rows = rows_from_json(LIST, "", "linux", &hints);
        assert_eq!(rows[1].hint, "prefix+a");
        assert_eq!(rows[0].hint, "");
    }

    #[test]
    fn a_display_name_drops_the_herdr_prefix_and_capitalizes() {
        assert_eq!(plugin_display_name("herdr-file-viewer"), "File-viewer");
    }

    #[test]
    fn a_display_name_keeps_only_the_segment_after_the_last_dot() {
        assert_eq!(plugin_display_name("jt.command-palette"), "Command-palette");
    }

    #[test]
    fn the_palette_does_not_offer_its_own_actions() {
        let raw = r#"{"result":{"actions":[
            {"plugin_id":"vjeantet.palette","action_id":"open","title":"Command palette"},
            {"plugin_id":"other","action_id":"x","title":"X"}
        ]}}"#;
        let rows = rows_from_json(raw, "vjeantet.palette", "linux", &KeyHints::empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "plugin:other.x");
    }

    #[test]
    fn an_unknown_host_platform_is_offered_everything() {
        let rows = rows_from_json(LIST, "", "", &KeyHints::empty());
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn a_malformed_entry_is_skipped_without_dropping_the_rest() {
        let raw = r#"{"result":{"actions":[
            {"plugin_id":"broken"},
            {"plugin_id":"other","action_id":"x","title":"X"}
        ]}}"#;
        let rows = rows_from_json(raw, "", "linux", &KeyHints::empty());
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn invalid_json_yields_no_rows() {
        assert!(rows_from_json("not json", "", "linux", &KeyHints::empty()).is_empty());
    }
}
