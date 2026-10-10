//! The `run` subcommand: the process herdr starts in the pane opened for a
//! user command. It reads the argv out of the environment and spawns it
//! directly — no shell, no interpolation, one JSON array element per argv
//! element.
//!
//! It spawns and waits rather than `exec`ing, because a pane closes as soon as
//! its process exits (herdr 0.8.2: `AppEvent::PaneDied` removes the pane from
//! the layout). Without the wait, a command that fails would take its own
//! error message off the screen with it.
//!
//! Waiting also means surviving the keys that stop the command. Ctrl-C and
//! `Ctrl-\` reach the pane's terminal as ordinary input (herdr passes them
//! through), and the terminal sends SIGINT or SIGQUIT to its whole foreground
//! process group: the command, and this runner with it. So the runner ignores
//! both while it waits, and the command gets them back before it starts.

use std::process::{Command, ExitCode};

use crate::fatal::Fatal;
use crate::launch::{ARGV_ENV, HOLD_ENV};

pub fn run() -> Result<ExitCode, Fatal> {
    let argv = argv_from_env()?;
    let hold = std::env::var(HOLD_ENV).is_ok_and(|value| value == "1");

    let status = run_child(&argv);
    let (code, note) = match status {
        Ok(status) => match status.code() {
            Some(0) => (0, None),
            Some(code) => (code, Some(format!("exited with status {code}"))),
            None => {
                let (code, note) = terminated(&status);
                (code, Some(note))
            }
        },
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

/// Runs argv to completion with SIGINT and SIGQUIT ignored here, so only the
/// child dies of them and `run` still reports it and holds. The ignore is
/// never undone: after the wait, what is left is the prompt, which reads Ctrl-C
/// as a key in raw mode, and the exit.
fn run_child(argv: &[String]) -> std::io::Result<std::process::ExitStatus> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // An ignored disposition survives exec, so without the reset the
        // command could not be interrupted at all. The child gets back what
        // the runner was started with, as if it had been started directly.
        //
        // SAFETY: signal is async-signal-safe, and the runner installs no
        // handlers of its own that ignoring these two could displace.
        unsafe {
            let int = libc::signal(libc::SIGINT, libc::SIG_IGN);
            let quit = libc::signal(libc::SIGQUIT, libc::SIG_IGN);
            command.pre_exec(move || {
                libc::signal(libc::SIGINT, int);
                libc::signal(libc::SIGQUIT, quit);
                Ok(())
            });
        }
    }
    command.status()
}

/// A child killed by a signal has no exit code on Unix: `ExitStatus::code()`
/// is `None` there, and reporting "status 1" would send the user looking for a
/// code nothing ever returned. The 128 + N convention is the shells'.
fn terminated(status: &std::process::ExitStatus) -> (i32, String) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return (128 + signal, format!("killed by signal {signal}"));
        }
    }
    (1, "ended without an exit status".to_string())
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
