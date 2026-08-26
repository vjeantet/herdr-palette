//! Argument resolution: each catalog argument becomes exactly one argv
//! element, resolved left to right. Input and select screens arrive in M2;
//! until then a command that needs them fails on an explicit error screen
//! rather than silently.

use std::path::Path;

use serde_json::Value;

use crate::catalog::{ArgSpec, CommandEntry, ContextKey};
use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};
use crate::origin::Origin;

pub fn resolve_args(
    entry: &CommandEntry,
    origin: &Origin,
    herdr: &HerdrClient,
) -> Result<Vec<String>, Fatal> {
    let mut args = Vec::with_capacity(entry.arguments.len());
    for spec in &entry.arguments {
        match spec {
            ArgSpec::Literal { value } => args.push(value.clone()),
            ArgSpec::Context { key } => {
                let value = resolve_context_key(*key, origin, herdr)?;
                if *key == ContextKey::Cwd && (value.is_empty() || !Path::new(&value).is_dir()) {
                    return Err(Fatal::new(
                        "command-palette: origin working directory is unavailable or missing",
                    ));
                }
                args.push(value);
            }
            ArgSpec::Input { .. } | ArgSpec::Select { .. } => {
                return Err(Fatal::new(
                    "command-palette: this command is not available yet in the Rust palette (input/select screens land in the next milestone)",
                ));
            }
        }
    }
    Ok(args)
}

