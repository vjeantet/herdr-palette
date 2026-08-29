//! Running one `[[prompt]]` entry: run its argv (if any) inside the popup,
//! assemble the text, pick the agent, drop the text into that agent's input
//! box — never submitted — and focus it.
//!
//! The deposit goes through the socket (`ipc.rs`): no CLI subcommand sends a
//! text without an Enter or without bracketed paste. The focus goes through
//! the CLI on purpose, so it stays observable by the test stub.
//!
//! A non-zero exit status is not a failure here. `cargo test` failing is the
//! very case "explain this error" exists for; `git diff --exit-code` and
//! `grep` exit non-zero with exactly the output wanted. Only having text
//! matters; everything else is a header warning.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::catalog::Selector;
use crate::fatal::Fatal;
use crate::herdr::HerdrClient;
use crate::ipc;
use crate::origin::Origin;
use crate::prompt::{Capture, UserPrompt};
use crate::resolve;
use crate::ui::{PickOutcome, PickScreen, Row, Ui};

/// Budget for the JSON-encoded `text` field. herdr caps a request line at
/// 1 MiB (`api/server.rs`, `MAX_INITIAL_REQUEST_BYTES`) and rejects a paste
/// over the same size; the rest of the line is under a hundred bytes.
const TEXT_BUDGET: usize = 1_000_000;
const TRUNCATION_MARK: &str = "\n[output truncated by herdr-palette]";
/// Suffix on the agent row hosting the pane the palette was opened from.
const ORIGIN_SUFFIX: &str = " - this pane";

pub enum Sent {
    Done,
    /// Nothing to send, no agent to send to, or Esc on the agent picker:
    /// exit 0 in silence, like a cancelled catalog command.
    Cancelled,
}

pub fn run(
    prompt: &UserPrompt,
    origin: &Origin,
    herdr: &HerdrClient,
    ui: &mut dyn Ui,
) -> Result<Sent, Fatal> {
    let captured = if prompt.argv.is_empty() {
        None
    } else {
        let cwd = run_cwd(prompt, origin);
        Some(capture(
            &prompt.argv,
            cwd.as_deref(),
            Duration::from_millis(prompt.timeout_ms),
            prompt.capture,
        ))
    };
    let output = captured
        .as_ref()
        .map(|captured| captured.output.as_str())
        .unwrap_or_default();
    let (text, truncated) = truncate(assemble(prompt.text.as_deref(), output), TEXT_BUDGET);
    let warning = captured
        .as_ref()
        .map(|captured| warning_for(prompt, captured, truncated))
        .unwrap_or_default();

    if text.is_empty() {
        // A command that could not start or run to completion is a mistake
        // in the file, and there is no later screen to show it on.
        return match captured.map(|captured| captured.outcome) {
            Some(Outcome::CannotStart(_)) | Some(Outcome::TimedOut) => {
                Err(Fatal(format!("command-palette: {warning}")))
            }
            _ => Ok(Sent::Cancelled),
        };
    }

    // No exclusion: the agent in the origin pane is the likeliest target.
    let rows = resolve::selector_rows(herdr, Selector::Agents, "")?;
    if rows.is_empty() {
        return Ok(Sent::Cancelled);
    }
    let rows = promote_origin(rows, &origin.pane_id);
    let has_warning = !warning.is_empty();
    let picked = ui.pick(&PickScreen {
        header: if has_warning {
            warning
        } else {
            prompt.title.clone()
        },
        prompt: "agent ▸ ".to_string(),
        placeholder: String::new(),
        rows,
        warning: has_warning,
    })?;
    let pane_id = match picked {
        PickOutcome::Selected(id) => id,
        PickOutcome::Cancelled => return Ok(Sent::Cancelled),
    };

    ipc::send_input(&pane_id, &text).map_err(|message| {
        Fatal(format!(
            "command-palette: cannot send {} to the agent:\n{message}",
            prompt.title
        ))
    })?;
    // The text is already there; failing to switch to it must not turn a
    // successful deposit into an error screen.
    let _ = herdr.raw(["agent", "focus", &pane_id]);
    Ok(Sent::Done)
}

