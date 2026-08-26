//! Argument resolution: each catalog argument becomes exactly one argv
//! element, resolved left to right. Input and select run their own modal
//! screens; cancelling either cancels the whole command silently.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::catalog::{ArgSpec, CommandEntry, ContextKey, Selector, StaticContextKey, Validation};
use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};
use crate::origin::Origin;
use crate::ui::{InputOutcome, InputScreen, PickOutcome, PickScreen, Row, Ui};

pub enum Resolution {
    Args(Vec<String>),
    /// A silent exit 0: Esc on a screen, a required input left empty, or a
    /// selector with nothing to offer.
    Cancelled,
}

pub fn resolve_args(
    entry: &CommandEntry,
    origin: &Origin,
    herdr: &HerdrClient,
    ui: &mut dyn Ui,
) -> Result<Resolution, Fatal> {
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
            ArgSpec::Input {
                prompt,
                required,
                default_context,
                validation,
                description,
            } => {
                let initial = default_context
                    .map(|key| static_context(key, origin))
                    .unwrap_or_default();
                let outcome = ui.input(&InputScreen {
                    header: screen_header(description, entry),
                    prompt: prompt.clone(),
                    initial,
                })?;
                let value = match outcome {
                    InputOutcome::Submitted(value) => value,
                    InputOutcome::Cancelled => return Ok(Resolution::Cancelled),
                };
                if *required && value.is_empty() {
                    return Ok(Resolution::Cancelled);
                }
                if matches!(validation, Some(Validation::Directory))
                    && !value.is_empty()
                    && !Path::new(&value).is_dir()
                {
                    return Err(Fatal::new(
                        "command-palette: input is not an existing directory",
                    ));
                }
                // An optional input left empty is still one (empty) argv
                // element, as in the bash version.
                args.push(value);
            }
            ArgSpec::Select {
                selector,
                prompt,
                exclude_context,
                description,
            } => {
                let exclude = exclude_context
                    .map(|key| static_context(key, origin))
                    .unwrap_or_default();
                let rows = selector_rows(herdr, *selector, &exclude)?;
                if rows.is_empty() {
                    return Ok(Resolution::Cancelled);
                }
                let picked = ui.pick(&PickScreen {
                    header: screen_header(description, entry),
                    prompt: prompt.clone(),
                    rows,
                })?;
                match picked {
                    PickOutcome::Selected(id) => args.push(id),
                    PickOutcome::Cancelled => return Ok(Resolution::Cancelled),
                }
            }
        }
    }
    Ok(Resolution::Args(args))
}

/// A screen's own description, falling back to the command's.
fn screen_header(description: &Option<String>, entry: &CommandEntry) -> String {
    description
        .clone()
        .filter(|text| !text.is_empty())
        .or_else(|| entry.description.clone())
        .unwrap_or_default()
}

