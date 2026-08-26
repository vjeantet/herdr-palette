# Herdr Command Palette JSON Catalog Design

> **Implementation note (2026-08-26).** The palette was rewritten from bash+fzf+jq into a
> single Rust binary (`herdr-palette`, subcommands `open` and `ui`) with a Sublime-Text-style
> picker. Every contract this document defines — the catalog format, argument sources,
> resolution order, defensive behaviors, silent-cancel paths — is preserved by the rewrite.
> Where the prose below describes fzf mechanics (exit statuses, `--with-nth`, query behavior),
> read it as the behavioral specification the binary reproduces, not as the current mechanism.

## Background

This document designs a command palette that runs herdr's built-in operations from a popup
picker.

The original plan wrote the command list and argument-resolution logic directly into a `case`
statement in `palette.sh`. Under that design, changing a display name or an argument would
require editing Bash, and the design could not separate command definitions from execution
logic.

This design instead declares the command list in a `commands.json` bundled with the plugin,
and the palette reads the definitions and executes them. Schema validation of the
definitions happens at development time and in CI, never at runtime. There is no feature for
users to define arbitrary shell commands or jq filters. The JSON catalog is under Git and
code review, and it is never merged with user settings.

This design follows what was agreed in a design discussion on 2026-08-16: move the command
list into JSON; represent arguments as free input, selection from herdr's list results, or a
reference to the launch-origin context; and split Pane Swap into per-direction commands. Swap
is split by direction so that, as in VS Code's Command Palette, each operation can be searched
and run directly without opening a second-level direction picker.

## Goals

This design achieves the following:

- Manage the display name, description, herdr subcommand, and argument sources in
  `commands.json`.
- Build the herdr CLI's argv by combining free input, selection from list results, and
  references to the launch-origin context.
- Never evaluate arbitrary shell; pass each JSON value to the herdr CLI as exactly one argv
  element.
- Expose herdr's existing built-in keybindings as searchable commands wherever the public CLI
  can achieve the same effect safely.
- Show invalid definitions and runtime errors to the user; never hide a failure.

## Out of scope

The initial version does not implement:

- Loading a user-supplied JSON catalog, merging it with the bundled one, or overriding
  commands
- Running arbitrary shell commands, shell strings, or jq filters
- Environment-variable expansion, command substitution, or `eval` from the JSON definitions
- Creating, opening, or removing worktrees
- UI-internal operations that have no equivalent in herdr's public CLI
- Saving and reordering recently run commands

Worktree operations are excluded from the initial version because they create or delete
working directories and need different confirmation handling than ordinary Tab, Workspace, and
Pane operations. Requirements for recently run commands are pinned in "Future work".

## File layout

This design's implementation uses the following files:

- `commands.json`: defines commands, display information, argument sources, confirmation
  text, and the expected herdr API protocol.
- `commands.schema.json`: declares the catalog's validation conditions as JSON Schema.
- `src/` (the `herdr-palette` binary): the `ui` subcommand handles the picker, argument
  resolution, and running the herdr CLI; the `open` subcommand captures the origin Pane,
  Tab, Workspace, and working directory before the popup takes focus, and passes them
  through environment variables.
- `scripts/check-compat.sh`: validates the catalog schema, compares the API protocol, and
  checks that the herdr subcommands referenced in the catalog are available.
- `README.md`: documents, for users, the available commands, requirements, installation, and
  what is out of scope.

`commands.json` lives at the plugin root. The palette reads
`$HERDR_PLUGIN_ROOT/commands.json`. There is no fallback that searches for a different file
when the environment variable is absent.

## Top-level catalog structure

The top level of `commands.json` has this shape:

```json
{
  "schema_version": 1,
  "expected_herdr_protocol": 20,
  "commands": []
}
```

**`schema_version`**: the version of the JSON catalog format this plugin defines. The initial
value is `1`; the palette and the compatibility check accept only `1`.

**`expected_herdr_protocol`**: the herdr socket API protocol this catalog assumes. The value
confirmed on herdr 0.8.0 is `19`. The palette and the compatibility check both refer to this
single value.

