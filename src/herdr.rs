//! Subprocess client for the herdr CLI. Every call goes through
//! `${HERDR_BIN_PATH:-herdr}` — the single seam the test stub
//! (`tests/stubs/herdr`) plugs into, exactly as with the bash version.

use std::ffi::OsString;
use std::io;
use std::process::{Command, Output};

pub struct HerdrClient {
    bin: OsString,
}

impl HerdrClient {
    pub fn from_env() -> Self {
        HerdrClient {
            bin: std::env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| OsString::from("herdr")),
        }
    }

    /// A bare `Command` on the herdr binary, for callers that need more than
    /// a captured run (the `open` subcommand exec()s it).
    pub fn command(&self) -> Command {
        Command::new(&self.bin)
    }

    /// Run `herdr <args…>`, capturing stdout and stderr.
    pub fn raw<I, S>(&self, args: I) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.command().args(args).output()
    }

    /// The `protocol:` line of `herdr api schema`, if any. Like the bash
    /// version, the exit status is deliberately ignored and both streams are
    /// searched: an unreadable protocol is a warning header, never an error.
    pub fn api_schema_protocol(&self) -> Option<String> {
        let output = self.raw(["api", "schema"]).ok()?;
        merged_output(&output)
            .lines()
            .find_map(|line| line.strip_prefix("protocol:"))
            .map(|rest| rest.trim_start_matches(' ').to_string())
    }
}

/// stdout followed by stderr, lossily decoded — the equivalent of the bash
/// `2>&1` captures used in every error message.
pub fn merged_output(output: &Output) -> String {
    let mut merged = String::from_utf8_lossy(&output.stdout).into_owned();
    merged.push_str(&String::from_utf8_lossy(&output.stderr));
    merged
}
