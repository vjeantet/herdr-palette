mod actions;
mod catalog;
mod exec;
mod fatal;
mod herdr;
mod open;
mod origin;
mod resolve;
mod rows;
mod ui;

use std::process::ExitCode;

use fatal::Fatal;
use ui::{PickOutcome, PickScreen, Row, Ui};

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    let subcommand = args.next();
    match subcommand.as_deref().and_then(|s| s.to_str()) {
        Some("open") => match open::run() {
            Ok(code) => code,
            Err(err) => {
                // Server-side: no TTY, no keypress wait — stderr is the log.
                eprintln!("{err}");
                ExitCode::from(1)
            }
        },
        Some("ui") => run_ui(),
        _ => {
            eprintln!("usage: herdr-palette <open|ui>");
            ExitCode::from(2)
        }
    }
}

fn run_ui() -> ExitCode {
    let mut ui = match ui::from_env() {
        Ok(ui) => ui,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    match ui_flow(ui.as_mut()) {
        Ok(code) => code,
        Err(err) => ui.fatal(&err.0),
    }
}

fn ui_flow(ui: &mut dyn Ui) -> Result<ExitCode, Fatal> {
    let origin = origin::Origin::from_env()?;
    let root = catalog::plugin_root()?;
    let catalog = catalog::load_catalog(&root)?;
    let herdr = herdr::HerdrClient::from_env();

    let header = protocol_header(&catalog, &herdr);
    let mut rows = rows::catalog_rows(&catalog);
    rows.extend(actions::plugin_rows(&herdr));

    let picked = ui.pick(&PickScreen {
        header,
        prompt: "herdr > ".to_string(),
        rows,
    })?;
    let selected_id = match picked {
        PickOutcome::Selected(id) => id,
        PickOutcome::Cancelled => return Ok(ExitCode::SUCCESS),
    };

    // A plugin row dispatches straight to herdr's plugin runner. None of the
    // catalog machinery below (arguments, confirmation, argv assembly)
    // applies to it: a plugin action takes no arguments from us.
    if let Some(qid) = selected_id.strip_prefix("plugin:") {
        actions::run_plugin_action(qid, &herdr)?;
        return Ok(ExitCode::SUCCESS);
    }

    let entry = catalog
        .commands
        .iter()
        .find(|command| command.id == selected_id)
        .ok_or_else(|| {
            Fatal::new("command-palette: internal error: selected command not found in catalog")
        })?;
    let [group, subcommand] = entry.command.as_slice() else {
        return Err(Fatal(format!(
            "command-palette: invalid command entry {}: expected [group, subcommand]",
            entry.id
        )));
    };

    let args = match resolve::resolve_args(entry, &origin, &herdr, ui)? {
        resolve::Resolution::Args(args) => args,
        resolve::Resolution::Cancelled => return Ok(ExitCode::SUCCESS),
    };

    // No is listed first so the cursor starts on it: a reflexive Enter
    // cancels instead of confirming. No and Esc are clean cancels.
    if let Some(confirm) = entry.confirm.as_deref().filter(|text| !text.is_empty()) {
        let choice = ui.pick(&PickScreen {
            header: confirm.to_string(),
            prompt: "confirm > ".to_string(),
            rows: vec![Row::plain("No"), Row::plain("Yes")],
        })?;
        match choice {
            PickOutcome::Selected(answer) if answer == "Yes" => {}
            _ => return Ok(ExitCode::SUCCESS),
        }
    }

    exec::execute(&herdr, group, subcommand, &args)?;
    Ok(ExitCode::SUCCESS)
}

/// The picker header doubles as the protocol warning channel: neither an
/// unreadable protocol nor a mismatch blocks execution. Empty means no
/// warning — the TUI then drops the header line entirely.
fn protocol_header(catalog: &catalog::Catalog, herdr: &herdr::HerdrClient) -> String {
    let expected = catalog
        .expected_herdr_protocol
        .map(|protocol| protocol.to_string())
        .unwrap_or_else(|| "null".to_string());
    match herdr.api_schema_protocol() {
        None => "warning: could not read protocol from herdr api schema".to_string(),
        Some(actual) if actual.is_empty() => {
            "warning: could not read protocol from herdr api schema".to_string()
        }
        Some(actual) if actual != expected => {
            format!("warning: catalog expects herdr protocol {expected}, herdr reports {actual}")
        }
        Some(_) => String::new(),
    }
}