fn static_context(key: StaticContextKey, origin: &Origin) -> String {
    match key {
        StaticContextKey::PaneId => origin.pane_id.clone(),
        StaticContextKey::TabId => origin.tab_id.clone(),
        StaticContextKey::WorkspaceId => origin.workspace_id.clone(),
        StaticContextKey::Cwd => origin.cwd.clone(),
    }
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

/// The mapping from a named selector to its herdr list command and row shape
/// lives here, never in `commands.json`. Guards mirror the bash version:
/// call failure, `.result.<collection>` array shape, and a null-or-empty id
/// on ANY candidate (checked before exclusion) are fatal; herdr-supplied
/// labels get their `\n`/`\r`/`\t` replaced by spaces before display (a
/// label is herdr-supplied, not catalog-controlled).
fn selector_rows(
    herdr: &HerdrClient,
    selector: Selector,
    exclude: &str,
) -> Result<Vec<Row>, Fatal> {
    match selector {
        Selector::Workspaces => {
            let raw = list_call(herdr, &["workspace", "list"], "workspace list")?;
            workspace_rows(&raw, exclude)
        }
        Selector::Tabs => {
            // `tab list` spans all workspaces but only carries workspace_id,
            // so a second `workspace list` call resolves id -> label for the
            // candidate prefixes.
            let raw = list_call(herdr, &["tab", "list"], "tab list")?;
            let ws_raw = list_call(herdr, &["workspace", "list"], "workspace list")?;
            let labels = workspace_label_map(&ws_raw)?;
            prefixed_rows(&raw, &labels, exclude, &SelectSpec::tabs())
        }
        Selector::Agents => {
            let raw = list_call(herdr, &["agent", "list"], "agent list")?;
            let ws_raw = list_call(herdr, &["workspace", "list"], "workspace list")?;
            let labels = workspace_label_map(&ws_raw)?;
            prefixed_rows(&raw, &labels, exclude, &SelectSpec::agents())
        }
    }
}

fn list_call(herdr: &HerdrClient, argv: &[&str], desc: &str) -> Result<String, Fatal> {
    let output = herdr
        .raw(argv)
        .map_err(|e| Fatal(format!("command-palette: herdr {desc} failed:\n{e}")))?;
    if !output.status.success() {
        return Err(Fatal(format!(
            "command-palette: herdr {desc} failed:\n{}",
            merged_output(&output)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct SelectSpec {
    desc: &'static str,
    collection: &'static str,
    id_field: &'static str,
    /// Field the display label comes from (`label`, or the agents'
    /// `terminal_title_stripped`).
    label_field: &'static str,
    /// Extra prefix between the workspace label and the item label
    /// (`agent: ` for agents).
    label_prefix: &'static str,
}

impl SelectSpec {
    fn tabs() -> Self {
        SelectSpec {
            desc: "tab list",
            collection: "tabs",
            id_field: "tab_id",
            label_field: "label",
            label_prefix: "",
        }
    }

    fn agents() -> Self {
        SelectSpec {
            desc: "agent list",
            collection: "agents",
            id_field: "pane_id",
            label_field: "terminal_title_stripped",
            label_prefix: "agent: ",
        }
    }
}

fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|ch| {
            if matches!(ch, '\n' | '\r' | '\t') {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn collection_items<'v>(
    value: &'v Value,
    collection: &str,
    desc: &str,
) -> Result<&'v Vec<Value>, Fatal> {
    value
        .get("result")
        .and_then(|result| result.get(collection))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Fatal(format!(
                "command-palette: herdr {desc} returned an unexpected shape"
            ))
        })
}

/// The select guards are weaker than the computed-context ones on purpose
/// (bash parity): only null-or-missing-or-empty ids are rejected, and a
/// numeric id is stringified. Control characters in an id are harmless here —
/// rows are structured data, not tab-delimited text.
fn candidate_id(item: &Value, spec: &SelectSpec) -> Result<String, Fatal> {
    let object = item.as_object().ok_or_else(|| {
        Fatal(format!(
            "command-palette: failed to build {} candidates",
            spec.desc
        ))
    })?;
    let id = match object.get(spec.id_field) {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if id.is_empty() => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(other) => Some(other.to_string()),
    };
    id.ok_or_else(|| {
        Fatal(format!(
            "command-palette: herdr {} returned a candidate without {}",
            spec.desc, spec.id_field
        ))
    })
}

fn item_label(item: &Value, field: &str) -> String {
    sanitize_label(item.get(field).and_then(Value::as_str).unwrap_or_default())
}

fn workspace_rows(raw: &str, exclude: &str) -> Result<Vec<Row>, Fatal> {
    let spec = SelectSpec {
        desc: "workspace list",
        collection: "workspaces",
        id_field: "workspace_id",
        label_field: "label",
        label_prefix: "",
    };
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        Fatal(format!(
            "command-palette: herdr {} returned an unexpected shape",
            spec.desc
        ))
    })?;
    let items = collection_items(&value, spec.collection, spec.desc)?;
    // Ids are validated on every candidate, before exclusion.
    let ids: Vec<String> = items
        .iter()
        .map(|item| candidate_id(item, &spec))
        .collect::<Result<_, _>>()?;
    let mut rows = Vec::new();
    for (item, id) in items.iter().zip(ids) {
        if !exclude.is_empty() && id == exclude {
            continue;
        }
        let label = item_label(item, spec.label_field);
        rows.push(Row {
            label: format!("{label} ({id})"),
            id,
        });
    }
    Ok(rows)
}

/// Builds `workspace_id -> sanitized label` for the tabs/agents prefixes. A
/// non-string workspace_id fails the lookup build, as jq's object-key
/// constraint did.
fn workspace_label_map(raw: &str) -> Result<HashMap<String, String>, Fatal> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        Fatal::new("command-palette: herdr workspace list returned an unexpected shape")
    })?;
    let items = collection_items(&value, "workspaces", "workspace list")?;
    let mut labels = HashMap::new();
    for item in items {
        match item.get("workspace_id") {
            None | Some(Value::Null) => continue,
            Some(Value::String(id)) => {
                labels.insert(id.clone(), item_label(item, "label"));
            }
            Some(_) => {
                return Err(Fatal::new(
                    "command-palette: failed to build workspace label lookup",
                ))
            }
        }
    }
    Ok(labels)
}