**`commands`**: an array of command definitions. The array order is fzf's initial display
order.

## Command definition

Each command has the following fields:

```json
{
  "id": "tab.rename",
  "title": "Tab: Rename current tab",
  "description": "Enter a new label for the tab that opened the palette.",
  "command": ["tab", "rename"],
  "arguments": [],
  "confirm": "Rename the current tab?"
}
```

**`id`**: a stable ID, unique within the catalog. Only lowercase alphanumerics, periods,
hyphens, and underscores are allowed. The ID never changes when the display name changes.

**`title`**: the name shown in the main fzf list. Write it with the target first, e.g.
`Tab: Rename current tab`.

**`description`**: the explanation shown in the header on the input, selection, and
confirmation screens. It states clearly whether the target is the launch origin or something
the user selects.

**`command`**: a two-element array of group and subcommand, placed directly after `herdr`.
Each element allows only lowercase alphanumerics and hyphens. Options and values go in
`arguments`.

**`arguments`**: an array of argument definitions, resolved left to right at execution time.
Each element is one of `literal`, `context`, `input`, or `select`, described below.

**`confirm`**: optional confirmation text. When present, after all arguments are resolved and
just before running the herdr CLI, the user is asked to choose Yes or No in fzf. The picker
defaults to No — fzf's cursor starts on the first candidate, and No is listed first, so
pressing Enter without an explicit selection cancels rather than confirms. Both No and Esc are
treated as a normal cancellation.

Every command runs the herdr CLI synchronously; there is no field for deferring a command's
execution past when the palette pane closes. See "Research evidence" for why this is safe
under the plugin's current popup placement.

## Argument types

### Literal value

`literal` adds the string written in the catalog as a single argv element.

```json
{
  "source": "literal",
  "value": "--focus"
}
```

`literal` is not a shell string. A value containing whitespace is still treated as a single
argv element and is never reinterpreted by a shell.

### Origin context

`context` refers to a value that the `open` action captured before the popup opened.

```json
{
  "source": "context",
  "key": "tab_id"
}
```

The four static keys are:

- `pane_id`: `ORIGIN_PANE_ID`
- `tab_id`: `ORIGIN_TAB_ID`
- `workspace_id`: `ORIGIN_WORKSPACE_ID`
- `cwd`: `ORIGIN_CWD`

If any of the Pane, Tab, or Workspace IDs is empty, this is an error before the catalog is
displayed. For commands that use `cwd`, argument resolution checks that the value is
non-empty and is an existing directory.

Command arguments may also use four computed context keys:

- `next_workspace_id`
- `previous_workspace_id`
- `next_tab_id`
- `previous_tab_id`

Workspace keys resolve against the array order from `herdr workspace list`. Tab keys resolve
against `herdr tab list --workspace "$ORIGIN_WORKSPACE_ID"`, so navigation stays inside the
workspace that opened the palette. The resolver locates the corresponding origin ID, selects
the adjacent entry, and wraps at both ends. A one-entry list resolves to the same ID.

Computed context keys are valid only as command arguments. `input.default_context` and
`select.exclude_context` accept only the four static keys, because resolving a default or an
exclusion must not trigger a navigation lookup.

`herdr pane current` must never be re-run after the popup opens, because doing so would
refer to the palette pane instead of the launch origin.

### Free input

`input` shows a description and prompt, and adds the string the user types as a single argv
element.

```json
{
  "source": "input",
  "prompt": "New tab label ▸ ",
  "description": "Enter a new label for the current tab.",
  "required": true
}
```

`input` has the following fields:

- `prompt`: the prompt for the input field.
- `description`: an optional description. When absent, the command's `description` is used.
- `required`: a boolean indicating whether an empty string is an allowed value.
- `default_context`: an optional initial value. The allowed values are the four static
  context keys.
- `validation`: an optional validation method. The only value the initial version allows is
  `directory`.