fn resolve_context_key(
    key: ContextKey,
    origin: &Origin,
    herdr: &HerdrClient,
) -> Result<String, Fatal> {
    match key {
        ContextKey::PaneId => Ok(origin.pane_id.clone()),
        ContextKey::TabId => Ok(origin.tab_id.clone()),
        ContextKey::WorkspaceId => Ok(origin.workspace_id.clone()),
        ContextKey::Cwd => Ok(origin.cwd.clone()),
        ContextKey::NextWorkspaceId => {
            computed(herdr, ListKind::Workspaces, Direction::Next, origin)
        }
        ContextKey::PreviousWorkspaceId => {
            computed(herdr, ListKind::Workspaces, Direction::Previous, origin)
        }
        ContextKey::NextTabId => computed(herdr, ListKind::Tabs, Direction::Next, origin),
        ContextKey::PreviousTabId => computed(herdr, ListKind::Tabs, Direction::Previous, origin),
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Next,
    Previous,
}

#[derive(Clone, Copy)]
enum ListKind {
    Workspaces,
    Tabs,
}

struct ListSpec {
    /// The herdr command as shown in error messages (`workspace list`).
    desc: &'static str,
    /// Collection key under `.result`.
    collection: &'static str,
    /// Id field of each candidate.
    id_field: &'static str,
}

impl ListKind {
    fn spec(self) -> ListSpec {
        match self {
            ListKind::Workspaces => ListSpec {
                desc: "workspace list",
                collection: "workspaces",
                id_field: "workspace_id",
            },
            ListKind::Tabs => ListSpec {
                desc: "tab list",
                collection: "tabs",
                id_field: "tab_id",
            },
        }
    }
}

fn computed(
    herdr: &HerdrClient,
    kind: ListKind,
    direction: Direction,
    origin: &Origin,
) -> Result<String, Fatal> {
    let spec = kind.spec();
    let output = match kind {
        ListKind::Workspaces => herdr.raw(["workspace", "list"]),
        ListKind::Tabs => herdr.raw(["tab", "list", "--workspace", &origin.workspace_id]),
    }
    .map_err(|e| Fatal(format!("command-palette: herdr {} failed:\n{e}", spec.desc)))?;
    if !output.status.success() {
        return Err(Fatal(format!(
            "command-palette: herdr {} failed:\n{}",
            spec.desc,
            merged_output(&output)
        )));
    }
    let origin_id = match kind {
        ListKind::Workspaces => &origin.workspace_id,
        ListKind::Tabs => &origin.tab_id,
    };
    neighbor_from_json(
        &String::from_utf8_lossy(&output.stdout),
        &spec,
        direction,
        origin_id,
    )
}

/// The wrap-around pick over an ordered id list, with the same three guards
/// the bash/jq version had: array shape, per-candidate id validity (a string,
/// non-empty, no NUL or newline — an id with either could forge picker rows
/// downstream), and origin membership.
fn neighbor_from_json(
    raw: &str,
    spec: &ListSpec,
    direction: Direction,
    origin_id: &str,
) -> Result<String, Fatal> {
    let shape_err = || {
        Fatal(format!(
            "command-palette: herdr {} returned an unexpected shape",
            spec.desc
        ))
    };
    let value: Value = serde_json::from_str(raw).map_err(|_| shape_err())?;
    let items = value
        .get("result")
        .and_then(|result| result.get(spec.collection))
        .and_then(Value::as_array)
        .ok_or_else(shape_err)?;

    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let id = item
            .as_object()
            .and_then(|object| object.get(spec.id_field))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && !id.contains('\0') && !id.contains('\n'))
            .ok_or_else(|| {
                Fatal(format!(
                    "command-palette: herdr {} returned a candidate without a valid {}",
                    spec.desc, spec.id_field
                ))
            })?;
        ids.push(id);
    }

    let index = ids.iter().position(|id| *id == origin_id).ok_or_else(|| {
        Fatal(format!(
            "command-palette: herdr {} did not include the origin {}",
            spec.desc, spec.id_field
        ))
    })?;
    let len = ids.len();
    let target = match direction {
        Direction::Next => (index + 1) % len,
        Direction::Previous => (index + len - 1) % len,
    };
    Ok(ids[target].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_spec() -> ListSpec {
        ListKind::Workspaces.spec()
    }

    fn ws_json(ids: &[&str]) -> String {
        let workspaces: Vec<String> = ids
            .iter()
            .map(|id| format!(r#"{{"workspace_id":"{id}","label":"x"}}"#))
            .collect();
        format!(
            r#"{{"result":{{"workspaces":[{}]}}}}"#,
            workspaces.join(",")
        )
    }

    #[test]
    fn next_picks_the_following_id() {
        let id = neighbor_from_json(
            &ws_json(&["w1", "w2", "w3"]),
            &ws_spec(),
            Direction::Next,
            "w1",
        )
        .unwrap();
        assert_eq!(id, "w2");
    }

    #[test]
    fn next_wraps_from_the_last_id_to_the_first() {
        let id = neighbor_from_json(
            &ws_json(&["w1", "w2", "w3"]),
            &ws_spec(),
            Direction::Next,
            "w3",
        )
        .unwrap();
        assert_eq!(id, "w1");
    }

    #[test]
    fn previous_wraps_from_the_first_id_to_the_last() {
        let id = neighbor_from_json(
            &ws_json(&["w1", "w2", "w3"]),
            &ws_spec(),
            Direction::Previous,
            "w1",
        )
        .unwrap();
        assert_eq!(id, "w3");
    }

    #[test]
    fn a_single_id_resolves_to_itself() {
        let id = neighbor_from_json(&ws_json(&["w1"]), &ws_spec(), Direction::Next, "w1").unwrap();
        assert_eq!(id, "w1");
    }

    #[test]
    fn a_non_array_collection_is_an_unexpected_shape() {
        let err = neighbor_from_json(
            r#"{"result":{"workspaces":{}}}"#,
            &ws_spec(),
            Direction::Next,
            "w1",
        )
        .unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: herdr workspace list returned an unexpected shape"
        );
    }

    #[test]
    fn invalid_json_is_an_unexpected_shape() {
        let err = neighbor_from_json("socket unavailable", &ws_spec(), Direction::Next, "w1")
            .unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: herdr workspace list returned an unexpected shape"
        );
    }

    #[test]
    fn a_candidate_without_an_id_is_rejected() {
        let err = neighbor_from_json(
            r#"{"result":{"workspaces":[{"workspace_id":"w1"},{"label":"missing"}]}}"#,
            &ws_spec(),
            Direction::Next,
            "w1",
        )
        .unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: herdr workspace list returned a candidate without a valid workspace_id"
        );
    }

    #[test]
    fn a_numeric_id_is_rejected() {
        let err = neighbor_from_json(
            r#"{"result":{"workspaces":[{"workspace_id":7}]}}"#,
            &ws_spec(),
            Direction::Next,
            "w1",
        )
        .unwrap_err();
        assert!(err.0.contains("without a valid workspace_id"));
    }

    #[test]
    fn an_id_containing_a_newline_is_rejected() {
        let err = neighbor_from_json(
            "{\"result\":{\"workspaces\":[{\"workspace_id\":\"w\\n1\"}]}}",
            &ws_spec(),
            Direction::Next,
            "w1",
        )
        .unwrap_err();
        assert!(err.0.contains("without a valid workspace_id"));
    }

    #[test]
    fn a_list_omitting_the_origin_is_rejected() {
        let err =
            neighbor_from_json(&ws_json(&["w2"]), &ws_spec(), Direction::Next, "w1").unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: herdr workspace list did not include the origin workspace_id"
        );
    }
}
