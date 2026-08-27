#!/usr/bin/env bats
# The keybinding hint column: catalog defaults, user overrides from the herdr
# config ([keys] and [[keys.command]]), and their headless dump as a third
# tab-separated field. The config is read from HERDR_CONFIG_PATH — the same
# override the real herdr honors — pointed at a per-test file.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  TEST_PLUGIN_ROOT="$BATS_TEST_TMPDIR/plugin"
  mkdir -p "$TEST_PLUGIN_ROOT"

  export HERDR_PLUGIN_ROOT="$TEST_PLUGIN_ROOT"
  export HERDR_BIN_PATH="$ROOT/tests/stubs/herdr"
  export HERDR_STUB_CALLS="$BATS_TEST_TMPDIR/herdr-calls"
  export PALETTE_STUB_DUMP="$BATS_TEST_TMPDIR/picker-dump"
  export ORIGIN_PANE_ID="w1:p1"
  export ORIGIN_TAB_ID="w1:t2"
  export ORIGIN_WORKSPACE_ID="w1"
  export ORIGIN_CWD="$BATS_TEST_TMPDIR"
  export PALETTE_STUB=1
  export HERDR_CONFIG_PATH="$BATS_TEST_TMPDIR/herdr-config.toml"

  write_catalog
  export HERDR_STUB_ACTION_LIST_JSON='{"result":{"actions":[
    {"plugin_id":"herdr-scratchpad","action_id":"open-scratchpad","title":"Toggle scratchpad","platforms":["linux","macos"]}
  ]}}'
  export PALETTE_STUB_SELECT_ID="no.such.row"
}

# write_catalog — one command with a default binding, one without any.
write_catalog() {
  cat >"$TEST_PLUGIN_ROOT/commands.json" <<'JSON'
{
  "schema_version": 1,
  "expected_herdr_protocol": 20,
  "commands": [
    {
      "id": "tab.new",
      "title": "Tab: New",
      "description": "Create a tab.",
      "command": ["tab", "create"],
      "keys_action": "new_tab",
      "key": "prefix+c",
      "arguments": []
    },
    {
      "id": "bare.op",
      "title": "Bare: Op",
      "description": "No binding at all.",
      "command": ["workspace", "focus"],
      "arguments": [{"source": "literal", "value": "w9"}]
    }
  ]
}
JSON
}

picker_lines() {
  cat "$PALETTE_STUB_DUMP"
}

@test "a catalog default binding shows as the third dump field" {
  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^tab.new' | cut -f3)" = "prefix+c" ]
}

@test "a command without a binding dumps only two fields" {
  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^bare.op')" = "$(printf 'bare.op\tBare: Op')" ]
}

@test "a keys override in the herdr config beats the catalog default" {
  cat >"$HERDR_CONFIG_PATH" <<'TOML'
[keys]
new_tab = "ctrl+alt+t"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^tab.new' | cut -f3)" = "ctrl+alt+t" ]
}

@test "an empty keys override removes the hint" {
  cat >"$HERDR_CONFIG_PATH" <<'TOML'
[keys]
new_tab = ""
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^tab.new')" = "$(printf 'tab.new\tTab: New')" ]
}

@test "an array override shows its first binding" {
  cat >"$HERDR_CONFIG_PATH" <<'TOML'
[keys]
new_tab = ["prefix+c", "ctrl+alt+t"]
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^tab.new' | cut -f3)" = "prefix+c" ]
}

@test "a keys.command binding shows on its plugin row" {
  cat >"$HERDR_CONFIG_PATH" <<'TOML'
[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "herdr-scratchpad.open-scratchpad"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^plugin:herdr-scratchpad' | cut -f3)" = "prefix+a" ]
}

@test "a plugin action without a binding dumps only two fields" {
  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^plugin:herdr-scratchpad')" = "$(printf 'plugin:herdr-scratchpad.open-scratchpad\tScratchpad: Toggle scratchpad')" ]
}

@test "an unreadable herdr config degrades to catalog defaults" {
  printf 'not toml [' >"$HERDR_CONFIG_PATH"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(picker_lines | grep '^tab.new' | cut -f3)" = "prefix+c" ]
}
