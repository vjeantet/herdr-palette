#!/usr/bin/env bats
# The `[[prompt]]` half of the user's config.toml: entries that drop a text
# into an agent's input box without submitting it.
#
# The deposit itself goes over herdr's socket, which the CLI stub cannot see;
# under PALETTE_STUB=1 the binary opens no socket and appends the request line
# to PALETTE_STUB_IPC_DUMP instead (PALETTE_STUB_IPC_ERROR makes it fail). The
# agent listing and the final focus still go through the CLI stub.

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
  export PALETTE_STUB_IPC_DUMP="$BATS_TEST_TMPDIR/ipc-dump"
  export ORIGIN_PANE_ID="w1:p1"
  export ORIGIN_TAB_ID="w1:t2"
  export ORIGIN_WORKSPACE_ID="w1"
  export ORIGIN_CWD="$BATS_TEST_TMPDIR"
  export PALETTE_STUB=1
  export HERDR_CONFIG_PATH="$BATS_TEST_TMPDIR/herdr-config.toml"
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"Main"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w1:p7","workspace_id":"w1","terminal_title_stripped":"codex"},{"pane_id":"w1:p1","workspace_id":"w1","terminal_title_stripped":"claude"}]}}'

  write_catalog
}

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

ipc_request() {
  cat "$PALETTE_STUB_IPC_DUMP"
}

ipc_field() {
  jq -r "$1" "$PALETTE_STUB_IPC_DUMP"
}

@test "a prompt is offered after the user commands and ahead of the built-in catalog" {
  write_user_config <<'TOML'
[[command]]
id = "lazygit"
title = "Lazygit"
argv = ["lazygit"]

[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(sed -n 1p "$PALETTE_STUB_DUMP" | cut -f1)" = "user:lazygit" ]
  [ "$(sed -n 2p "$PALETTE_STUB_DUMP" | cut -f1)" = "prompt:hello" ]
  [ "$(sed -n 3p "$PALETTE_STUB_DUMP" | cut -f1)" = "builtin.test" ]
}

@test "a prompt row displays the Prompt prefix and the declared title" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML

  run "$(palette_bin)" ui

  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f2)" = "Prompt: Say hello" ]
}

@test "a fixed text is sent to the chosen agent without any key" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello there"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:hello\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .method)" = "pane.send_input" ]
  [ "$(ipc_field .params.pane_id)" = "w1:p7" ]
  [ "$(ipc_field .params.text)" = "hello there" ]
  [ "$(ipc_field '.params.keys | length')" = "0" ]
}

@test "the chosen agent is focused through the CLI after the deposit" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:hello\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "agent focus w1:p7" ]
}

@test "the agent picker lists every agent with the origin one first and marked" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:hello\nw1:p1')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(sed -n 3p "$PALETTE_STUB_DUMP" | cut -f1)" = "w1:p1" ]
  [[ "$(sed -n 3p "$PALETTE_STUB_DUMP")" == *"- this pane" ]]
  [ "$(sed -n 4p "$PALETTE_STUB_DUMP" | cut -f1)" = "w1:p7" ]
  [[ "$(sed -n 4p "$PALETTE_STUB_DUMP")" != *"- this pane"* ]]
}

@test "a command output is appended to the text on its own line" {
  write_user_config <<'TOML'
[[prompt]]
id = "review"
title = "Review"
text = "Review this:"
argv = ["printf", "line one\nline two\n"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:review\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "$(printf 'Review this:\nline one\nline two')" ]
}

@test "a multi-line text travels as one request" {
  write_user_config <<'TOML'
[[prompt]]
id = "review"
title = "Review"
argv = ["printf", "a\nb\nc\n"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:review\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(wc -l <"$PALETTE_STUB_IPC_DUMP" | tr -d ' ')" = "1" ]
  [ "$(ipc_field .params.text)" = "$(printf 'a\nb\nc')" ]
}

@test "a nonzero exit status still sends the output and warns in the header" {
  write_user_config <<'TOML'
[[prompt]]
id = "fail"
title = "Failing"
argv = ["sh", "-c", "echo boom; exit 7"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:fail\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "boom" ]
}

@test "capture both appends stderr after stdout" {
  write_user_config <<'TOML'
[[prompt]]
id = "both"
title = "Both"
argv = ["sh", "-c", "echo out; echo err >&2"]
capture = "both"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:both\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "$(printf 'out\nerr')" ]
}

@test "stderr is left out by default" {
  write_user_config <<'TOML'
[[prompt]]
id = "out"
title = "Out"
argv = ["sh", "-c", "echo out; echo err >&2"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:out\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "out" ]
}

