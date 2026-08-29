#!/usr/bin/env bats
# The plugin-action half of the palette: rows sourced from
# `herdr plugin action list` alongside the built-in catalog, and their
# dispatch through `herdr plugin action invoke`.
#
# Same boundaries as palette.bats: herdr is stubbed at its process boundary,
# the picker is driven headlessly through the PALETTE_STUB_* variables.
#
# Selecting a plugin row hands the action to a detached `herdr-palette
# dispatch` process the palette does not wait for (herdr allows one popup at
# a time, and the palette is one). UI-level tests therefore wait for the
# dispatch to leave its trace; the dispatch itself is also driven directly.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  TEST_PLUGIN_ROOT="$BATS_TEST_TMPDIR/plugin"
  mkdir -p "$TEST_PLUGIN_ROOT"

  export HERDR_PLUGIN_ROOT="$TEST_PLUGIN_ROOT"
  export HERDR_BIN_PATH="$ROOT/tests/stubs/herdr"
  export HERDR_STUB_CALLS="$BATS_TEST_TMPDIR/herdr-calls"
  export PALETTE_STUB_DUMP="$BATS_TEST_TMPDIR/picker-dump"
  export PALETTE_STUB_IPC_DUMP="$BATS_TEST_TMPDIR/ipc-dump"
  export ORIGIN_PANE_ID="w1:p1"
  export ORIGIN_TAB_ID="w1:t2"
  export ORIGIN_WORKSPACE_ID="w1"
  export ORIGIN_CWD="$BATS_TEST_TMPDIR"
  export PALETTE_STUB=1
  # Hermetic keybinding hints: never read the developer's real herdr config.
  export HERDR_CONFIG_PATH="$BATS_TEST_TMPDIR/herdr-config.toml"

  write_catalog
  export HERDR_STUB_ACTION_LIST_JSON='{"result":{"actions":[
    {"plugin_id":"herdr-scratchpad","action_id":"open-scratchpad","title":"Toggle scratchpad","platforms":["linux","macos"]},
    {"plugin_id":"herdr-file-viewer","action_id":"open-file-viewer-windows","title":"Open file viewer","platforms":["windows"]},
    {"plugin_id":"some.plugin","action_id":"bare","title":"No platforms declared"}
  ]}}'
}

# write_catalog — one built-in command whose effect is observable: the herdr
# stub records `workspace focus` calls, so selecting it leaves a trace.
write_catalog() {
  cat >"$TEST_PLUGIN_ROOT/commands.json" <<'JSON'
{
  "schema_version": 1,
  "expected_herdr_protocol": 20,
  "commands": [{
    "id": "builtin.test",
    "title": "Builtin: Test",
    "description": "A built-in command with one literal argument.",
    "command": ["workspace", "focus"],
    "arguments": [{"source": "literal", "value": "w9"}]
  }]
}
JSON
}

# picker_lines — what the main picker was offered. The dump file also
# receives later pickers (select, confirm); this catalog triggers none.
picker_lines() {
  cat "$PALETTE_STUB_DUMP"
}

# wait_for_call PATTERN — the detached dispatch outlives the palette; give it
# up to five seconds to record a matching herdr call.
wait_for_call() {
  local attempt
  for attempt in $(seq 1 100); do
    if grep -q -- "$1" "$HERDR_STUB_CALLS" 2>/dev/null; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

popup_close_requested() {
  grep -q '"method":"popup.close"' "$PALETTE_STUB_IPC_DUMP"
}

@test "plugin actions are offered alongside the built-in catalog" {
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
  [[ "$(picker_lines)" == *"plugin:herdr-scratchpad.open-scratchpad"* ]]
}

@test "a plugin row displays the derived plugin name and the action title" {
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  # Field 2 is what the picker displays and matches on.
  [[ "$(picker_lines | grep '^plugin:herdr-scratchpad' | cut -f2)" == "Scratchpad: Toggle scratchpad" ]]
}

@test "selecting a plugin row invokes the action from a detached dispatch" {
  export PALETTE_STUB_SELECT_ID="plugin:herdr-scratchpad.open-scratchpad"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  wait_for_call "^plugin action invoke herdr-scratchpad.open-scratchpad$"
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "plugin action invoke herdr-scratchpad.open-scratchpad" ]
}

