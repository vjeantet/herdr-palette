#!/usr/bin/env bats
# The user half of the palette: entries declared in the user's own
# config.toml, and their dispatch through `herdr plugin pane open` onto this
# plugin's `runner` entrypoint.
#
# Same boundaries as the other suites: herdr is stubbed at its process
# boundary, the picker is driven headlessly through the PALETTE_STUB_*
# variables. The `run` subcommand is exercised directly — with PALETTE_STUB=1,
# which is also what tells it not to wait for a keypress it will never get.

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

# write_catalog — the built-in half, reduced to one observable command, so
# these tests can assert on ordering against something.
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

write_user_config() {
  cat >"$TEST_CONFIG_DIR/config.toml"
}

picker_lines() {
  cat "$PALETTE_STUB_DUMP"
}

herdr_calls() {
  cat "$HERDR_STUB_CALLS"
}

@test "a user command is offered ahead of the built-in catalog" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f1)" = "user:lazygit" ]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
}

@test "a user row displays the User prefix and the declared title" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML

  run "$(palette_bin)" ui

  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f2)" = "User: Lazygit" ]
}

@test "selecting a user command opens a pane on the runner entrypoint" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  export PALETTE_STUB_SELECT_ID="user:lazygit"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(herdr_calls)" == *"plugin pane open --plugin vjeantet.palette --entrypoint runner"* ]]
  [[ "$(herdr_calls)" == *"--env PALETTE_RUN_ARGV=[\"lazygit\"]"* ]]
}

@test "a split targets the origin pane and names no workspace" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  export PALETTE_STUB_SELECT_ID="user:lazygit"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"--target-pane w1:p1"* ]]
  [[ "$(herdr_calls)" != *"--workspace"* ]]
}

@test "a tab names the origin workspace and targets no pane" {
  write_user_config <<'TOML'
[[command]]
id = "notes"
title = "Notes"
argv = ["true"]
placement = "tab"
TOML
  export PALETTE_STUB_SELECT_ID="user:notes"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"--workspace w1"* ]]
  [[ "$(herdr_calls)" != *"--target-pane"* ]]
}

@test "an entry without a placement is opened as a split" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  export PALETTE_STUB_SELECT_ID="user:lazygit"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"--placement split"* ]]
}

@test "the declared placement is passed through to herdr" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
placement = "zoomed"
TOML
  export PALETTE_STUB_SELECT_ID="user:lazygit"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"--placement zoomed"* ]]
}

@test "hold travels to the runner as an environment variable" {
  write_user_config <<'TOML'
[[command]]
id = "test"
title = "Cargo test"
argv = ["cargo", "test"]
hold = true
TOML
  export PALETTE_STUB_SELECT_ID="user:test"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"--env PALETTE_RUN_HOLD=1"* ]]
}

@test "an entry without hold sends no hold variable" {
  write_user_config <<'TOML'
[[command]]
id = "test"
title = "Cargo test"
argv = ["cargo", "test"]
TOML
  export PALETTE_STUB_SELECT_ID="user:test"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" != *"PALETTE_RUN_HOLD"* ]]
}

@test "the opened pane is renamed with the entry title" {
  write_user_config <<'TOML'
[[command]]
id = "test"
title = "Cargo test"
argv = ["cargo", "test"]
TOML
  export PALETTE_STUB_SELECT_ID="user:test"

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"pane rename w1:p9 Cargo test"* ]]
}

@test "a launch herdr refuses is reported and exits nonzero" {
  write_user_config <<'TOML'
[[command]]
id = "test"
title = "Cargo test"
argv = ["cargo", "test"]
TOML
  export PALETTE_STUB_SELECT_ID="user:test"
  export HERDR_STUB_PANE_OPEN_STATUS=1
  export HERDR_STUB_PANE_OPEN_ERROR="no active pane"

  run "$(palette_bin)" ui

  [ "$status" -eq 1 ]
  [[ "$output" == *"cannot run Cargo test"* ]]
  [[ "$output" == *"no active pane"* ]]
}

@test "an input value is appended to argv as one element" {
  write_user_config <<'TOML'
[[command]]
id = "edit"
title = "Edit a file"
argv = ["nvim"]
input = { prompt = "File", required = true }
TOML
  export PALETTE_STUB_SELECT_ID="user:edit"
  export PALETTE_STUB_INPUT="my notes.md"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(herdr_calls)" == *"PALETTE_RUN_ARGV=[\"nvim\",\"my notes.md\"]"* ]]
}

@test "a required input left empty cancels without opening a pane" {
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
}

@test "an optional input left empty adds no argv element" {
  write_user_config <<'TOML'
[[command]]
id = "edit"
title = "Edit a file"
argv = ["nvim"]
input = { prompt = "File" }
TOML
  export PALETTE_STUB_SELECT_ID="user:edit"
  export PALETTE_STUB_INPUT=""

  run "$(palette_bin)" ui

  [[ "$(herdr_calls)" == *"PALETTE_RUN_ARGV=[\"nvim\"]"* ]]
}

@test "a rejected entry never takes the valid ones with it" {
  write_user_config <<'TOML'
[[command]]
id = "broken"
title = "Broken"
argv = []

[[command]]
id = "fine"
title = "Fine"
argv = ["true"]
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"user:fine"* ]]
  [[ "$(picker_lines)" != *"user:broken"* ]]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
}

@test "an unparsable user file leaves the built-in palette usable" {
  write_user_config <<'TOML'
[[command]
id = "broken
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
  [[ "$(picker_lines)" != *"user:"* ]]
}

@test "no user file at all yields no user rows" {
  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
  [[ "$(picker_lines)" != *"user:"* ]]
}

@test "an unset plugin config dir yields no user rows" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]
TOML
  unset HERDR_PLUGIN_CONFIG_DIR

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" != *"user:"* ]]
}

@test "the runner spawns the argv it is given" {
  export PALETTE_RUN_ARGV='["printf","hello %s","world"]'

  run "$(palette_bin)" run

  [ "$status" -eq 0 ]
  [ "$output" = "hello world" ]
}

@test "the runner propagates the command exit status and says so" {
  export PALETTE_RUN_ARGV='["sh","-c","exit 3"]'

  run "$(palette_bin)" run

  [ "$status" -eq 3 ]
  [[ "$output" == *"exited with status 3"* ]]
}

@test "the runner reports a command it cannot start" {
  export PALETTE_RUN_ARGV='["definitely-not-a-real-binary"]'

  # Deliberately not bats' `run`: it warns (BW01) whenever the command under
  # test exits 127, which is exactly the status this test is about. The
  # `run -127` form that silences it needs bats >= 1.5, newer than the
  # distro bats CI installs.
  local out status
  out=$("$(palette_bin)" run 2>&1) && status=0 || status=$?

  [ "$status" -eq 127 ]
  [[ "$out" == *"cannot run definitely-not-a-real-binary"* ]]
}

@test "a successful run says nothing" {
  export PALETTE_RUN_ARGV='["true"]'

  run "$(palette_bin)" run

  [ "$status" -eq 0 ]
  [ "$output" = "" ]
}

@test "the runner refuses an empty argv" {
  export PALETTE_RUN_ARGV='[]'

  run "$(palette_bin)" run

  [ "$status" -eq 1 ]
  [[ "$output" == *"PALETTE_RUN_ARGV is empty"* ]]
}

@test "the runner refuses to start without an argv" {
  run "$(palette_bin)" run

  [ "$status" -eq 1 ]
  [[ "$output" == *"PALETTE_RUN_ARGV is not set"* ]]
}
