# One key, one picker, everything a herdr session can do:

- herdr's built-in operations - workspace, tab, pane, agent, config;
- every action of every other installed plugin;
- your own commands, declared in a config file.

A Sublime-Text- or VSCode-style popup: what you type stays on the top line, fuzzy-filtered results
underneath with the matched characters in bold. One Rust binary, no runtime dependency
beyond herdr itself.

![The palette: fuzzy-search the picker, run a built-in command, then one of your own](docs/assets/demo.gif)

## Install

```bash
herdr plugin install vjeantet/herdr-palette
```

Requires herdr 0.8.0+. The manifest's build step downloads the prebuilt binary published for this
version and your platform from this repository's GitHub releases, and verifies its SHA-256. Prebuilt
targets are macOS (Apple silicon and Intel) and Linux (x86_64, aarch64, armv7 - statically linked
against musl, so no glibc version to match). Anywhere else, or when a release is missing, it builds
from source instead and a [Rust toolchain](https://rustup.rs) is required.

For development, herdr runs your checkout directly. Rebuild after every change:

```bash
cargo build --release
herdr plugin link .
```

## Bind a key

In `~/.config/herdr/config.toml`, then `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "vjeantet.palette.open"
description = "Command palette"
```

Drop the `prefix+` for an editor-style single chord. `ctrl+shift+p` requires a terminal that
speaks the kitty keyboard protocol; on terminals that don't, it never fires - use
`ctrl+alt+p` there.

## Use it

Press the key. Fuzzy-search, arrow-select, enter. Esc closes without doing anything.

Some commands ask for more before running: a **free input** (a new name, a directory), a
**selection** from a list (which workspace, tab, agent), or a **Yes/No confirmation** for
anything destructive, defaulting to No. Esc cancels at every step.

Commands act on the pane, tab and workspace you came from, never on the palette itself.

## What is in the picker

**Your own commands** first, prefixed `User:`.

**Built-in operations**, 38 of them: workspace switch/next/previous/new/rename/close, the
same for tabs, pane rename/close/zoom/focus/split/swap/resize/move, worktree
new/open/remove, agent focus/prompt/rename, config reload.

**Plugin actions**, one row each as `Plugin: <title>  <plugin_id>.<action_id>` - the
qualified id is part of the searchable text, so typing a plugin's name finds its actions.
Picking one closes the palette and dispatches the action from a detached process, which
waits for the run to finish: a failing action shows up as a herdr notification instead of
vanishing silently. (herdr allows one popup at a time and the palette is one, so an
action that opens its own popup, the plugin manager for instance, could not run while
the palette was still up.) This palette's own actions and actions declared for another
platform are not listed.

Each half stands alone: if `herdr plugin action list` fails, or no other plugin is
installed, the rest still works.

## Your own commands

Anything you run often enough to want it a keystroke away - `lazygit`, `make test`, a script
of your own. Declare it in the palette's config file:

```bash
herdr plugin config-dir vjeantet.palette   # prints the directory; the file is config.toml
```

```toml
[[command]]
id    = "lazygit"                 # unique among your entries; lowercase, digits, . - _
title = "Lazygit"                 # shown as "User: Lazygit"
argv  = ["lazygit"]               # one array element per argument - never a shell string
placement = "zoomed"              # split (default) | tab | zoomed

[[command]]
id    = "test"
title = "Cargo test"
argv  = ["cargo", "test"]
hold  = true                      # keep the pane open after a run that succeeded
cwd   = "/home/me/project"        # absolute, and must exist; default: the pane you came from

[[command]]
id    = "edit"
title = "Edit a file"
argv  = ["nvim"]
input = { prompt = "File", required = true }   # asked for, appended to argv as one element
```

Picking one opens a pane running that command: `split` next to the pane you came from, `tab`
as a new tab, `zoomed` full-screen. The pane closes when the command exits. A run that
**fails** holds the pane open on its own output, with the exit status, until you press a key;
`hold = true` does the same for a run that succeeded.

Rules that apply to every entry:

- **No shell.** `argv` is an array, not a command line: no globbing, no `$VAR`, no pipes, no
  redirection. Use a script for those. What you type into an `input` becomes exactly one
  argument, spaces and quotes included, unescaped.
- **`argv[0]` is resolved against the herdr server's `PATH`**, not your shell's. A command
  that works in every pane can still fail here with `cannot run …` - name the binary by
  absolute path when that happens.
- **A broken entry is skipped, never fatal.** It is counted in a header line at the top of
  the picker; your other entries, the built-in commands and the plugin actions keep working.
  Invalid TOML costs you your own entries and nothing more. Unknown keys are ignored.
- **Entries only add rows.** They cannot rename, replace, reorder or hide a built-in command
  or a plugin action.

## Your own prompts

A `[[prompt]]` entry drops a text into an agent's input box **without submitting it**, then
focuses that agent. You read it over and press Enter yourself. The text is fixed, produced by
a command, or both:

```toml
[[prompt]]
id      = "review"
title   = "Review the staged diff"          # shown as "Prompt: Review the staged diff"
text    = "Review this diff and flag anything risky:"
argv    = ["git", "diff", "--staged"]       # its output goes under the text, one array element per argument
capture = "stdout"                          # stdout (default) | both (stdout, then stderr)
timeout_ms = 5000                           # default 5000, at most 60000
cwd     = "/home/me/project"                # absolute, and must exist; default: the pane you came from

[[prompt]]
id    = "explain"
title = "Explain the last test failure"
argv  = ["cargo", "test"]
capture = "both"
```

Picking one runs `argv` (if any), then asks which agent to send to - every agent herdr knows,
the one in the pane you came from listed first and marked. The text lands in that agent's
input box as a single paste, however many lines it has, and the agent gets the focus.

- **Nothing is submitted.** The text is added to whatever the agent already had in its box;
  sending it is your keystroke.
- **A non-zero exit status is not a failure.** A failing `cargo test` is exactly what you want
  to paste; `git diff --exit-code` and `grep` exit non-zero with the output you asked for. The
  status is mentioned in the header of the agent picker, and the output is sent anyway. Only
  a command that produced nothing, with no `text` to fall back on, sends nothing.
- **The command runs inside the palette, under a timeout.** The popup is frozen meanwhile,
  so a command that hangs is killed at `timeout_ms` and its output dropped. `argv[0]` is
  resolved against the palette's own `PATH`, that of the pane you came from.
- **No shell**, as for `[[command]]`: no globbing, no `$VAR`, no pipes. Use a script.
- **Big outputs are cut** a little under herdr's 1 MB request limit, with a visible mark at
  the end and a header warning.
- A broken entry is skipped and counted in the same header line as a broken `[[command]]`;
  `id`s are unique per table, so a prompt and a command may share one.

## Scope

This palette covers what herdr's public CLI can do. The design document lists the
[built-in keybindings with no equivalent here](docs/design/command-catalog.md#mapping-to-built-in-keybindings).

Both palettes credited in [LICENSE](LICENSE) also expose an action called `open`; if you run
one of them alongside this one, give it a different key.