@test "the palette closes its own popup before the action runs" {
  export PALETTE_STUB_SELECT_ID="plugin:herdr-scratchpad.open-scratchpad"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  wait_for_call "^plugin action invoke"
  popup_close_requested
}

@test "selecting a plugin row runs no catalog command" {
  export PALETTE_STUB_SELECT_ID="plugin:herdr-scratchpad.open-scratchpad"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  wait_for_call "^plugin action invoke"
  ! grep -q "^workspace focus" "$HERDR_STUB_CALLS"
}

@test "the palette exits clean before a dispatched action fails" {
  export PALETTE_STUB_SELECT_ID="plugin:herdr-scratchpad.open-scratchpad"
  export HERDR_STUB_INVOKE_STATUS=1
  export HERDR_STUB_INVOKE_ERROR="plugin_action_not_found"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ -z "$output" ]
  wait_for_call "^notification show "
}

@test "an action declared for another platform is not offered" {
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" != *"open-file-viewer-windows"* ]]
}

@test "an action that declares no platforms is offered" {
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"plugin:some.plugin.bare"* ]]
}

@test "the palette does not offer its own actions" {
  export HERDR_PLUGIN_ID="vjeantet.palette"
  export HERDR_STUB_ACTION_LIST_JSON='{"result":{"actions":[
    {"plugin_id":"vjeantet.palette","action_id":"open","title":"Command palette","platforms":["linux","macos"]},
    {"plugin_id":"herdr-scratchpad","action_id":"open-scratchpad","title":"Toggle scratchpad","platforms":["linux","macos"]}
  ]}}'
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" != *"vjeantet.palette"* ]]
  [[ "$(picker_lines)" == *"plugin:herdr-scratchpad.open-scratchpad"* ]]
}

@test "the built-in half survives a plugin action list that fails" {
  export HERDR_STUB_ACTION_LIST_STATUS=1
  export PALETTE_STUB_SELECT_ID="builtin.test"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w9" ]
}

@test "dispatch invokes the action after closing the popup" {
  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "plugin action invoke herdr-scratchpad.open-scratchpad" ]
  popup_close_requested
}

@test "dispatch still invokes the action when no popup is open" {
  export PALETTE_STUB_IPC_ERROR="no popup is open"

  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "plugin action invoke herdr-scratchpad.open-scratchpad" ]
}

@test "dispatch prints nothing" {
  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "a dispatched action that fails afterwards is reported as a notification" {
  export HERDR_STUB_INVOKE_JSON='{"result":{"log":{"log_id":"plugin-log-7","plugin_id":"herdr-scratchpad","status":"running"}}}'
  export HERDR_STUB_PLUGIN_LOG_JSON='{"result":{"logs":[{"log_id":"plugin-log-7","plugin_id":"herdr-scratchpad","status":"failed","exit_code":127,"stderr":"open-scratchpad.sh: not found"}]}}'

  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 1 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "notification show command-palette: herdr-scratchpad.open-scratchpad failed (exit 127) --body open-scratchpad.sh: not found" ]
}

@test "an invoke the server refuses is reported as a notification" {
  export HERDR_STUB_INVOKE_STATUS=1
  export HERDR_STUB_INVOKE_ERROR="plugin_action_not_found"

  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 1 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "notification show command-palette: failed to invoke herdr-scratchpad.open-scratchpad --body plugin_action_not_found" ]
}

@test "a successful action raises no notification" {
  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 0 ]
  ! grep -q "^notification show" "$HERDR_STUB_CALLS"
}

@test "a herdr too old to report a log is not treated as a failure" {
  export HERDR_STUB_INVOKE_JSON='{"result":{}}'

  run "$(palette_bin)" dispatch herdr-scratchpad.open-scratchpad

  [ "$status" -eq 0 ]
  ! grep -q "^plugin log list" "$HERDR_STUB_CALLS"
  ! grep -q "^notification show" "$HERDR_STUB_CALLS"
}

@test "dispatch without an action id is a usage error" {
  run "$(palette_bin)" dispatch

  [ "$status" -eq 2 ]
  [ ! -e "$HERDR_STUB_CALLS" ]
}