Free input uses fzf's query input. Enter confirms the query, and Esc is a normal
cancellation. When `default_context` is present, its value becomes the initial query. If
`required` is true and the confirmed value is empty, the herdr CLI is not run and the palette
exits successfully. If `validation` is `directory` and the value is not an existing directory,
an error is shown and the palette exits.

### Selection from list results

`select` runs a named selector and lets the user pick one entry from herdr's list results in
fzf.

```json
{
  "source": "select",
  "selector": "workspaces",
  "prompt": "workspace ▸ ",
  "exclude_context": "workspace_id"
}
```

`select` has the following fields:

- `selector`: the named selector to use.
- `prompt`: the prompt for the selection screen.
- `description`: an optional description. When absent, the command's `description` is used.
- `exclude_context`: an optional exclusion condition. Candidates whose selector value matches
  the given context are removed.

The initial three selectors are:

| selector | source command | value passed to argv | display name |
|---|---|---|---|
| `workspaces` | `herdr workspace list` | `workspace_id` | `label (workspace_id)` |
| `tabs` | `herdr tab list` | `tab_id` | `workspace_label / label (tab_id)` |
| `agents` | `herdr agent list` | `pane_id` | `workspace_label / agent: terminal_title_stripped (pane_id)` |

`tabs` and `agents` display candidates across all workspaces, so their display name is
prefixed with the workspace's label. Because `herdr tab list` and `herdr agent list` return
only `workspace_id`, the selector makes one extra call to `herdr workspace list` to resolve a
label from the workspace_id. A candidate whose workspace label cannot be resolved falls back
to displaying the raw `workspace_id`. A failure of this workspace list call is treated as an
error, the same as a failure of any other list command.

A selector's name and implementation are matched in the palette source (`src/resolve.rs`). The
JSON never contains a list command or a filter. This keeps editing the catalog alone from
becoming a channel for launching an arbitrary command.

New selector kinds are added only when a new kind of list result is needed. Selectors not
referenced by the initial catalog are not implemented.

## JSON examples

Renaming a tab combines `context` and `input`.

```json
{
  "id": "tab.rename",
  "title": "Tab: Rename current tab",
  "description": "Enter a new label for the tab that opened the palette.",
  "command": ["tab", "rename"],
  "arguments": [
    { "source": "context", "key": "tab_id" },
    {
      "source": "input",
      "prompt": "new tab label ▸ ",
      "description": "Enter a new label for the current tab.",
      "required": true
    }
  ]
}
```

Switching workspaces uses `select` and excludes the current workspace.

```json
{
  "id": "workspace.switch",
  "title": "Workspace: Switch…",
  "description": "Focus another workspace.",
  "command": ["workspace", "focus"],
  "arguments": [
    {
      "source": "select",
      "selector": "workspaces",
      "prompt": "workspace ▸ ",
      "exclude_context": "workspace_id"
    }
  ]
}
```

Creating a workspace takes the working directory as free input.

```json
{
  "id": "workspace.new",
  "title": "Workspace: New…",
  "description": "Create and focus a workspace in the selected directory.",
  "command": ["workspace", "create"],
  "arguments": [
    { "source": "literal", "value": "--cwd" },
    {
      "source": "input",
      "prompt": "workspace directory ▸ ",
      "description": "Enter an existing directory for the new workspace.",
      "required": true,
      "default_context": "cwd",
      "validation": "directory"
    },
    { "source": "literal", "value": "--focus" }
  ]
}
```

Closing the current tab attaches a confirmation.

```json
{
  "id": "tab.close",
  "title": "Tab: Close current tab",
  "description": "Close the tab that opened the palette.",
  "command": ["tab", "close"],
  "arguments": [
    { "source": "context", "key": "tab_id" }
  ],
  "confirm": "Close the current tab?"
}
```

Pane Swap defines one command per direction.

```json
{
  "id": "pane.swap.left",
  "title": "Pane: Swap left",
  "description": "Swap the current pane with its left neighbor.",
  "command": ["pane", "swap"],
  "arguments": [
    { "source": "literal", "value": "--pane" },
    { "source": "context", "key": "pane_id" },
    { "source": "literal", "value": "--direction" },
    { "source": "literal", "value": "left" }
  ]
}
```

