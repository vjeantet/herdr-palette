#!/usr/bin/env bash
# Action `vjeantet.palette.open`: opens the built-in command palette
# popup. Runs server-side (no TTY) so fzf can't run here. Before the
# popup steals focus, capture the still-focused origin pane/tab/workspace
# and forward them via --env.
# The popup-launch pattern follows Jan Tvrdík's jt.command-palette (MIT,
# https://github.com/JanTvrdik/herdr-command-palette).
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"

if ! command -v jq >/dev/null 2>&1; then
  echo "command-palette: jq is not installed" >&2
  exit 1
fi

cur="$("$herdr_bin" pane current 2>&1)" || { echo "command-palette: pane current failed: $cur" >&2; exit 1; }
pane_id="$(printf '%s' "$cur" | jq -r '.result.pane.pane_id // empty')"
tab_id="$(printf '%s' "$cur" | jq -r '.result.pane.tab_id // empty')"
workspace_id="$(printf '%s' "$cur" | jq -r '.result.pane.workspace_id // empty')"
cwd="$(printf '%s' "$cur" | jq -r '.result.pane.foreground_cwd // .result.pane.cwd // empty')"

if [ -z "$pane_id" ] || [ -z "$tab_id" ] || [ -z "$workspace_id" ]; then
  echo "command-palette: could not resolve origin context from: $cur" >&2
  exit 1
fi

# Placement is not passed here; the manifest's [[panes]] entry decides it.
set -- plugin pane open \
  --plugin vjeantet.palette \
  --entrypoint palette \
  --focus \
  --env "ORIGIN_PANE_ID=$pane_id" \
  --env "ORIGIN_TAB_ID=$tab_id" \
  --env "ORIGIN_WORKSPACE_ID=$workspace_id" \
  --env "ORIGIN_CWD=$cwd"

if [ -n "$cwd" ] && [ -d "$cwd" ]; then
  set -- "$@" --cwd "$cwd"
fi

exec "$herdr_bin" "$@"