@test "the command runs in the declared cwd" {
  write_user_config <<TOML
[[prompt]]
id = "where"
title = "Where"
argv = ["pwd"]
cwd = "/"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:where\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "/" ]
}

@test "the command runs in the origin cwd when the entry declares none" {
  write_user_config <<'TOML'
[[prompt]]
id = "where"
title = "Where"
argv = ["pwd"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:where\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "$(cd "$ORIGIN_CWD" && pwd -P)" ] || [ "$(ipc_field .params.text)" = "$ORIGIN_CWD" ]
}

@test "the argv is spawned directly, not through a shell" {
  write_user_config <<'TOML'
[[prompt]]
id = "literal"
title = "Literal"
argv = ["printf", "%s", "$HOME *"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:literal\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = '$HOME *' ]
}

@test "a command producing nothing and no text cancels without any request" {
  write_user_config <<'TOML'
[[prompt]]
id = "empty"
title = "Empty"
argv = ["true"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:empty\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_STUB_IPC_DUMP" ]
  [[ "$(picker_lines)" != *"w1:p7"* ]]
}

@test "a command producing nothing still sends the fixed text" {
  write_user_config <<'TOML'
[[prompt]]
id = "empty"
title = "Empty"
text = "just this"
argv = ["true"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:empty\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(ipc_field .params.text)" = "just this" ]
}

@test "a command past its timeout is killed and reported, nothing is sent" {
  write_user_config <<'TOML'
[[prompt]]
id = "slow"
title = "Slow"
argv = ["sleep", "5"]
timeout_ms = 100
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:slow\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 1 ]
  [[ "$output" == *"sleep timed out after 100 ms"* ]]
  [ ! -e "$PALETTE_STUB_IPC_DUMP" ]
}

@test "a command that cannot start is reported, nothing is sent" {
  write_user_config <<'TOML'
[[prompt]]
id = "missing"
title = "Missing"
argv = ["/no/such/binary/here"]
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:missing\nw1:p7')"

  run "$(palette_bin)" ui

  [ "$status" -eq 1 ]
  [[ "$output" == *"cannot run /no/such/binary/here"* ]]
  [ ! -e "$PALETTE_STUB_IPC_DUMP" ]
}

@test "cancelling the agent picker sends nothing and focuses nothing" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:hello\nw9:p9')"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_STUB_IPC_DUMP" ]
  [[ "$(herdr_calls)" != *"agent focus"* ]]
}

@test "no agent at all cancels silently" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[]}}'
  export PALETTE_STUB_SELECT_ID="prompt:hello"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_STUB_IPC_DUMP" ]
  [[ "$(herdr_calls)" != *"agent focus"* ]]
}

@test "a deposit herdr refuses is reported and exits nonzero" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML
  export PALETTE_STUB_SELECT_IDS="$(printf 'prompt:hello\nw1:p7')"
  export PALETTE_STUB_IPC_ERROR="pane w1:p7 not found"

  run "$(palette_bin)" ui

  [ "$status" -eq 1 ]
  [[ "$output" == *"cannot send Say hello to the agent"* ]]
  [[ "$output" == *"pane w1:p7 not found"* ]]
  [[ "$(herdr_calls)" != *"agent focus"* ]]
}

@test "a rejected prompt never takes the valid entries with it" {
  write_user_config <<'TOML'
[[command]]
id = "fine"
title = "Fine"
argv = ["true"]

[[prompt]]
id = "broken"
title = "Broken"

[[prompt]]
id = "ok"
title = "Ok"
text = "t"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"user:fine"* ]]
  [[ "$(picker_lines)" == *"prompt:ok"* ]]
  [[ "$(picker_lines)" != *"prompt:broken"* ]]
  [[ "$(picker_lines)" == *"builtin.test"* ]]
}

@test "a prompt and a command may share an id" {
  write_user_config <<'TOML'
[[command]]
id = "x"
title = "Command X"
argv = ["true"]

[[prompt]]
id = "x"
title = "Prompt X"
text = "t"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" == *"user:x"* ]]
  [[ "$(picker_lines)" == *"prompt:x"* ]]
}

@test "a file holding only prompts yields no user command rows" {
  write_user_config <<'TOML'
[[prompt]]
id = "hello"
title = "Say hello"
text = "hello"
TOML

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [[ "$(picker_lines)" != *"user:"* ]]
  [ "$(head -n 1 "$PALETTE_STUB_DUMP" | cut -f1)" = "prompt:hello" ]
}