## Initial command list

The initial version includes the following operations, all safely achievable via the public
CLI.

### Workspace

- `Workspace: Switch…`
- `Workspace: Next`
- `Workspace: Previous`
- `Workspace: New…`
- `Workspace: Rename current`
- `Workspace: Close current`

### Tab

- `Tab: Switch…`
- `Tab: Next`
- `Tab: Previous`
- `Tab: New`
- `Tab: Rename current`
- `Tab: Close current`

### Pane

- `Pane: Rename current`
- `Pane: Close current`
- `Pane: Toggle zoom`
- `Pane: Focus left`
- `Pane: Focus right`
- `Pane: Focus up`
- `Pane: Focus down`
- `Pane: Split right`
- `Pane: Split down`
- `Pane: Swap left`
- `Pane: Swap right`
- `Pane: Swap up`
- `Pane: Swap down`
- `Pane: Resize left`
- `Pane: Resize right`
- `Pane: Resize up`
- `Pane: Resize down`

Resize uses the herdr CLI's default amount; the initial version does not let the user enter an
amount.

### Agent

- `Agent: Focus…`

The Agent selector passes the `pane_id` from `herdr agent list` to `herdr agent focus`.

### Config

- `Config: Reload`

## Mapping to built-in keybindings

The scope of coverage is the built-in keybindings listed by `herdr --default-config`.
Operations for which the public CLI can achieve the same goal are made available from the
catalog even where there isn't a strict one-to-one match to a keybinding name.

| built-in keybinding | initial-version mapping |
|---|---|
| `reload_config` | `Config: Reload` |
| `workspace_picker`, `switch_workspace` | `Workspace: Switch…` |
| `new_workspace` | `Workspace: New…` |
| `rename_workspace` | `Workspace: Rename current` |
| `close_workspace` | `Workspace: Close current` |
| `new_tab` | `Tab: New` |
| `rename_tab` | `Tab: Rename current` |
| `switch_tab` | `Tab: Switch…` |
| `close_tab` | `Tab: Close current` |
| `rename_pane` | `Pane: Rename current` |
| `focus_pane_left`, `focus_pane_right`, `focus_pane_up`, `focus_pane_down` | per-direction `Pane: Focus` |
| `split_vertical`, `split_horizontal` | `Pane: Split right` and `Pane: Split down` |
| `close_pane` | `Pane: Close current` |
| `zoom` | `Pane: Toggle zoom` |
| `resize_mode` | per-direction `Pane: Resize` |
| `focus_agent` | `Agent: Focus…` |
| `previous_workspace`, `next_workspace` | `Workspace: Previous`, `Workspace: Next` |
| `previous_tab`, `next_tab` | `Tab: Previous`, `Tab: Next` |
| `previous_agent`, `next_agent` | served by `Agent: Focus…` |

The initial version does not cover the following operations.

| category | built-in keybinding | reason not covered |
|---|---|---|
| UI-internal operations | `help`, `settings`, `detach`, `open_notification_target`, `goto`, `edit_scrollback`, `toggle_sidebar` | No matching request in herdr 0.8.0's public CLI or socket API schema |
| Temporary UI modes | `resize_mode` itself, navigate-mode operations | Direct per-direction operations are offered instead of opening a mode |
| Pane cycling | `cycle_pane_next`, `cycle_pane_previous` | Protocol 19 exposes raw `pane.focus`, but the public CLI has no arbitrary-ID focus wrapper; support needs a separate socket execution path |
| Focus history | `last_pane` | The public CLI does not expose focus history |
| Worktree | `new_worktree`, `open_worktree`, `remove_worktree` | Filesystem side effects and extra confirmation are handled in a separate design |
| Remote | `remote_image_paste` | Remote-client-specific input handling, not a built-in operation of the socket CLI |

## Execution flow