/// The entry's own `cwd`, else the origin pane's. A declared one was checked
/// at load; the origin one is only used when it still exists.
fn run_cwd(prompt: &UserPrompt, origin: &Origin) -> Option<String> {
    prompt.cwd.clone().or_else(|| {
        Some(origin.cwd.clone()).filter(|cwd| !cwd.is_empty() && Path::new(cwd).is_dir())
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// `None` when the process was killed by a signal.
    Exited(Option<i32>),
    /// Killed by us at the deadline. Its output is dropped: a partial capture
    /// would read as a complete one.
    TimedOut,
    CannotStart(String),
}

#[derive(Debug)]
struct Captured {
    output: String,
    outcome: Outcome,
}

/// Runs `argv` with both streams piped and a hard deadline. `std::process`
/// has no bounded wait, and no crate is worth adding for one: the child is
/// polled every 10 ms, the same idiom as the dispatch poll in `actions.rs`.
/// Each stream has its own reader thread — a full pipe would otherwise block
/// the child — and the readers hand their bytes back over channels, so a
/// grandchild keeping a pipe open cannot hang this side past the deadline.
fn capture(argv: &[String], cwd: Option<&str>, timeout: Duration, mode: Capture) -> Captured {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        // The popup's terminal is in raw mode; a command reading it would
        // wait on a keyboard it must never see.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Captured {
                output: String::new(),
                outcome: Outcome::CannotStart(err.to_string()),
            }
        }
    };
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        return Captured {
            output: String::new(),
            outcome: Outcome::TimedOut,
        };
    };

    let remaining = |now: Instant| deadline.saturating_duration_since(now);
    let collect = |rx: Option<mpsc::Receiver<Vec<u8>>>| {
        rx.and_then(|rx| rx.recv_timeout(remaining(Instant::now())).ok())
    };
    let (Some(out), Some(err)) = (collect(stdout), collect(stderr)) else {
        return Captured {
            output: String::new(),
            outcome: Outcome::TimedOut,
        };
    };
    let mut output = String::from_utf8_lossy(&out).into_owned();
    if mode == Capture::Both && !err.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&String::from_utf8_lossy(&err));
    }
    Captured {
        output,
        outcome: Outcome::Exited(status.code()),
    }
}

fn drain<R: Read + Send + 'static>(mut stream: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    rx
}

/// `text`, a newline, then the output stripped of its trailing newlines;
/// either half alone when the other is absent.
fn assemble(text: Option<&str>, output: &str) -> String {
    let output = output.trim_end_matches(['\n', '\r']);
    match text {
        Some(text) if !output.is_empty() => format!("{text}\n{output}"),
        Some(text) => text.to_string(),
        None => output.to_string(),
    }
}

/// Bytes `c` occupies once JSON-encoded by serde_json: the short escapes take
/// two, other control characters six (`\u00XX`), everything else its UTF-8
/// length.
fn json_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// Cuts `text` so that its JSON encoding fits in `budget` bytes, ending it
/// with a visible mark when it had to. Measured on the encoding, not the
/// text: a diff full of tabs and newlines doubles in size once escaped.
fn truncate(text: String, budget: usize) -> (String, bool) {
    let total: usize = text.chars().map(json_len).sum();
    if total <= budget {
        return (text, false);
    }
    let mark_len: usize = TRUNCATION_MARK.chars().map(json_len).sum();
    let mut kept = 0;
    let mut cut = 0;
    for (index, c) in text.char_indices() {
        if kept + json_len(c) + mark_len > budget {
            break;
        }
        kept += json_len(c);
        cut = index + c.len_utf8();
    }
    let mut truncated = text[..cut].to_string();
    truncated.push_str(TRUNCATION_MARK);
    (truncated, true)
}

