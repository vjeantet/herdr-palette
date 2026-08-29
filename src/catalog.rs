//! The built-in command catalog (`commands.json`, schema_version 1).
//!
//! Runtime parsing stays as permissive as the bash version's: unknown fields
//! are tolerated, and no JSON-Schema validation happens here — CI's
//! `check-jsonschema` run remains the strict gate. Only `schema_version == 1`
//! and the presence of the fields actually used are hard runtime errors.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::fatal::Fatal;

#[derive(Debug, Deserialize)]
pub struct Catalog {
    #[allow(dead_code)] // checked before typed deserialization; kept for completeness
    pub schema_version: u32,
    /// Compared (as text, like the bash interpolation did) against the
    /// `protocol:` line of `herdr api schema`; a mismatch only produces a
    /// warning header, never a failure.
    pub expected_herdr_protocol: Option<u64>,
    pub commands: Vec<CommandEntry>,
}

#[derive(Debug, Deserialize)]
pub struct CommandEntry {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// `[group, subcommand]` per the schema; length is re-checked at use.
    pub command: Vec<String>,
    #[serde(default)]
    pub arguments: Vec<ArgSpec>,
    /// Presence triggers the No/Yes confirmation screen; the string is its
    /// header.
    pub confirm: Option<String>,
    /// Name of the herdr `[keys]` field bound to this operation, if any —
    /// the user's config.toml override of that field wins over `key`.
    #[serde(default)]
    pub keys_action: Option<String>,
    /// herdr's default binding for that field (kept honest against
    /// `herdr --default-config` by scripts/check-compat.sh).
    #[serde(default)]
    pub key: Option<String>,
}

/// One argument of a catalog command, a discriminated union on `source`.
/// Each resolved argument becomes exactly one argv element.
#[derive(Debug, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ArgSpec {
    Literal {
        value: String,
    },
    Context {
        key: ContextKey,
    },
    Input {
        prompt: String,
        required: bool,
        #[serde(default)]
        default_context: Option<StaticContextKey>,
        #[serde(default)]
        validation: Option<Validation>,
        #[serde(default)]
        description: Option<String>,
    },
    Select {
        selector: Selector,
        prompt: String,
        #[serde(default)]
        exclude_context: Option<StaticContextKey>,
        #[serde(default)]
        description: Option<String>,
    },
}

/// The four origin-derived keys, valid wherever a context value is consumed
/// without a herdr round-trip (`default_context`, `exclude_context`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticContextKey {
    PaneId,
    TabId,
    WorkspaceId,
    Cwd,
}