The palette (`herdr-palette ui`) processes in the following order:

1. Confirm `fzf` and `jq` are present.
2. Confirm `ORIGIN_PANE_ID`, `ORIGIN_TAB_ID`, and `ORIGIN_WORKSPACE_ID` are present.
3. Read `$HERDR_PLUGIN_ROOT/commands.json`. If it cannot be read as JSON, this is an error.
   Schema validation is not performed at runtime, except that the palette also confirms
   `schema_version` is `1` and dies otherwise.
4. Read the protocol from `herdr api schema` and compare it against
   `expected_herdr_protocol`.
5. If the protocol could not be read at all, show a "could not read protocol" warning in fzf's
   header; if it was read but differs from `expected_herdr_protocol`, show a mismatch warning
   instead. Neither case blocks execution.
6. Pass `id` and `title` to the main fzf. A non-cancel, non-zero `fzf` exit status (i.e. 2, not
   1 or 130) is an error, not a cancellation — see "Errors and cancellation".
7. Resolve the selected command's `arguments` left to right, appending one element at a time
   to a Bash array. For a `select` argument, a jq transform failure while building the
   candidate list is an error, distinct from the transform succeeding with zero candidates
   (a normal cancellation); candidate labels are sanitized before display (see "Errors and
   cancellation").
8. If `confirm` is present, let the user choose Yes or No.
9. Run `"$herdr_bin" "${argv[@]}"`.
10. On success, exit and close the popup.

Bash is restricted to syntax that also works on the Bash 3.2 shipped by default on macOS.
It does not depend on `mapfile`, associative arrays, or other Bash-4-only features.

## Catalog validation

Validation conditions are declared in `commands.schema.json` (JSON Schema draft 2020-12), and
`scripts/check-compat.sh` validates against it with
`uvx check-jsonschema --schemafile commands.schema.json commands.json`. Validation happens
only at development time and in CI; the palette never performs schema validation at runtime.

The reasons runtime validation is skipped are as follows. `commands.json` is bundled with the
plugin, under Git, and subject to code review; there is no path for a user to edit it. The
only path by which a broken catalog could reach a user is a release, and CI validation
prevents that. The palette shows an error only when the file cannot be read as JSON, or when
argument resolution encounters an unexpected value.

Uniqueness of `id` cannot be expressed in JSON Schema, so it is checked with jq inside
`scripts/check-compat.sh`.

The validation conditions are:

- The top level is an object.
- `schema_version` is the integer `1`.
- `expected_herdr_protocol` is a positive integer.
- `commands` is a non-empty array.
- Every `id` matches the required format and is unique.
- `title` and `description` are non-empty strings.
- `command` is a non-empty two-element string array matching the group-and-subcommand format.
- `arguments` is an array.
- Each argument's `source` is one of `literal`, `context`, `input`, or `select`.
- Each source has its required fields present and no disallowed fields.
- `context.key` is a static or computed argument context key.
- `input.default_context` is a static context key.
- `input.required` is a boolean.
- `input.validation` is unspecified or `directory`.
- `select.selector` is an allowed selector.
- `select.exclude_context` is unspecified or a static context key.
- `confirm` is unspecified or a non-empty string.
- Definition strings contain no NUL (which cannot pass through a shell), no newline including a
  trailing one, and no tab (all of which would break the tab-delimited fzf candidate display).

When validation fails, check-jsonschema prints the JSON path of the invalid location. No
custom error message is written; the JSON Schema validator's own output is relied on.

## Errors and cancellation

Errors are shown inside the palette pane, and it waits for a keypress before exiting. This
error-display pattern — print the message with `die`, then wait for a keypress before exiting
— is adapted from Jan Tvrdík's jt.command-palette, so its MIT attribution is kept in
implementation comments. The source is https://github.com/JanTvrdik/herdr-command-palette.

The following states are treated as errors:

- `fzf` or `jq` is missing.
- A required piece of launch-origin context is missing.
- `commands.json` is missing, unreadable, or invalid JSON, or its `schema_version` is not `1`.
- A `herdr ... list` call fails.
- A list result does not match the expected shape, a candidate has a missing or invalid ID, or
  the jq transform that builds candidates from it fails outright (as opposed to producing zero
  candidates, which is a normal cancellation — see below).
- Input validation fails.
- Running the herdr CLI fails.
- `fzf` itself exits with status 2 (an fzf usage or runtime error) at the main picker, a select,
  or a confirmation screen. This is distinct from a cancellation (see below) even though both
  are a nonzero exit status.

The following states are a normal cancellation and exit with status 0:

- Esc is pressed at the main picker, or `fzf` there reports no match (exit status 1).
- Esc is pressed during input or select, or (for select) `fzf` reports no match.
- A required input is confirmed while still empty.
- A select has no candidates (the list call succeeded and returned a well-shaped, empty list;
  see above for when the transform itself fails instead).
- No or Esc is chosen on the confirmation screen. No is the default selection, so pressing
  Enter without picking anything also cancels.
- `herdr api schema` doesn't report a protocol at all, or reports one that differs from
  `expected_herdr_protocol`: both show a header warning and continue, they never block.

A label from a `herdr ... list` result (e.g. a workspace, tab, or agent label) may contain
control characters such as a newline or tab; the palette strips those (replacing them with a
space) before display so a crafted label can't forge rows or mislead the picker — in the
fzf era they could literally shift which tab-delimited field was treated as the id.

When the herdr CLI fails, the group and subcommand that were run, along with herdr's own
output, are shown. Every command runs synchronously and its failure is always shown this way.
Free-input values, selected IDs, and working directories are never interpolated into an error
message, because input values may contain sensitive information and the group and subcommand
are enough for diagnosis.

## Compatibility checks

`scripts/check-compat.sh` treats `commands.json` as the single source of truth and checks the
following:

1. Confirms the catalog matches the schema.
2. Compares `expected_herdr_protocol` against the protocol from `herdr api schema`.
3. For every distinct `commands[].command`, confirms that `herdr <group> <subcommand> -h`
   actually returns that subcommand's help (a `Usage: herdr <group> <sub>` line). An unknown
   group still falls back to the top-level help with exit code 0, so exit code alone cannot
   decide this.
4. Checks `workspace list`, `tab list`, and `agent list` — used by named selectors or computed
   context — the same way.
5. For every command, confirms its resolved argv supplies positional arguments compatible in
   both count and name with what `herdr <group> <subcommand> -h`'s Usage line requires, and
   supplies every required option the Usage line lists.
   Required positionals appear as bare `<NAME>` tokens (optionally suffixed `...` for
   "one or more"); optional ones as `[NAME]` (a single optional slot, raising the allowed
   count by exactly one) or `[NAME]...` (variadic, unbounded — same as a trailing `...` on a
   required positional); a `--flag <VALUE>` pair outside `[OPTIONS]` is a required option, not
   a positional. commands.json never spells out a positional directly — each is supplied via a
   `context`, `select`, `input`, or non-flag `literal` argv element — so the check walks each
   command's resolved argv, skips known `--flag`/`--flag value` pairs (using that subcommand's
   own Options section to tell which flags take a value), and treats what's left as the
   supplied positionals. Their count must be within what the Usage line allows (exactly the
   required count when there's no `[NAME]`/`...`; up to the required count plus one per
   non-variadic `[NAME]`; unbounded above the required count when `...` appears), and each
   one's name must be compatible with its expected placeholder, using this mapping (verified
   against herdr 0.8.0's help text for all 31 commands, 2026-08-16):

   | argument source | maps to placeholder |
   |---|---|
   | `context.key: workspace_id`, `next_workspace_id`, or `previous_workspace_id` / `select.selector: workspaces` | `workspace_id` |
   | `context.key: tab_id`, `next_tab_id`, or `previous_tab_id` / `select.selector: tabs` | `tab_id` |
   | `context.key: pane_id` | `pane_id` |
   | `select.selector: agents` | `target` (herdr accepts a pane id for `agent focus <target>`; see `herdr --skill`) |
   | `input`, or a non-flag `literal` value | matches any placeholder (free text; count is still enforced) |

   The comparison is case-insensitive because herdr's own help text is inconsistent about it
   (e.g. `workspace close <workspace_id>` vs `workspace rename <WORKSPACE_ID>` name the same
   placeholder). Separately, every required option (`--flag <VALUE>` outside `[OPTIONS]`) must
   appear as one of the command's `literal` arguments; a positional count/name match alone is
   not accepted as standing in for a missing required option.

