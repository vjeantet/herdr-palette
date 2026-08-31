#!/usr/bin/env bats
# The last-used rule: the row picked in the main picker is remembered in
# $HERDR_PLUGIN_CONFIG_DIR/last-used and leads the list on the next opening.
#
# Same boundaries as the other suites: herdr is stubbed at its process
# boundary, the picker is driven headlessly through the PALETTE_STUB_*
# variables.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  TEST_PLUGIN_ROOT="$BATS_TEST_TMPDIR/plugin"
  TEST_CONFIG_DIR="$BATS_TEST_TMPDIR/plugin-config"
  mkdir -p "$TEST_PLUGIN_ROOT" "$TEST_CONFIG_DIR"

  export HERDR_PLUGIN_ROOT="$TEST_PLUGIN_ROOT"
  export HERDR_PLUGIN_CONFIG_DIR="$TEST_CONFIG_DIR"
  export HERDR_PLUGIN_ID="vjeantet.palette"
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
}

# write_catalog — two built-in commands, so promotion has an order to change.
write_catalog() {
  cat >"$TEST_PLUGIN_ROOT/commands.json" <<'JSON'
{
  "schema_version": 1,
  "expected_herdr_protocol": 20,
  "commands": [{
    "id": "builtin.first",
    "title": "Builtin: First",
    "description": "The first built-in command.",
    "command": ["workspace", "focus"],
    "arguments": [{"source": "literal", "value": "w8"}]
  }, {
    "id": "builtin.second",
    "title": "Builtin: Second",
    "description": "The second built-in command.",
    "command": ["workspace", "focus"],
    "arguments": [{"source": "literal", "value": "w9"}]
  }]
}
JSON
}

write_user_config() {
  cat >"$TEST_CONFIG_DIR/config.toml"
}

state_file() {
  printf '%s/last-used' "$TEST_CONFIG_DIR"
}

@test "selecting a command records its id in the state file" {
  export PALETTE_STUB_SELECT_ID="builtin.second"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(cat "$(state_file)")" = "builtin.second" ]
}

@test "the last used command leads the picker on the next opening" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  printf 'builtin.second\n' >"$(state_file)"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f1)" = "builtin.second" ]
}

@test "the rows behind the promoted one keep their order" {
  printf 'builtin.second\n' >"$(state_file)"

  run "$(palette_bin)" ui

  [ "$(head -n 2 "$PALETTE_STUB_DUMP" | cut -f1 | tail -n 1)" = "builtin.first" ]
}

@test "a last used id matching no row leaves the order alone" {
  printf 'gone.command\n' >"$(state_file)"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f1)" = "builtin.first" ]
}

@test "cancelling the picker records nothing" {
  export PALETTE_STUB_SELECT_ID="no.such.row"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$(state_file)" ]
}

@test "a user command selection is remembered with its namespace" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  export PALETTE_STUB_SELECT_ID="user:lazygit"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(cat "$(state_file)")" = "user:lazygit" ]
}

@test "a selection cancelled downstream is still the last used command" {
  write_user_config <<'TOML'
[[command]]
id = "edit"
title = "Edit a file"
argv = ["nvim"]
input = { prompt = "File", required = true }
TOML
  export PALETTE_STUB_SELECT_ID="user:edit"
  export PALETTE_STUB_INPUT=""

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$HERDR_STUB_CALLS" ]
  [ "$(cat "$(state_file)")" = "user:edit" ]
}

@test "without a config directory the palette neither promotes nor records" {
  unset HERDR_PLUGIN_CONFIG_DIR
  export PALETTE_STUB_SELECT_ID="builtin.second"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$(state_file)" ]
}