/// The single header line for the agent picker; empty when there is nothing
/// to say. One thing at a time, most consequential first.
fn warning_for(prompt: &UserPrompt, captured: &Captured, truncated: bool) -> String {
    let program = &prompt.argv[0];
    match &captured.outcome {
        Outcome::CannotStart(err) => format!("warning: cannot run {program}: {err}"),
        Outcome::TimedOut => format!(
            "warning: {program} timed out after {} ms, its output was dropped",
            prompt.timeout_ms
        ),
        Outcome::Exited(code) => {
            if captured.output.is_empty() {
                format!("warning: {program} produced no output")
            } else if truncated {
                "warning: output truncated to fit herdr's 1 MB limit".to_string()
            } else {
                match code {
                    Some(0) => String::new(),
                    Some(code) => format!("warning: {program} exited with status {code}"),
                    None => format!("warning: {program} was killed by a signal"),
                }
            }
        }
    }
}

/// Moves the row for `origin_pane_id` to the top and marks it. The pane the
/// palette was opened from is the likeliest target; when it hosts no agent,
/// no row matches and the order is untouched.
fn promote_origin(mut rows: Vec<Row>, origin_pane_id: &str) -> Vec<Row> {
    if let Some(index) = rows.iter().position(|row| row.id == origin_pane_id) {
        let mut row = rows.remove(index);
        row.label.push_str(ORIGIN_SUFFIX);
        rows.insert(0, row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(argv: &[&str]) -> UserPrompt {
        UserPrompt {
            id: "x".to_string(),
            title: "X".to_string(),
            text: None,
            argv: argv.iter().map(|s| s.to_string()).collect(),
            capture: Capture::Stdout,
            timeout_ms: 5_000,
            cwd: None,
        }
    }

    fn row(id: &str, label: &str) -> Row {
        Row {
            id: id.to_string(),
            label: label.to_string(),
            hint: String::new(),
        }
    }

    #[test]
    fn text_and_output_are_joined_by_one_newline() {
        assert_eq!(assemble(Some("Review:"), "diff\n"), "Review:\ndiff");
    }

    #[test]
    fn the_output_loses_only_its_trailing_newlines() {
        assert_eq!(assemble(None, "a\n\nb\n\n"), "a\n\nb");
    }

    #[test]
    fn text_alone_is_sent_as_is() {
        assert_eq!(assemble(Some("hello\n"), ""), "hello\n");
    }

    #[test]
    fn empty_text_and_empty_output_assemble_to_nothing() {
        assert_eq!(assemble(None, "\n"), "");
    }

    #[test]
    fn a_text_within_budget_is_untouched() {
        let (text, truncated) = truncate("abc".to_string(), 10);
        assert_eq!(text, "abc");
        assert!(!truncated);
    }

    #[test]
    fn a_truncated_text_ends_with_the_mark_and_fits_the_budget() {
        let budget = 200;
        let (text, truncated) = truncate("x".repeat(500), budget);
        assert!(truncated);
        assert!(text.ends_with(TRUNCATION_MARK));
        assert!(serde_json::to_string(&text).unwrap().len() - 2 <= budget);
    }

    // A newline costs two bytes once escaped; the budget is spent on the
    // encoding, so 100 newlines do not fit in 150 bytes.
    #[test]
    fn the_budget_is_measured_on_the_json_encoding() {
        let (text, truncated) = truncate("\n".repeat(100), 150);
        assert!(truncated);
        assert!(serde_json::to_string(&text).unwrap().len() - 2 <= 150);
    }

    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        let (text, _) = truncate("é".repeat(100), 100);
        assert!(text.ends_with(TRUNCATION_MARK));
        assert!(text
            .trim_end_matches(TRUNCATION_MARK)
            .chars()
            .all(|c| c == 'é'));
    }

    #[test]
    fn the_origin_agent_row_moves_to_the_top_and_is_marked() {
        let rows = promote_origin(
            vec![row("w1:p1", "a"), row("w1:p2", "b"), row("w1:p3", "c")],
            "w1:p2",
        );
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["w1:p2", "w1:p1", "w1:p3"]);
        assert_eq!(rows[0].label, format!("b{ORIGIN_SUFFIX}"));
        assert_eq!(rows[1].label, "a");
    }

    #[test]
    fn an_origin_hosting_no_agent_leaves_the_rows_untouched() {
        let rows = promote_origin(vec![row("w1:p1", "a"), row("w1:p2", "b")], "w1:p9");
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["w1:p1", "w1:p2"]);
        assert!(!rows.iter().any(|row| row.label.ends_with(ORIGIN_SUFFIX)));
    }

    #[test]
    fn a_clean_run_with_output_yields_no_warning() {
        let captured = Captured {
            output: "diff".to_string(),
            outcome: Outcome::Exited(Some(0)),
        };
        assert_eq!(warning_for(&prompt(&["git"]), &captured, false), "");
    }

    #[test]
    fn a_nonzero_status_is_a_warning_not_a_failure() {
        let captured = Captured {
            output: "error[E0308]".to_string(),
            outcome: Outcome::Exited(Some(101)),
        };
        assert_eq!(
            warning_for(&prompt(&["cargo", "test"]), &captured, false),
            "warning: cargo exited with status 101"
        );
    }

    #[test]
    fn no_output_is_reported_ahead_of_the_exit_status() {
        let captured = Captured {
            output: String::new(),
            outcome: Outcome::Exited(Some(1)),
        };
        assert_eq!(
            warning_for(&prompt(&["git"]), &captured, false),
            "warning: git produced no output"
        );
    }

    #[test]
    fn truncation_is_reported_ahead_of_the_exit_status() {
        let captured = Captured {
            output: "x".to_string(),
            outcome: Outcome::Exited(Some(1)),
        };
        assert_eq!(
            warning_for(&prompt(&["git"]), &captured, true),
            "warning: output truncated to fit herdr's 1 MB limit"
        );
    }

    #[test]
    fn a_timeout_names_the_program_and_the_limit() {
        let captured = Captured {
            output: String::new(),
            outcome: Outcome::TimedOut,
        };
        assert_eq!(
            warning_for(&prompt(&["sleep"]), &captured, false),
            "warning: sleep timed out after 5000 ms, its output was dropped"
        );
    }

    #[test]
    fn stdout_is_captured_and_the_status_reported() {
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            "echo out; echo err >&2; exit 3".to_string(),
        ];
        let captured = capture(&argv, None, Duration::from_secs(5), Capture::Stdout);
        assert_eq!(captured.output, "out\n");
        assert_eq!(captured.outcome, Outcome::Exited(Some(3)));
    }

    #[test]
    fn both_captures_stdout_then_stderr() {
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            "echo out; echo err >&2".to_string(),
        ];
        let captured = capture(&argv, None, Duration::from_secs(5), Capture::Both);
        assert_eq!(captured.output, "out\nerr\n");
        assert_eq!(captured.outcome, Outcome::Exited(Some(0)));
    }

    #[test]
    fn a_command_past_its_deadline_is_killed_and_its_output_dropped() {
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            "echo early; sleep 5".to_string(),
        ];
        let started = Instant::now();
        let captured = capture(&argv, None, Duration::from_millis(100), Capture::Stdout);
        assert_eq!(captured.outcome, Outcome::TimedOut);
        assert_eq!(captured.output, "");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_program_that_cannot_start_is_reported_as_such() {
        let argv = ["/no/such/binary/here".to_string()];
        let captured = capture(&argv, None, Duration::from_secs(1), Capture::Stdout);
        assert!(matches!(captured.outcome, Outcome::CannotStart(_)));
    }

    #[test]
    fn the_command_runs_in_the_requested_directory() {
        let argv = ["pwd".to_string()];
        let captured = capture(&argv, Some("/"), Duration::from_secs(5), Capture::Stdout);
        assert_eq!(captured.output, "/\n");
    }

    #[test]
    fn a_declared_cwd_wins_over_the_origin_one() {
        let mut entry = prompt(&["true"]);
        entry.cwd = Some("/".to_string());
        let origin = Origin {
            pane_id: "w1:p1".to_string(),
            tab_id: "w1:t1".to_string(),
            workspace_id: "w1".to_string(),
            cwd: "/usr".to_string(),
        };
        assert_eq!(run_cwd(&entry, &origin).as_deref(), Some("/"));
        entry.cwd = None;
        assert_eq!(run_cwd(&entry, &origin).as_deref(), Some("/usr"));
    }
}