It also checks that specific flags exist. Rather than duplicating the flag list in the shell
script, it extracts the `--`-prefixed values from each command's `literal` entries and checks
that each one exactly matches a flag the subcommand's help declares (not merely appears as a
substring of the help text — e.g. a catalog `--focus` must not pass just because the help
declares `--focused`).

GitHub Actions installs the latest herdr and runs this weekly. `actions/checkout` is pinned to
a commit SHA, and permissions are limited to `contents: read` and `issues: write`. `herdr`
itself is installed unpinned (checking compatibility with the latest release is the point of
this job); `check-jsonschema` is pinned to the version validated locally. On failure, the
workflow run itself ends red (the check step exits non-zero), and a separate step, run via
`always()` so it still executes despite that failure, either opens a GitHub issue whose body is
the check results or, if an open issue with a matching title already exists, comments on it
instead of opening a duplicate.

## Verification

Static verification runs:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
- `bash -n scripts/check-compat.sh`
- `shellcheck scripts/check-compat.sh`
- `uvx check-jsonschema --schemafile commands.schema.json commands.json`
- `scripts/check-compat.sh`
- `bats tests/`: schema-validation tests (the real catalog and a set of deliberately invalid
  fixtures under `tests/fixtures/schema/`) and `scripts/check-compat.sh` tests run against
  `tests/stubs/herdr`, a fake herdr CLI reproducing real 0.8.0 output shapes, including both of
  its "-h" fallback shapes (an unrecognized group vs. an unrecognized subcommand within a known
  group), so each of `check-compat.sh`'s comparison-based FAIL paths (protocol mismatch,
  duplicate ids, both unknown-subcommand fallback shapes, missing/superstring flags, and
  positional count/name/surplus/required-option checks) can be exercised without a live herdr
  install. Failures from missing preconditions (`jq`, `herdr`, or the catalog/schema files
  themselves being absent) are not covered by the stub.

