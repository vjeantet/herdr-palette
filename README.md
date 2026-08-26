# herdr-palette (vjeantet.palette)

A command palette for herdr that lists **both** halves of what a herdr session can do,
in a single Sublime-Text-style picker behind a single key:

- herdr's own built-in operations — Workspace, Tab, Pane, Agent, Config;
- every action exposed by every other installed plugin.

> **Credits.** The built-in-operations half started from [herdr-command-palette by Hiroyuki
> Ota][ota]; the plugin-action half is modelled on [herdr-command-palette by Jan Tvrdík][jt].
> Both are MIT, and both are credited in [LICENSE](LICENSE) and in source comments. Neither is
> tracked here — this repository is its own project, not a fork kept in sync with either.
> Whichever palette you came from, the point of this one is that you should not need two keys —
> see [Why one palette and not two](#why-one-palette-and-not-two).

[ota]: https://github.com/hota911/herdr-command-palette
[jt]: https://github.com/JanTvrdik/herdr-command-palette

![Command palette demo](docs/assets/demo.gif)

## Usage

Press `prefix+p`. The palette opens as a popup over your current pane: what you type stays on
the top line, the fuzzy-filtered results follow underneath with the matched characters
highlighted, listing every built-in command *and* every action of every other installed
plugin. Fuzzy-search or arrow-select the one you want and hit enter.

- Some commands ask for more input before running:
  - **Free input** (e.g. renaming a tab, a workspace directory) — type a value and press
    enter. Leaving a required field empty and confirming cancels the command; Esc cancels at
    any time.
  - **Selection from a list** (e.g. which workspace, tab, or agent to switch to) — fuzzy-search
    or arrow-select an entry from the list, same as the main palette. Esc cancels.
  - **Confirmation** — destructive commands (closing a workspace, tab, or pane) ask Yes/No
    first, defaulting to No. Choosing No, pressing Esc, or pressing Enter without picking
    anything all cancel without running the command.
- Esc at the main picker closes the palette without doing anything.

## Available commands

- **Workspace:** Switch…, Next, Previous, New…, Rename current, Close current
- **Tab:** Switch…, Next, Previous, New, Rename current, Close current
- **Pane:** Rename current, Close current, Toggle zoom, Focus left/right/up/down, Split
  right/down, Swap left/right/up/down, Resize left/right/up/down
- **Agent:** Focus…
- **Config:** Reload

All commands act on the pane, tab, or workspace that opened the palette, except commands that
target something else by design (e.g. `Workspace: Switch…`, `Agent: Focus…`).

### Plugin actions

Below the built-in commands, the picker lists every action of every *other* installed plugin,
one row each, shown as `Plugin: <title>  <plugin_id>.<action_id>`. The qualified id sits in the
displayed field on purpose: the picker matches on what it displays, so typing a plugin's name
finds its actions. Picking one dispatches it through `herdr plugin action invoke` and waits for the
run to finish, so an action that fails shows its error instead of vanishing with the popup.

Two rows are never offered: this palette's own actions, and actions declared for a platform
other than the one you are on (several plugins ship a Windows twin of each action; on Linux and
macOS those can only fail).

If `herdr plugin action list` fails, or no other plugin is installed, the built-in half still
works — the plugin half is additive, never a prerequisite.

### Why one palette and not two

The two palettes this one draws on are designed to sit side by side on two keys, and say so. They
have to: jt.command-palette runs its picker in an **overlay** pane, and an overlay is a real pane
in the workspace's layout tree, so while it is up it *is* the focused pane. Any built-in
operation dispatched from there would resolve its target — pane to close, tab to rename — to the
palette itself rather than to the pane you were working in. jt's own open action documents this and
works around the cwd half of it by forwarding `--cwd`.

This palette does not have the problem, because it uses **popup** placement. A popup lives
outside the layout tree (`state.popup_pane` in herdr), so it never becomes the focused pane:
`pane.current` and the plugin invocation context both resolve through the workspace's
`focused_pane_id`, which still points at your working pane. That is what makes one key enough —
built-in commands and plugin actions both act on where you actually were.

On top of that, the `open` action captures the origin pane, tab, workspace and cwd
server-side, before the popup opens, and forwards them as `ORIGIN_*`.

### Not covered

This palette targets what herdr's public CLI can do. See the design document's
[built-in keybinding coverage table](docs/design/command-catalog.md#mapping-to-built-in-keybindings)
for which built-in keybindings have no equivalent here, and why.

## Requirements

- herdr 0.8.0 or later
- a [Rust toolchain](https://rustup.rs) to build the palette binary (build-time only; the
  built plugin has no runtime dependency beyond herdr itself)

## Installation

This plugin is not published; build the binary, then link a working copy:

```bash
cd /path/to/herdr-palette
cargo build --release
herdr plugin link .
```

The plugin manifest points at `target/release/herdr-palette`; rebuild after pulling changes.

## Keybinding

Add the following to `~/.config/herdr/config.toml`, then `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "vjeantet.palette.open"
description = "Command palette"
```

Prefer an editor-style single chord (no prefix)? Drop the `prefix+` part: herdr treats the
chord as a direct binding, intercepted before it reaches the pane. `key = "ctrl+shift+p"`
works if your terminal speaks the kitty keyboard protocol (herdr requests it on startup);
on terminals that don't, that chord is indistinguishable from `ctrl+p` and the binding never
fires — use `key = "ctrl+alt+p"` instead, the one modifier family that survives every
terminal.

One key is the whole point; if you also run either of the palettes credited above, give it a
different key or uninstall it, since all three expose an action called `open`.

## How it works

- Both halves live in one Rust binary: **action `open`** (`herdr-palette open`) runs on the
  herdr server side (no TTY) and opens the palette as a popup pane, which does have a TTY and
  runs the picker (`herdr-palette ui`). The popup is session-modal and sized independently of your
  tiled layout, and doesn't show up in your pane list.
- If your herdr version is below the plugin's minimum, herdr won't load the plugin at all. If
  the herdr you're running has drifted from the protocol version this catalog was built
  against, or if that protocol can't be read at all, the palette header shows a warning — it
  still works, the warning just flags possible staleness. If a command you pick fails, the
  palette shows the error (the group, subcommand, and herdr's own output) and waits for a
  keypress instead of silently closing.

## Related plugin

JanTvrdík's [jt.command-palette](https://github.com/JanTvrdik/herdr-command-palette) is a
palette for plugin actions, not herdr's own built-in operations (tab close, pane split, and so
on). This palette lists those plugin actions itself, so running both is redundant — one key is
the whole point.

This plugin's popup plumbing and error-visibility pattern are adapted from
jt.command-palette (MIT); the source is credited in [LICENSE](LICENSE) and in source comments.
