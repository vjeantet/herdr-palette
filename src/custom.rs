//! The user's own palette entries, read from
//! `$HERDR_PLUGIN_CONFIG_DIR/config.toml`.
//!
//! Strictly additive: these rows are appended to the picker, never replace or
//! hide a catalog command or a plugin action. Nothing here is fatal — a file
//! the user wrote by hand has no CI gate behind it, so a broken entry is
//! skipped and reported in the picker header while every valid entry, and the
//! whole built-in half, keeps working.
//!
//! Unlike `commands.json` there is no `schema_version`: unknown keys are
//! tolerated and the format only ever grows.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Where the command runs. Passed verbatim to `plugin pane open --placement`,
/// which overrides the manifest's own placement for the `runner` entrypoint
/// (herdr 0.8.2, `api/plugins/mod.rs`: `params.placement.unwrap_or(pane.placement)`).
///
/// `overlay` is deliberately absent: an overlay pane belongs to the tree and
/// becomes the focused pane, which is exactly what this plugin must never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Split,
    Tab,
    Zoomed,
}

impl Placement {
    pub fn as_str(self) -> &'static str {
        match self {
            Placement::Split => "split",
            Placement::Tab => "tab",
            Placement::Zoomed => "zoomed",
        }
    }

    fn parse(text: &str) -> Option<Placement> {
        match text {
            "split" => Some(Placement::Split),
            "tab" => Some(Placement::Tab),
            "zoomed" => Some(Placement::Zoomed),
            _ => None,
        }
    }
}

/// The single free-text argument an entry may ask for. Its value is appended
/// to `argv` as exactly one element — never split, never expanded.
#[derive(Debug)]
pub struct InputSpec {
    pub prompt: String,
    pub required: bool,
}

#[derive(Debug)]
pub struct UserCommand {
    pub id: String,
    pub title: String,
    pub argv: Vec<String>,
    pub placement: Placement,
    /// Keep the pane open after a successful run. A failing run always holds,
    /// whatever this says.
    pub hold: bool,
    pub cwd: Option<String>,
    pub input: Option<InputSpec>,
}

#[derive(Debug, Default)]
pub struct UserCatalog {
    pub commands: Vec<UserCommand>,
    /// Empty means nothing to report. Otherwise a single line for the picker
    /// header, which takes precedence over the protocol warning.
    pub warning: String,
}

/// The directory herdr hands every plugin process (`HERDR_PLUGIN_CONFIG_DIR`).
/// herdr injects it for panes as well as actions and refuses to let `--env`
/// overwrite it, so there is no fallback search: no variable means no user
/// catalog, silently.
fn config_path() -> Option<PathBuf> {
    let dir = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR").filter(|dir| !dir.is_empty())?;
    Some(PathBuf::from(dir).join("config.toml"))
}

pub fn load() -> UserCatalog {
    let Some(path) = config_path() else {
        return UserCatalog::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        // An absent file is the normal case: most users declare nothing.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => UserCatalog::default(),
        Err(err) => UserCatalog {
            commands: Vec::new(),
            warning: format!("warning: cannot read {}: {err}", path.display()),
        },
    }
}

#[derive(Debug, Deserialize)]
struct RawCommand {
    id: Option<String>,
    title: Option<String>,
    argv: Option<Vec<String>>,
    placement: Option<String>,
    hold: Option<bool>,
    cwd: Option<String>,
    input: Option<RawInput>,
}

#[derive(Debug, Deserialize)]
struct RawInput {
    prompt: Option<String>,
    #[serde(default)]
    required: bool,
}

pub(crate) fn parse(text: &str) -> UserCatalog {
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            let detail = err.to_string();
            let first = detail.lines().next().unwrap_or("parse error").to_string();
            return UserCatalog {
                commands: Vec::new(),
                warning: format!("warning: config.toml is not valid TOML: {first}"),
            };
        }
    };
    let Some(entries) = table.get("command").and_then(toml::Value::as_array) else {
        // No [[command]] at all: a config file holding only future settings.
        return UserCatalog::default();
    };

    let mut commands: Vec<UserCommand> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        // The id is read off the raw table first so a rejection can name the
        // entry even when the typed deserialization is what failed.
        let named = entry
            .get("id")
            .and_then(toml::Value::as_str)
            .map(display_name)
            .unwrap_or_else(|| format!("#{}", index + 1));
        match parse_entry(entry, &commands) {
            Ok(command) => commands.push(command),
            Err(reason) => skipped.push(format!("{named} ({reason})")),
        }
    }

    let warning = if skipped.is_empty() {
        String::new()
    } else {
        let plural = if skipped.len() == 1 { "" } else { "s" };
        format!(
            "warning: {} user command{plural} skipped: {}",
            skipped.len(),
            skipped.join(", ")
        )
    };
    UserCatalog { commands, warning }
}