`.github/workflows/ci.yml` runs all of the above (plus `shellcheck`/`bash -n` on the stub and
`actionlint` on both workflows) on every push and pull request. This is separate from
`.github/workflows/compat-check.yml`, which installs the latest herdr release and checks
real-world compatibility weekly (see "Compatibility checks" above).

Real-machine E2E confirms representative argument types and cancellation. The palette pane is
popup placement, which does not appear in `herdr pane list`; it therefore can't be driven with
`herdr pane send-keys` from the CLI the way an overlay pane could, so these steps are performed
manually:

- Open the palette and close it with Esc at the main picker.
- Run `Tab: New`, confirm a Tab is added to the correct Workspace via `literal` and `context`,
  then undo it.
- Run `Tab: Rename current tab`, confirm a label containing whitespace can be passed via
  `input` with its description, then restore the original label.
- Run `Workspace: Switch…`, confirm `select` excludes the current Workspace from the list
  result, then switch back to the original Workspace.
- Select `Tab: Close current tab` and confirm it does not run when No is chosen.
- Run `Pane: Focus right` and confirm the focus change survives the popup closing (see
  "Research evidence" for why this doesn't need deferred execution under popup placement).
- Temporarily change `expected_herdr_protocol` to a different value, confirm the picker can
  still be operated while the warning is shown, then restore it.
- Load a catalog that cannot be read as JSON from a temporary file, and confirm an error is
  shown before the picker opens.

For any state created or changed during real-machine E2E, save the listing beforehand, restore
it, and confirm the post-verification listing matches.

## Documentation

The README documents, for users:

- How to open the command palette and search it
- The list of available commands
- How to use free input, selection, and confirmation
- The herdr, fzf, and jq requirements
- Installation and a `prefix+shift+p` keybinding example
- A pointer to this design document's "Mapping to built-in keybindings" coverage table (not a
  duplicated table) for built-in keybindings the CLI cannot cover or that are out of scope for
  the initial version
