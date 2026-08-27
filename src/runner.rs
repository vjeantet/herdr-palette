//! The `run` subcommand: the process herdr starts in the pane opened for a
//! user command. It reads the argv out of the environment and spawns it
//! directly — no shell, no interpolation, one JSON array element per argv
//! element.
//!
//! It spawns and waits rather than `exec`ing, because a pane closes as soon as
//! its process exits (herdr 0.8.2: `AppEvent::PaneDied` removes the pane from
//! the layout). Without the wait, a command that fails would take its own
//! error message off the screen with it.

use std::process::{Command, ExitCode};

use crate::fatal::Fatal;
use crate::launch::{ARGV_ENV, HOLD_ENV};

pub fn run() -> Result<ExitCode, Fatal> {
    let argv = argv_from_env()?;
    let hold = std::env::var(HOLD_ENV).is_ok_and(|value| value == "1");

    let status = Command::new(&argv[0]).args(&argv[1..]).status();
    let (code, note) = match status {
        Ok(status) => {
            let code = status.code().unwrap_or(1);
            if code == 0 {
                (0, None)
            } else {
                (code, Some(format!("exited with status {code}")))
            }
        }
        // A missing binary is the likeliest failure of a hand-written entry,
        // and the one that must never vanish silently.
        Err(err) => (127, Some(format!("cannot run {}: {err}", argv[0]))),
    };

    if let Some(note) = &note {
        println!("\r\n[{}] {note}", argv[0]);
    }
    if note.is_some() || hold {
        wait_for_key();
    }
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn argv_from_env() -> Result<Vec<String>, Fatal> {
    let raw = std::env::var(ARGV_ENV)
        .map_err(|_| Fatal(format!("command-palette: {ARGV_ENV} is not set")))?;
    let argv: Vec<String> = serde_json::from_str(&raw)
        .map_err(|err| Fatal(format!("command-palette: invalid {ARGV_ENV}: {err}")))?;
    if argv.is_empty() || argv[0].is_empty() {
        return Err(Fatal(format!("command-palette: {ARGV_ENV} is empty")));
    }
    Ok(argv)
}

/// Holds the pane open on the command's own output — no alternate screen, so
/// what the command printed stays on screen behind the prompt. `PALETTE_STUB`
/// is the test seam: bats runs this subcommand with no keyboard attached.
fn wait_for_key() {
    if std::env::var_os("PALETTE_STUB").is_some_and(|value| value == "1") {
        return;
    }
    println!("(press any key to close)");
    if crossterm::terminal::enable_raw_mode().is_err() {
        return;
    }
    loop {
        match crossterm::event::read() {
            Ok(crossterm::event::Event::Key(key))
                if key.kind == crossterm::event::KeyEventKind::Press =>
            {
                break
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    let _ = crossterm::terminal::disable_raw_mode();
}