/// An id as the user wrote it, made safe to name in the warning. It is read
/// before any validation, so it may hold anything at all; the header is a
/// single unwrapped line (`ui::tui::rule_line`), which a newline or a stray
/// control byte would garble.
fn display_name(raw: &str) -> String {
    const MAX: usize = 32;
    let cleaned: String = raw
        .chars()
        .take(MAX)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if raw.chars().nth(MAX).is_some() {
        format!("{cleaned}...")
    } else {
        cleaned
    }
}

fn parse_entry(entry: &toml::Value, accepted: &[UserCommand]) -> Result<UserCommand, &'static str> {
    let raw: RawCommand = entry
        .clone()
        .try_into()
        .map_err(|_| "invalid field types")?;

    let id = raw.id.unwrap_or_default();
    if id.is_empty() {
        return Err("missing id");
    }
    // The same shape catalog ids are held to. It keeps `user:<id>` free of
    // tabs and colons, so the row key stays unambiguous against `plugin:`.
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
    {
        return Err("invalid id");
    }
    if accepted.iter().any(|command| command.id == id) {
        return Err("duplicate id");
    }

    let title = raw.title.unwrap_or_default();
    if title.is_empty() {
        return Err("missing title");
    }
    // Rows are rendered — and dumped by the headless driver — as tab-separated
    // fields on one line.
    if title.contains(['\t', '\n', '\r', '\0']) {
        return Err("title spans lines");
    }

    let argv = raw.argv.unwrap_or_default();
    if argv.is_empty() || argv[0].is_empty() {
        return Err("missing argv");
    }

    let placement = match raw.placement.as_deref() {
        None => Placement::Split,
        Some(text) => Placement::parse(text).ok_or("unknown placement")?,
    };

    // A relative cwd would be checked here, in the popup process, and then
    // resolved somewhere else entirely: herdr hands `--cwd` to a bare
    // `PathBuf::from` (0.8.2, `api/plugins/panes.rs`, `plugin_pane_cwd`) and
    // spawns the pane from the server process, whose own working directory has
    // nothing to do with this one. Only an absolute path names the same
    // directory on both sides.
    let cwd = match raw.cwd.filter(|cwd| !cwd.is_empty()) {
        None => None,
        Some(cwd) => {
            let path = Path::new(&cwd);
            if !path.is_absolute() {
                return Err("cwd is not absolute");
            }
            // Rejected at load rather than at launch, where the alternatives
            // are both worse: silently running somewhere else, or failing on a
            // row the picker had shown as fine.
            if !path.is_dir() {
                return Err("cwd is not a directory");
            }
            Some(cwd)
        }
    };

    let input = match raw.input {
        None => None,
        Some(input) => {
            let prompt = input.prompt.unwrap_or_default();
            if prompt.is_empty() {
                return Err("input without prompt");
            }
            Some(InputSpec {
                prompt,
                required: input.required,
            })
        }
    };

    Ok(UserCommand {
        id,
        title,
        argv,
        placement,
        hold: raw.hold.unwrap_or(false),
        cwd,
        input,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
placement = "zoomed"

[[command]]
id = "test"
title = "Cargo test"
argv = ["cargo", "test"]
hold = true

[[command]]
id = "edit"
title = "Edit a file"
argv = ["nvim"]
input = { prompt = "File", required = true }
"#;

    #[test]
    fn every_valid_entry_is_loaded_in_file_order() {
        let catalog = parse(SAMPLE);
        let ids: Vec<&str> = catalog
            .commands
            .iter()
            .map(|command| command.id.as_str())
            .collect();
        assert_eq!(ids, ["lazygit", "test", "edit"]);
        assert_eq!(catalog.warning, "");
    }

    #[test]
    fn placement_defaults_to_split_and_hold_to_false() {
        let catalog = parse(SAMPLE);
        assert_eq!(catalog.commands[1].placement, Placement::Split);
        assert!(catalog.commands[1].hold);
        assert!(!catalog.commands[0].hold);
        assert_eq!(catalog.commands[0].placement, Placement::Zoomed);
    }

    #[test]
    fn an_input_carries_its_prompt_and_requiredness() {
        let catalog = parse(SAMPLE);
        let input = catalog.commands[2].input.as_ref().unwrap();
        assert_eq!(input.prompt, "File");
        assert!(input.required);
    }

    #[test]
    fn an_input_is_optional_unless_declared_required() {
        let catalog = parse(
            r#"[[command]]
id = "x"
title = "X"
argv = ["true"]
input = { prompt = "Value" }"#,
        );
        assert!(!catalog.commands[0].input.as_ref().unwrap().required);
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let catalog = parse(
            r#"[[command]]
id = "x"
title = "X"
argv = ["true"]
future_key = 42"#,
        );
        assert_eq!(catalog.commands.len(), 1);
        assert_eq!(catalog.warning, "");
    }

    #[test]
    fn a_file_without_any_command_table_yields_nothing_and_no_warning() {
        let catalog = parse("some_future_setting = true\n");
        assert!(catalog.commands.is_empty());
        assert_eq!(catalog.warning, "");
    }

    #[test]
    fn invalid_toml_reports_one_warning_and_no_commands() {
        let catalog = parse("[[command]\nid = ");
        assert!(catalog.commands.is_empty());
        assert!(catalog
            .warning
            .starts_with("warning: config.toml is not valid TOML: "));
        assert_eq!(catalog.warning.lines().count(), 1);
    }

    #[test]
    fn a_rejected_entry_never_takes_a_valid_one_with_it() {
        let catalog = parse(
            r#"
[[command]]
id = "broken"
title = "Broken"
argv = []

[[command]]
id = "fine"
title = "Fine"
argv = ["true"]
"#,
        );
        let ids: Vec<&str> = catalog
            .commands
            .iter()
            .map(|command| command.id.as_str())
            .collect();
        assert_eq!(ids, ["fine"]);
        assert_eq!(
            catalog.warning,
            "warning: 1 user command skipped: broken (missing argv)"
        );
    }

    #[test]
    fn several_rejections_are_counted_and_named() {
        let catalog = parse(
            r#"
[[command]]
id = "Deploy"
title = "Deploy"
argv = ["deploy"]

[[command]]
id = "again"
title = "Again"
argv = ["true"]
placement = "popup"
"#,
        );
        assert_eq!(
            catalog.warning,
            "warning: 2 user commands skipped: Deploy (invalid id), again (unknown placement)"
        );
    }

    #[test]
    fn the_second_entry_sharing_an_id_is_the_one_rejected() {
        let catalog = parse(
            r#"
[[command]]
id = "x"
title = "First"
argv = ["first"]

[[command]]
id = "x"
title = "Second"
argv = ["second"]
"#,
        );
        assert_eq!(catalog.commands.len(), 1);
        assert_eq!(catalog.commands[0].title, "First");
        assert_eq!(
            catalog.warning,
            "warning: 1 user command skipped: x (duplicate id)"
        );
    }

    #[test]
    fn an_entry_with_a_wrongly_typed_field_is_named_by_its_id() {
        let catalog = parse(
            r#"
[[command]]
id = "typo"
title = "Typo"
argv = "lazygit"
"#,
        );
        assert_eq!(
            catalog.warning,
            "warning: 1 user command skipped: typo (invalid field types)"
        );
    }

    #[test]
    fn an_entry_without_an_id_is_named_by_its_position() {
        let catalog = parse(
            r#"
[[command]]
title = "Nameless"
argv = ["true"]
"#,
        );
        assert_eq!(
            catalog.warning,
            "warning: 1 user command skipped: #1 (missing id)"
        );
    }

    #[test]
    fn a_title_spanning_lines_is_rejected() {
        let catalog =
            parse("[[command]]\nid = \"x\"\ntitle = \"two\\nlines\"\nargv = [\"true\"]\n");
        assert!(catalog.commands.is_empty());
        assert!(catalog.warning.ends_with("x (title spans lines)"));
    }

    #[test]
    fn an_empty_cwd_reads_as_no_cwd() {
        let catalog = parse(
            r#"[[command]]
id = "x"
title = "X"
argv = ["true"]
cwd = """#,
        );
        assert_eq!(catalog.commands[0].cwd, None);
    }

    fn with_cwd(cwd: &str) -> UserCatalog {
        parse(&format!(
            "[[command]]\nid = \"x\"\ntitle = \"X\"\nargv = [\"true\"]\ncwd = \"{cwd}\"\n"
        ))
    }

    #[test]
    fn an_existing_absolute_cwd_is_kept() {
        let catalog = with_cwd("/");
        assert_eq!(catalog.commands[0].cwd.as_deref(), Some("/"));
        assert_eq!(catalog.warning, "");
    }

    // The popup and the herdr server resolve a relative path against different
    // working directories, so one would be validated here and used there.
    #[test]
    fn a_relative_cwd_is_rejected() {
        let catalog = with_cwd("src");
        assert!(catalog.commands.is_empty());
        assert!(catalog.warning.ends_with("x (cwd is not absolute)"));
    }

    #[test]
    fn a_cwd_naming_no_directory_is_rejected_rather_than_replaced() {
        let catalog = with_cwd("/no/such/directory/here");
        assert!(catalog.commands.is_empty());
        assert!(catalog.warning.ends_with("x (cwd is not a directory)"));
    }

    #[test]
    fn a_rejected_id_cannot_break_the_single_warning_line() {
        let catalog = parse("[[command]]\nid = \"a\\nb\"\ntitle = \"X\"\nargv = [\"true\"]\n");
        assert_eq!(
            catalog.warning,
            "warning: 1 user command skipped: a?b (invalid id)"
        );
        assert_eq!(catalog.warning.lines().count(), 1);
    }

    #[test]
    fn a_very_long_id_is_truncated_in_the_warning() {
        let long = "z".repeat(80);
        let catalog = parse(&format!(
            "[[command]]\nid = \"{long}!\"\ntitle = \"X\"\nargv = [\"true\"]\n"
        ));
        assert_eq!(
            catalog.warning,
            format!(
                "warning: 1 user command skipped: {} (invalid id)",
                "z".repeat(32) + "..."
            )
        );
    }
}