fn prefixed_rows(
    raw: &str,
    workspace_labels: &HashMap<String, String>,
    exclude: &str,
    spec: &SelectSpec,
) -> Result<Vec<Row>, Fatal> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        Fatal(format!(
            "command-palette: herdr {} returned an unexpected shape",
            spec.desc
        ))
    })?;
    let items = collection_items(&value, spec.collection, spec.desc)?;
    let ids: Vec<String> = items
        .iter()
        .map(|item| candidate_id(item, spec))
        .collect::<Result<_, _>>()?;
    let mut rows = Vec::new();
    for (item, id) in items.iter().zip(ids) {
        if !exclude.is_empty() && id == exclude {
            continue;
        }
        // A candidate without a string workspace_id cannot be prefixed; the
        // jq pipeline errored out of the whole build there too.
        let ws_label = match item.get("workspace_id") {
            Some(Value::String(ws_id)) => workspace_labels
                .get(ws_id)
                .cloned()
                .unwrap_or_else(|| ws_id.clone()),
            _ => {
                return Err(Fatal(format!(
                    "command-palette: failed to build {} candidates",
                    spec.desc
                )))
            }
        };
        let label = item_label(item, spec.label_field);
        rows.push(Row {
            label: format!("{ws_label} / {}{label} ({id})", spec.label_prefix),
            id,
        });
    }
    Ok(rows)
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
    fn workspace_rows_exclude_the_origin_and_embed_ids_in_labels() {
        let raw = r#"{"result":{"workspaces":[
            {"workspace_id":"w1","label":"one"},
            {"workspace_id":"w2","label":"two"}]}}"#;
        let rows = workspace_rows(raw, "w1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "w2");
        assert_eq!(rows[0].label, "two (w2)");
    }

    #[test]
    fn workspace_labels_are_sanitized_for_display() {
        let raw =
            "{\"result\":{\"workspaces\":[{\"workspace_id\":\"w1\",\"label\":\"a\\nb\\tc\"}]}}";
        let rows = workspace_rows(raw, "").unwrap();
        assert_eq!(rows[0].label, "a b c (w1)");
    }

    #[test]
    fn a_workspace_candidate_without_an_id_fails_even_when_excluded_rows_remain() {
        let raw = r#"{"result":{"workspaces":[{"workspace_id":"w1"},{"label":"orphan"}]}}"#;
        let err = workspace_rows(raw, "w1").unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: herdr workspace list returned a candidate without workspace_id"
        );
    }

    #[test]
    fn a_numeric_select_id_is_stringified_not_rejected() {
        let raw = r#"{"result":{"workspaces":[{"workspace_id":7,"label":"seven"}]}}"#;
        let rows = workspace_rows(raw, "").unwrap();
        assert_eq!(rows[0].id, "7");
    }

    #[test]
    fn tab_rows_are_prefixed_with_the_workspace_label() {
        let tabs = r#"{"result":{"tabs":[
            {"tab_id":"w2:t1","workspace_id":"w2","label":"edit"}]}}"#;
        let ws = r#"{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}"#;
        let labels = workspace_label_map(ws).unwrap();
        let rows = prefixed_rows(tabs, &labels, "", &SelectSpec::tabs()).unwrap();
        assert_eq!(rows[0].label, "two / edit (w2:t1)");
    }

    #[test]
    fn an_unknown_workspace_id_prefixes_with_itself() {
        let tabs = r#"{"result":{"tabs":[
            {"tab_id":"w9:t1","workspace_id":"w9","label":"lost"}]}}"#;
        let rows = prefixed_rows(tabs, &HashMap::new(), "", &SelectSpec::tabs()).unwrap();
        assert_eq!(rows[0].label, "w9 / lost (w9:t1)");
    }

    #[test]
    fn agent_rows_use_the_stripped_terminal_title() {
        let agents = r#"{"result":{"agents":[
            {"pane_id":"w1:p9","workspace_id":"w1","terminal_title_stripped":"claude"}]}}"#;
        let ws = r#"{"result":{"workspaces":[{"workspace_id":"w1","label":"one"}]}}"#;
        let labels = workspace_label_map(ws).unwrap();
        let rows = prefixed_rows(agents, &labels, "", &SelectSpec::agents()).unwrap();
        assert_eq!(rows[0].label, "one / agent: claude (w1:p9)");
    }

    #[test]
    fn a_tab_without_a_string_workspace_id_fails_the_candidate_build() {
        let tabs = r#"{"result":{"tabs":[{"tab_id":"t1"}]}}"#;
        let err = prefixed_rows(tabs, &HashMap::new(), "", &SelectSpec::tabs()).unwrap_err();
        assert_eq!(
            err.0,
            "command-palette: failed to build tab list candidates"
        );
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
