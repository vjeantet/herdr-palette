//! Minimal client for herdr's socket API, used for the one call the CLI does
//! not expose: `pane.send_input` with no keys, which drops a text into a
//! pane's input box without submitting it. Every other herdr call in this
//! program goes through the CLI (`herdr.rs`) and stays observable by the
//! test stub; this one cannot, so it carries its own stub mode.
//!
//! Protocol: newline-delimited JSON, one request per connection. Modelled on
//! `src/ipc.rs` of herdr-scratchpad.
//!
//! Under `PALETTE_STUB=1` no socket is ever opened: the request line is
//! appended to the file named by `PALETTE_STUB_IPC_DUMP`, and the call
//! succeeds — or fails with the message in `PALETTE_STUB_IPC_ERROR` when that
//! is set.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde_json::Value;

/// Where the server listens. The same resolution as herdr's own
/// `session::active_api_socket_path()` (preview sources, `src/session.rs`):
/// `HERDR_SOCKET_PATH` first — herdr sets it for plugin panes and it is
/// inherited from the server otherwise — then the per-session default under
/// the config directory (`$XDG_CONFIG_HOME/herdr` or `~/.config/herdr`).
fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let config_dir = match std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("herdr"),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".config")
            .join("herdr"),
    };
    match std::env::var("HERDR_SESSION")
        .ok()
        .filter(|name| !name.is_empty() && name != "default")
    {
        Some(name) => config_dir.join("sessions").join(name).join("herdr.sock"),
        None => config_dir.join("herdr.sock"),
    }
}

fn stub_mode() -> bool {
    std::env::var_os("PALETTE_STUB").is_some_and(|v| v == "1")
}

/// Sends one request and returns the raw response line.
fn call(method: &str, params: Value) -> Result<String, String> {
    let request = serde_json::json!({
        "id": format!("vjeantet.palette:{method}"),
        "method": method,
        "params": params,
    });
    let line = request.to_string();

    if stub_mode() {
        if let Some(path) = std::env::var_os("PALETTE_STUB_IPC_DUMP") {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|err| format!("cannot write IPC dump: {err}"))?;
            writeln!(file, "{line}").map_err(|err| format!("cannot write IPC dump: {err}"))?;
        }
        return match std::env::var("PALETTE_STUB_IPC_ERROR") {
            Ok(message) if !message.is_empty() => Err(message),
            _ => Ok(String::new()),
        };
    }

    #[cfg(unix)]
    {
        let path = socket_path();
        let mut stream = std::os::unix::net::UnixStream::connect(&path)
            .map_err(|err| format!("cannot reach herdr at {}: {err}", path.display()))?;
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.write_all(b"\n"))
            .and_then(|()| stream.flush())
            .map_err(|err| format!("cannot send to herdr: {err}"))?;
        // Always read the response, even when nothing is done with it: herdr
        // would otherwise write into a pipe already closed on this side.
        let mut response = String::new();
        BufReader::new(&stream)
            .read_line(&mut response)
            .map_err(|err| format!("no response from herdr: {err}"))?;
        Ok(response)
    }
    #[cfg(not(unix))]
    {
        Err("herdr's socket API is only reachable on unix".to_string())
    }
}

/// The error message of a response, if it is one. A failure arrives with a
/// perfectly healthy transport: `{"id":…,"error":{"code":…,"message":…}}`
/// on the same connection a success would use.
pub(crate) fn error_of(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    let error = value.get("error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or("unknown error");
    Some(message.to_string())
}

/// Drops `text` into the input box of `pane_id` without submitting it.
///
/// `keys: []` is the whole point: no Enter follows. herdr wraps the text in
/// bracketed-paste markers itself when the pane has enabled them
/// (`api_helpers.rs`, `encode_api_text`), so a multi-line text lands as one
/// paste — never split it into lines here, and never add the markers.
pub fn send_input(pane_id: &str, text: &str) -> Result<(), String> {
    let line = call(
        "pane.send_input",
        serde_json::json!({
            "pane_id": pane_id,
            "text": text,
            "keys": [],
        }),
    )?;
    match error_of(&line) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_response_is_recognised_by_its_message() {
        let line = r#"{"id":"x","error":{"code":"pane_not_found","message":"no such pane"}}"#;
        assert_eq!(error_of(line).as_deref(), Some("no such pane"));
    }

    #[test]
    fn an_error_without_a_message_falls_back_to_its_code() {
        let line = r#"{"id":"x","error":{"code":"pane_not_found"}}"#;
        assert_eq!(error_of(line).as_deref(), Some("pane_not_found"));
    }

    #[test]
    fn a_success_response_is_not_an_error() {
        assert_eq!(error_of(r#"{"id":"x","result":{}}"#), None);
    }

    #[test]
    fn an_unparsable_line_is_not_reported_as_an_error() {
        assert_eq!(error_of("not json"), None);
        assert_eq!(error_of(""), None);
    }
}
