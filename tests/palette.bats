#!/usr/bin/env bats
# End-to-end argument-resolution tests for palette.sh. fzf and herdr are
# controlled at their process boundaries; the real palette script and jq
# transformations execute unchanged.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  TEST_PLUGIN_ROOT="$BATS_TEST_TMPDIR/plugin"
  mkdir -p "$TEST_PLUGIN_ROOT"

  export HERDR_PLUGIN_ROOT="$TEST_PLUGIN_ROOT"
  export HERDR_BIN_PATH="$ROOT/tests/stubs/herdr"
  export HERDR_STUB_CALLS="$BATS_TEST_TMPDIR/herdr-calls"
  export ORIGIN_PANE_ID="w1:p1"
  export ORIGIN_TAB_ID="w1:t2"
  export ORIGIN_WORKSPACE_ID="w1"
  export ORIGIN_CWD="$BATS_TEST_TMPDIR"
  export PATH="$ROOT/tests/stubs:$PATH"
}

write_navigation_catalog() {
  local group="$1"
  local key="$2"
  jq -n --arg group "$group" --arg key "$key" '
    {
      schema_version: 1,
      expected_herdr_protocol: 20,
      commands: [{
        id: "navigation.test",
        title: "Navigation test",
        description: "Resolve one computed context key.",
        command: [$group, "focus"],
        arguments: [{source: "context", key: $key}]
      }]
    }
  ' >"$TEST_PLUGIN_ROOT/commands.json"
}

run_navigation() {
  write_navigation_catalog "$1" "$2"
  run bash "$ROOT/palette.sh"
}

@test "next workspace focuses the following workspace in list order" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"},{"workspace_id":"w3","label":"three"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
}

@test "previous workspace focuses the preceding workspace in list order" {
  export ORIGIN_WORKSPACE_ID="w2"
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"},{"workspace_id":"w3","label":"three"}]}}'

  run_navigation workspace previous_workspace_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w1" ]
}

@test "next workspace wraps from the last workspace to the first" {
  export ORIGIN_WORKSPACE_ID="w3"
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"},{"workspace_id":"w3","label":"three"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w1" ]
}

@test "previous workspace wraps from the first workspace to the last" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"},{"workspace_id":"w3","label":"three"}]}}'

  run_navigation workspace previous_workspace_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w3" ]
}

@test "a single workspace resolves to itself" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w1" ]
}

@test "workspace navigation reports a list command failure" {
  export HERDR_STUB_LIST_STATUS=1
  export HERDR_STUB_LIST_ERROR="socket unavailable"

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list failed:"* ]]
  [[ "$output" == *"socket unavailable"* ]]
}

@test "workspace navigation rejects an unexpected list shape" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":{}}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list returned an unexpected shape"* ]]
}

@test "workspace navigation rejects a candidate without a workspace id" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"label":"missing"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list returned a candidate without a valid workspace_id"* ]]
}

@test "workspace navigation rejects a numeric workspace id" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":7,"label":"numeric"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list returned a candidate without a valid workspace_id"* ]]
}

@test "workspace navigation rejects an object workspace id" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":{"nested":"w2"},"label":"object"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list returned a candidate without a valid workspace_id"* ]]
}

@test "workspace navigation rejects a list that omits the origin workspace" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}'

  run_navigation workspace next_workspace_id

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: herdr workspace list did not include the origin workspace_id"* ]]
}

@test "next tab focuses the following tab within the origin workspace" {
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run_navigation tab next_tab_id

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "tab list --workspace w1" ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t3" ]
}

@test "previous tab focuses the preceding tab within the origin workspace" {
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run_navigation tab previous_tab_id

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "tab list --workspace w1" ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t1" ]
}

@test "next tab wraps from the last tab to the first" {
  export ORIGIN_TAB_ID="w1:t3"
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run_navigation tab next_tab_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t1" ]
}

@test "previous tab wraps from the first tab to the last" {
  export ORIGIN_TAB_ID="w1:t1"
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run_navigation tab previous_tab_id

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t3" ]
}

@test "a single tab resolves to itself" {
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t2","workspace_id":"w1","label":"two"}]}}'

  run_navigation tab next_tab_id

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "tab list --workspace w1" ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t2" ]
}

@test "the real catalog wires Workspace Next to next_workspace_id" {
  export HERDR_PLUGIN_ROOT="$ROOT"
  export FZF_STUB_SELECT_ID="workspace.next"
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
}

@test "the real catalog wires Workspace Previous to previous_workspace_id" {
  export HERDR_PLUGIN_ROOT="$ROOT"
  export FZF_STUB_SELECT_ID="workspace.previous"
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
}

@test "the real catalog wires Tab Next to next_tab_id" {
  export HERDR_PLUGIN_ROOT="$ROOT"
  export FZF_STUB_SELECT_ID="tab.next"
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t3" ]
}

@test "the real catalog wires Tab Previous to previous_tab_id" {
  export HERDR_PLUGIN_ROOT="$ROOT"
  export FZF_STUB_SELECT_ID="tab.previous"
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w1:t1","workspace_id":"w1","label":"one"},{"tab_id":"w1:t2","workspace_id":"w1","label":"two"},{"tab_id":"w1:t3","workspace_id":"w1","label":"three"}]}}'

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w1:t1" ]
}
