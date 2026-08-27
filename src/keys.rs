//! Keybinding hints for the picker's right-aligned column.
//!
//! herdr resolves bindings in-process and exposes them nowhere (no API
//! endpoint, no CLI listing), so the hints are rebuilt from the same inputs:
//! the user's config.toml — `[keys]` overrides and `[[keys.command]]` plugin
//! bindings, user wins — over the defaults recorded in the catalog (`key`
//! fields, confronted with `herdr --default-config` by
//! scripts/check-compat.sh so they cannot drift silently).
//!
//! Everything here is display-only: a wrong value would mislabel a row, not
//! break dispatch, so any read or parse failure degrades to "no hint".

use std::collections::HashMap;
use std::path::PathBuf;

pub struct KeyHints {
    /// `[keys]` field name → first binding string ("" disables a default).
    overrides: HashMap<String, String>,
    /// `[[keys.command]]` plugin_action target (qualified id) → first key.
    commands: HashMap<String, String>,
}

impl KeyHints {
    pub fn load() -> KeyHints {
        config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| KeyHints::from_toml(&text))
            .unwrap_or_else(KeyHints::empty)
    }

    pub(crate) fn empty() -> KeyHints {
        KeyHints {
            overrides: HashMap::new(),
            commands: HashMap::new(),
        }
    }

    pub(crate) fn from_toml(text: &str) -> KeyHints {
        let Ok(root) = text.parse::<toml::Table>() else {
            return KeyHints::empty();
        };
        let mut hints = KeyHints::empty();
        let Some(keys) = root.get("keys").and_then(|value| value.as_table()) else {
            return hints;
        };
        for (field, value) in keys {
            if field == "command" {
                let Some(entries) = value.as_array() else {
                    continue;
                };
                for entry in entries {
                    let Some(entry) = entry.as_table() else {
                        continue;
                    };
                    if entry.get("type").and_then(|value| value.as_str()) != Some("plugin_action") {
                        continue;
                    }
                    let (Some(command), Some(key)) = (
                        entry.get("command").and_then(|value| value.as_str()),
                        entry.get("key").and_then(first_binding),
                    ) else {
                        continue;
                    };
                    hints.commands.insert(command.to_string(), key);
                }
            } else if let Some(binding) = first_binding(value) {
                hints.overrides.insert(field.clone(), binding);
            }
        }
        hints
    }

    /// The hint for a catalog row. A user override wins — including the
    /// empty string, herdr's way of disabling a default binding. Empty
    /// result means no hint.
    pub fn native(&self, keys_action: Option<&str>, default_key: Option<&str>) -> String {
        if let Some(over) = keys_action.and_then(|field| self.overrides.get(field)) {
            return over.clone();
        }
        default_key.unwrap_or("").to_string()
    }

    /// The hint for a plugin action row. Empty means no binding.
    pub fn plugin(&self, qid: &str) -> String {
        self.commands.get(qid).cloned().unwrap_or_default()
    }
}

/// A binding value is a string or an array of strings (herdr's
/// `BindingConfig`); the hint shows the first entry.
fn first_binding(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(binding) => Some(binding.clone()),
        toml::Value::Array(items) => items
            .first()
            .and_then(|item| item.as_str())
            .map(str::to_string),
        _ => None,
    }
}

/// Mirrors herdr's own lookup (its config/io.rs): `$HERDR_CONFIG_PATH`
/// verbatim, else `$XDG_CONFIG_HOME`, else `$HOME/.config`. The env override
/// is also the test seam.
fn config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("HERDR_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }
    let dir = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => PathBuf::from(std::env::var("HOME").ok()?).join(".config"),
    };
    Some(dir.join("herdr").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[keys]
next_tab = ["prefix+n", "ctrl+alt+bracketright"]
zoom = "ctrl+alt+z"
close_tab = ""

[[keys.command]]
key = "ctrl+shift+p"
type = "plugin_action"
command = "vjeantet.palette.open"

[[keys.command]]
key = ["prefix+f", "ctrl+alt+f"]
type = "plugin_action"
command = "herdr-file-viewer.open-file-viewer"

[[keys.command]]
key = "prefix+g"
type = "popup"
command = "lazygit"
"#;

    #[test]
    fn a_catalog_default_shows_when_the_user_did_not_override_it() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.native(Some("new_tab"), Some("prefix+c")), "prefix+c");
    }

    #[test]
    fn a_user_override_beats_the_catalog_default() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.native(Some("zoom"), Some("prefix+z")), "ctrl+alt+z");
    }

    #[test]
    fn an_array_override_shows_its_first_binding() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.native(Some("next_tab"), Some("prefix+n")), "prefix+n");
    }

    #[test]
    fn an_empty_override_disables_the_default() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.native(Some("close_tab"), Some("prefix+shift+x")), "");
    }

    #[test]
    fn an_operation_without_keys_action_or_default_has_no_hint() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.native(None, None), "");
    }

    #[test]
    fn a_plugin_action_binding_is_found_by_qualified_id() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.plugin("vjeantet.palette.open"), "ctrl+shift+p");
    }

    #[test]
    fn an_array_plugin_binding_shows_its_first_key() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(
            hints.plugin("herdr-file-viewer.open-file-viewer"),
            "prefix+f"
        );
    }

    #[test]
    fn non_plugin_action_command_bindings_are_ignored() {
        let hints = KeyHints::from_toml(CONFIG);
        assert_eq!(hints.plugin("lazygit"), "");
    }

    #[test]
    fn invalid_toml_yields_no_hints() {
        let hints = KeyHints::from_toml("not toml [");
        assert_eq!(hints.native(Some("zoom"), Some("prefix+z")), "prefix+z");
        assert_eq!(hints.plugin("vjeantet.palette.open"), "");
    }
}