- What the protocol warning and a run failure mean
- The difference from jt.command-palette

The full field-by-field description of the JSON schema is not duplicated in the README; this
design document remains the developer-facing source of truth. Because the initial version
never reads a user-supplied catalog, the README does not present `commands.json` as a
user-facing configuration feature.

## Future work

Add `Pane: Next` and `Pane: Previous` through a separately designed raw-socket execution path,
or after herdr adds a public CLI wrapper for `pane.focus`. The socket version must define pane
ordering and validate the same request/response framing that herdr plugins use.

Save up to 10 recently run commands so they're easier to find in the palette next time.

Only the command ID of a successful run is saved to history. Free-input values, the selected
Pane, Tab, Workspace, or Agent ID, and the working directory are never saved. History is kept
in de-duplicated MRU order, and adding an 11th entry drops the oldest ID. An ID that has been
removed from the catalog is ignored when history is loaded.

Where history is stored, how concurrent updates from multiple herdr sessions are handled, and
how it's reflected in fzf's ordering are decided in a separate design. This requirement is not
implemented and is not currently scheduled.

## Research evidence

herdr 0.8.0's public feature set was confirmed on 2026-08-16 with the following commands:

- `herdr --default-config`
- `herdr --help`
- `herdr api schema`
- `herdr workspace --help`
- `herdr tab --help`
- `herdr pane --help`
- `herdr agent --help`
- `herdr worktree --help`
- `--help` for each relevant subcommand
- `herdr workspace list`
- `herdr tab list`
- `herdr pane list`
- `herdr agent list`
- `herdr --skill`

API protocol `19` is based on the output of `herdr api schema`. The determination that
UI-internal operations have no public request is based on `herdr --help` and the request
schema in `herdr api schema --json` having no equivalent operation.

`herdr --skill` documents that a pane command run without an explicit target may fall back to
the UI-focused pane, which can belong to the user or another client — the behavior the `open`
action relies on by capturing the origin pane/tab/workspace before the popup steals focus,
rather than letting a command resolve its target implicitly.

### Why popup placement is load-bearing

The palette pane's placement was overlay in earlier versions of this plugin, and is popup as
of this design. This isn't cosmetic: under overlay placement (measured on herdr 0.8.0,
2026-08-16), closing the palette pane restored focus to the pane that had focus before it
opened, which silently clobbered the effect of a synchronous focus-only command (e.g.
`Pane: Focus right`), and closing the palette pane also terminated its entire process group,
killing even a `nohup`+`disown`-detached child before it could run. Those two measurements are
why an earlier version of this plugin had a `post_close` field and a `deferred` plugin action
that ran focus commands from a separate, pane-independent server-side action after the
overlay closed — see git history around 2026-08-16 for that mechanism (commit range roughly
`305c88f`..`36cdfbb`).

Under popup placement, a probe command identical to `Pane: Focus right` but run synchronously
(no deferral) was tested live from the popup palette on 2026-08-16, and the focus change
survived the popup closing. The overlay-era clobber does not occur under popup placement, so
the `post_close` field, `deferred.sh`, and the `deferred` plugin action were removed entirely
— every command in `commands.json` now runs synchronously, and a run failure is always shown
in the palette pane (see "Errors and cancellation").

This means popup placement is a load-bearing design choice, not an incidental one: switching
`herdr-plugin.toml`'s `[[panes]]` entry for `palette` back to `placement = "overlay"` would
silently break `Pane: Focus left/right/up/down` and `Agent: Focus…` the same way it did before
`post_close` was introduced, with no deferred-execution fallback left in the code to catch it.
