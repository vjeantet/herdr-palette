//! Runs the resolved herdr command. Synchronous on purpose: under popup
//! placement a synchronous focus change survives the popup closing (herdr
//! 0.8.0, measured 2026-08-16), so no deferred/post-close path exists.

use crate::fatal::Fatal;
use crate::herdr::{merged_output, HerdrClient};

pub fn execute(
    herdr: &HerdrClient,
    group: &str,
    subcommand: &str,
    args: &[String],
) -> Result<(), Fatal> {
    // On failure, show group+subcommand+output only; never echo free-input
    // values, selected ids, or the working directory.
    let failed = |detail: String| {
        Fatal(format!(
            "command-palette: herdr {group} {subcommand} failed:\n{detail}"
        ))
    };
    let mut argv: Vec<&str> = vec![group, subcommand];
    argv.extend(args.iter().map(String::as_str));
    let output = herdr.raw(argv).map_err(|e| failed(e.to_string()))?;
    if !output.status.success() {
        return Err(failed(merged_output(&output)));
    }
    Ok(())
}
