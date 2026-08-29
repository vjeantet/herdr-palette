//! The `[[prompt]]` half of the user's config file: entries that drop a text
//! into an agent's input box without submitting it.
//!
//! Only the type and its validation live here. Running the argv, talking to
//! herdr and picking the agent are `send.rs` and `ipc.rs`; reading the file is
//! `custom.rs`, which owns the single warning line both tables share.

use serde::Deserialize;

use crate::custom::{validate_cwd, validate_id, validate_title};

/// Default and ceiling for `timeout_ms`. The command runs inside the popup
/// process, whose event loop is suspended meanwhile: a hung command freezes
/// the palette and Ctrl-C does nothing, so the ceiling is not negotiable.
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub const MAX_TIMEOUT_MS: u64 = 60_000;

/// Which streams of the argv end up in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    Stdout,
    /// stdout, then stderr. The two are read through separate pipes, so their
    /// real interleaving is lost by construction.
    Both,
}

impl Capture {
    fn parse(text: &str) -> Option<Capture> {
        match text {
            "stdout" => Some(Capture::Stdout),
            "both" => Some(Capture::Both),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct UserPrompt {
    pub id: String,
    pub title: String,
    /// Fixed text, sent ahead of the command output when there is one.
    pub text: Option<String>,
    /// Command whose output is appended to `text`. Empty means none.
    pub argv: Vec<String>,
    pub capture: Capture,
    pub timeout_ms: u64,
    /// Absolute and existing, or absent: the origin pane's cwd applies then.
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPrompt {
    id: Option<String>,
    title: Option<String>,
    text: Option<String>,
    argv: Option<Vec<String>>,
    capture: Option<String>,
    timeout_ms: Option<u64>,
    cwd: Option<String>,
}

pub(crate) fn parse_entry(
    entry: &toml::Value,
    accepted: &[UserPrompt],
) -> Result<UserPrompt, &'static str> {
    let raw: RawPrompt = entry
        .clone()
        .try_into()
        .map_err(|_| "invalid field types")?;

    let id = validate_id(raw.id)?;
    // Ids are only unique within their own table: `user:` and `prompt:` are
    // distinct row namespaces, so a command and a prompt may share an id.
    if accepted.iter().any(|prompt| prompt.id == id) {
        return Err("duplicate id");
    }
    let title = validate_title(raw.title)?;

    let text = raw.text.filter(|text| !text.is_empty());
    let argv = raw.argv.unwrap_or_default();
    if !argv.is_empty() && argv[0].is_empty() {
        return Err("empty argv[0]");
    }
    if text.is_none() && argv.is_empty() {
        return Err("nothing to send");
    }

    let capture = match raw.capture.as_deref() {
        None => Capture::Stdout,
        Some(text) => Capture::parse(text).ok_or("unknown capture")?,
    };

    let timeout_ms = raw.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err("invalid timeout_ms");
    }

    // Same rule as `[[command]]`, for a different reason: the command runs in
    // the popup, whose cwd is inherited from `open --cwd`. A relative path
    // would resolve against a directory the user never named in the file.
    let cwd = validate_cwd(raw.cwd)?;

    Ok(UserPrompt {
        id,
        title,
        text,
        argv,
        capture,
        timeout_ms,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::parse;

    fn one(body: &str) -> Result<UserPrompt, String> {
        let mut catalog = parse(&format!("[[prompt]]\n{body}\n"));
        if catalog.prompts.len() == 1 {
            Ok(catalog.prompts.remove(0))
        } else {
            Err(catalog.warning)
        }
    }

    #[test]
    fn a_text_only_entry_is_accepted_with_the_defaults() {
        let prompt = one("id = \"x\"\ntitle = \"X\"\ntext = \"hello\"").unwrap();
        assert_eq!(prompt.text.as_deref(), Some("hello"));
        assert!(prompt.argv.is_empty());
        assert_eq!(prompt.capture, Capture::Stdout);
        assert_eq!(prompt.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(prompt.cwd, None);
    }

    #[test]
    fn an_argv_only_entry_is_accepted() {
        let prompt = one("id = \"x\"\ntitle = \"X\"\nargv = [\"git\", \"diff\"]").unwrap();
        assert_eq!(prompt.text, None);
        assert_eq!(prompt.argv, ["git", "diff"]);
    }

    #[test]
    fn capture_and_timeout_travel_as_declared() {
        let prompt = one(
            "id = \"x\"\ntitle = \"X\"\nargv = [\"true\"]\ncapture = \"both\"\ntimeout_ms = 250",
        )
        .unwrap();
        assert_eq!(prompt.capture, Capture::Both);
        assert_eq!(prompt.timeout_ms, 250);
    }

    #[test]
    fn an_entry_with_neither_text_nor_argv_is_rejected() {
        let warning = one("id = \"x\"\ntitle = \"X\"").unwrap_err();
        assert!(warning.ends_with("x (nothing to send)"));
    }

    #[test]
    fn an_empty_text_counts_as_no_text() {
        let warning = one("id = \"x\"\ntitle = \"X\"\ntext = \"\"").unwrap_err();
        assert!(warning.ends_with("x (nothing to send)"));
    }

    #[test]
    fn an_empty_argv_array_counts_as_no_argv() {
        let prompt = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\nargv = []").unwrap();
        assert!(prompt.argv.is_empty());
    }

    #[test]
    fn an_argv_whose_first_element_is_empty_is_rejected() {
        let warning = one("id = \"x\"\ntitle = \"X\"\nargv = [\"\", \"diff\"]").unwrap_err();
        assert!(warning.ends_with("x (empty argv[0])"));
    }

    #[test]
    fn an_unknown_capture_is_rejected() {
        let warning =
            one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ncapture = \"stderr\"").unwrap_err();
        assert!(warning.ends_with("x (unknown capture)"));
    }

    #[test]
    fn a_zero_timeout_is_rejected() {
        let warning = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ntimeout_ms = 0").unwrap_err();
        assert!(warning.ends_with("x (invalid timeout_ms)"));
    }

    #[test]
    fn a_timeout_above_the_ceiling_is_rejected() {
        let warning =
            one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ntimeout_ms = 60001").unwrap_err();
        assert!(warning.ends_with("x (invalid timeout_ms)"));
    }

    #[test]
    fn the_ceiling_itself_is_accepted() {
        let prompt = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ntimeout_ms = 60000").unwrap();
        assert_eq!(prompt.timeout_ms, MAX_TIMEOUT_MS);
    }

    #[test]
    fn a_relative_cwd_is_rejected() {
        let warning = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ncwd = \"src\"").unwrap_err();
        assert!(warning.ends_with("x (cwd is not absolute)"));
    }

    #[test]
    fn a_cwd_naming_no_directory_is_rejected() {
        let warning = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\ncwd = \"/no/such/dir/here\"")
            .unwrap_err();
        assert!(warning.ends_with("x (cwd is not a directory)"));
    }

    #[test]
    fn a_prompt_may_share_an_id_with_a_command() {
        let catalog = parse(
            r#"
[[command]]
id = "x"
title = "Command"
argv = ["true"]

[[prompt]]
id = "x"
title = "Prompt"
text = "t"
"#,
        );
        assert_eq!(catalog.commands.len(), 1);
        assert_eq!(catalog.prompts.len(), 1);
        assert_eq!(catalog.warning, "");
    }

    #[test]
    fn the_second_prompt_sharing_an_id_is_the_one_rejected() {
        let catalog = parse(
            r#"
[[prompt]]
id = "x"
title = "First"
text = "t"

[[prompt]]
id = "x"
title = "Second"
text = "t"
"#,
        );
        assert_eq!(catalog.prompts.len(), 1);
        assert_eq!(catalog.prompts[0].title, "First");
        assert_eq!(
            catalog.warning,
            "warning: 1 user entry skipped: x (duplicate id)"
        );
    }

    #[test]
    fn a_prompt_without_an_id_is_named_by_its_position_in_its_table() {
        let catalog = parse("[[prompt]]\ntitle = \"Nameless\"\ntext = \"t\"\n");
        assert_eq!(
            catalog.warning,
            "warning: 1 user entry skipped: prompt #1 (missing id)"
        );
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let prompt = one("id = \"x\"\ntitle = \"X\"\ntext = \"t\"\nfuture_key = 42").unwrap();
        assert_eq!(prompt.id, "x");
    }
}
