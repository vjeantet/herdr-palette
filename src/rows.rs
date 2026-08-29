//! Builds the main picker's rows. User rows come first — commands, then
//! prompts, each in the order of the user's own file; then catalog rows, in catalog order (the file's order is
//! the presentation order); then plugin action rows, sorted among themselves
//! by label.

use crate::catalog::Catalog;
use crate::custom::UserCommand;
use crate::keys::KeyHints;
use crate::prompt::UserPrompt;
use crate::ui::Row;

/// The user's own entries, ahead of everything else: a handful of hand-written
/// rows are what someone wants to see first on an empty query (the fuzzy
/// matcher reorders from the first keystroke anyway).
///
/// Keyed `user:<id>`, which no catalog id can collide with — ids are held to
/// `^[a-z0-9._-]+$` on both sides, and that excludes the colon, exactly as it
/// does for `plugin:`. No keybinding hint: herdr can only bind a key to a
/// plugin action, never to one of these.
pub fn user_rows(commands: &[UserCommand]) -> Vec<Row> {
    commands
        .iter()
        .map(|command| Row {
            id: format!("user:{}", command.id),
            label: format!("User: {}", command.title),
            hint: String::new(),
        })
        .collect()
}

/// The user's `[[prompt]]` entries, keyed `prompt:<id>` — a third namespace,
/// disjoint from `user:` for the same reason that one is disjoint from
/// `plugin:`.
pub fn prompt_rows(prompts: &[UserPrompt]) -> Vec<Row> {
    prompts
        .iter()
        .map(|prompt| Row {
            id: format!("prompt:{}", prompt.id),
            label: format!("Prompt: {}", prompt.title),
            hint: String::new(),
        })
        .collect()
}

pub fn catalog_rows(catalog: &Catalog, hints: &KeyHints) -> Vec<Row> {
    catalog
        .commands
        .iter()
        .map(|command| Row {
            id: command.id.clone(),
            label: command.title.clone(),
            hint: hints.native(command.keys_action.as_deref(), command.key.as_deref()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::{Placement, UserCommand};
    use crate::prompt::Capture;

    fn prompt(id: &str, title: &str) -> UserPrompt {
        UserPrompt {
            id: id.to_string(),
            title: title.to_string(),
            text: Some("t".to_string()),
            argv: Vec::new(),
            capture: Capture::Stdout,
            timeout_ms: 5_000,
            cwd: None,
        }
    }

    fn command(id: &str, title: &str) -> UserCommand {
        UserCommand {
            id: id.to_string(),
            title: title.to_string(),
            argv: vec!["true".to_string()],
            placement: Placement::Split,
            hold: false,
            cwd: None,
            input: None,
        }
    }

    #[test]
    fn a_user_row_is_keyed_in_its_own_namespace() {
        let rows = user_rows(&[command("lazygit", "Lazygit")]);
        assert_eq!(rows[0].id, "user:lazygit");
    }

    #[test]
    fn a_user_row_label_is_prefixed_and_carries_no_hint() {
        let rows = user_rows(&[command("lazygit", "Lazygit")]);
        assert_eq!(rows[0].label, "User: Lazygit");
        assert_eq!(rows[0].hint, "");
    }

    #[test]
    fn user_rows_keep_the_order_of_the_user_file() {
        let rows = user_rows(&[command("b", "Beta"), command("a", "Alpha")]);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["user:b", "user:a"]);
    }

    #[test]
    fn a_prompt_row_is_keyed_in_its_own_namespace() {
        let rows = prompt_rows(&[prompt("review", "Review the diff")]);
        assert_eq!(rows[0].id, "prompt:review");
    }

    #[test]
    fn a_prompt_row_label_is_prefixed_and_carries_no_hint() {
        let rows = prompt_rows(&[prompt("review", "Review the diff")]);
        assert_eq!(rows[0].label, "Prompt: Review the diff");
        assert_eq!(rows[0].hint, "");
    }

    #[test]
    fn prompt_rows_keep_the_order_of_the_user_file() {
        let rows = prompt_rows(&[prompt("b", "Beta"), prompt("a", "Alpha")]);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["prompt:b", "prompt:a"]);
    }
}