/// Static keys plus the computed ones, which cost a `workspace list` /
/// `tab list` call and wrap around the origin's position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKey {
    PaneId,
    TabId,
    WorkspaceId,
    Cwd,
    NextWorkspaceId,
    PreviousWorkspaceId,
    NextTabId,
    PreviousTabId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Validation {
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Selector {
    Workspaces,
    Tabs,
    Agents,
}

pub fn plugin_root() -> Result<PathBuf, Fatal> {
    match std::env::var_os("HERDR_PLUGIN_ROOT") {
        Some(root) if !root.is_empty() => Ok(PathBuf::from(root)),
        _ => Err(Fatal::new("command-palette: HERDR_PLUGIN_ROOT is not set")),
    }
}

pub fn load_catalog(plugin_root: &Path) -> Result<Catalog, Fatal> {
    let path = plugin_root.join("commands.json");
    let data = std::fs::read_to_string(&path).map_err(|_| {
        Fatal(format!(
            "command-palette: cannot read catalog: {}",
            path.display()
        ))
    })?;
    parse_catalog(&data)
}

fn parse_catalog(data: &str) -> Result<Catalog, Fatal> {
    let value: Value = serde_json::from_str(data).map_err(|e| {
        Fatal(format!(
            "command-palette: commands.json is not valid JSON: {e}"
        ))
    })?;

    // schema_version is checked before the typed deserialization so a catalog
    // written for another format is reported as such (bash 3b), not as a
    // field-by-field parse error. A missing field renders as "null", exactly
    // as the jq interpolation did.
    let schema_version = value.get("schema_version").cloned().unwrap_or(Value::Null);
    if schema_version != 1 {
        let shown = match &schema_version {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        return Err(Fatal(format!(
            "command-palette: unsupported commands.json schema_version: {shown} (expected 1)"
        )));
    }

    serde_json::from_value(value)
        .map_err(|e| Fatal(format!("command-palette: invalid commands.json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_catalog() -> Catalog {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("commands.json");
        let data = std::fs::read_to_string(path).unwrap();
        parse_catalog(&data).unwrap()
    }

    #[test]
    fn the_repo_catalog_parses_with_all_38_commands() {
        let catalog = repo_catalog();
        assert_eq!(catalog.commands.len(), 38);
        assert_eq!(catalog.expected_herdr_protocol, Some(20));
    }

    #[test]
    fn catalog_order_is_preserved() {
        let catalog = repo_catalog();
        assert_eq!(catalog.commands[0].id, "workspace.switch");
        assert_eq!(catalog.commands.last().unwrap().id, "config.reload");
    }

    #[test]
    fn a_computed_context_argument_deserializes_to_its_key() {
        let catalog = repo_catalog();
        let tab_next = catalog
            .commands
            .iter()
            .find(|c| c.id == "tab.next")
            .unwrap();
        assert_eq!(tab_next.command, ["tab", "focus"]);
        assert!(matches!(
            tab_next.arguments.as_slice(),
            [ArgSpec::Context {
                key: ContextKey::NextTabId
            }]
        ));
    }

    #[test]
    fn an_input_argument_carries_prefill_and_validation() {
        let catalog = repo_catalog();
        let ws_new = catalog
            .commands
            .iter()
            .find(|c| c.id == "workspace.new")
            .unwrap();
        let input = ws_new
            .arguments
            .iter()
            .find(|a| matches!(a, ArgSpec::Input { .. }))
            .unwrap();
        match input {
            ArgSpec::Input {
                required,
                default_context,
                validation,
                ..
            } => {
                assert!(*required);
                assert_eq!(*default_context, Some(StaticContextKey::Cwd));
                assert_eq!(*validation, Some(Validation::Directory));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_confirm_command_exposes_its_prompt_text() {
        let catalog = repo_catalog();
        let close = catalog
            .commands
            .iter()
            .find(|c| c.id == "workspace.close")
            .unwrap();
        assert_eq!(
            close.confirm.as_deref(),
            Some("Close the current workspace?")
        );
    }

    #[test]
    fn a_catalog_with_another_schema_version_is_rejected_by_number() {
        let err = parse_catalog(r#"{"schema_version": 2, "commands": []}"#).unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: unsupported commands.json schema_version: 2 (expected 1)"
        );
    }

    #[test]
    fn a_catalog_without_schema_version_is_rejected_as_null() {
        let err = parse_catalog(r#"{"commands": []}"#).unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: unsupported commands.json schema_version: null (expected 1)"
        );
    }

    #[test]
    fn invalid_json_reports_the_parser_message() {
        let err = parse_catalog("{").unwrap_err();
        assert!(err
            .0
            .starts_with("command-palette: commands.json is not valid JSON: "));
    }

    #[test]
    fn unknown_fields_are_tolerated_at_runtime() {
        let catalog = parse_catalog(
            r#"{
              "schema_version": 1,
              "expected_herdr_protocol": 20,
              "future_field": true,
              "commands": [
                {"id": "x", "title": "X", "command": ["a", "b"], "arguments": [], "extra": 1}
              ]
            }"#,
        )
        .unwrap();
        assert_eq!(catalog.commands[0].id, "x");
    }
}
