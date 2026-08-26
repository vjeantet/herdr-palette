#!/usr/bin/env bats
# palette.sh inherits the user's fzf options on purpose: theme, colors,
# height, layout and key bindings should make the palette look like the rest
# of their tools. These tests cover the exception — the options that change
# the shape of what fzf prints, which this script reads as "one line, id in
# field 1".
#
# Same boundaries as palette.bats: fzf and herdr are stubbed, and the stub
# reacts to --multi and --print-query the way real fzf would, so these tests
# observe the damage rather than assert on a variable.

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
  export FZF_STUB_SELECT_ID="builtin.two"

  write_catalog
}

# write_catalog — two commands, so a picker that returns more than one row
# has a second row to return, and so the two are told apart by the argument
# the herdr stub records.
write_catalog() {
  cat >"$TEST_PLUGIN_ROOT/commands.json" <<'JSON'
{
  "schema_version": 1,
  "expected_herdr_protocol": 20,
  "commands": [
    {
      "id": "builtin.one",
      "title": "Builtin: One",
      "description": "The first command.",
      "command": ["workspace", "focus"],
      "arguments": [{"source": "literal", "value": "w1"}]
    },
    {
      "id": "builtin.two",
      "title": "Builtin: Two",
      "description": "The second command.",
      "command": ["workspace", "focus"],
      "arguments": [{"source": "literal", "value": "w2"}]
    }
  ]
}
JSON
}

@test "a FZF_DEFAULT_OPTS that enables multi-select still yields a single command" {
  export FZF_DEFAULT_OPTS="--multi"

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
  [ "$(grep -c '^workspace focus' "$HERDR_STUB_CALLS")" -eq 1 ]
}

@test "a FZF_DEFAULT_OPTS that prints the query does not corrupt the selected id" {
  export FZF_DEFAULT_OPTS="--print-query"

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
}

@test "the same options coming from FZF_DEFAULT_OPTS_FILE are neutralized too" {
  opts_file="$BATS_TEST_TMPDIR/fzf-opts"
  printf '# a comment line\n--print-query\n--multi\n' >"$opts_file"
  export FZF_DEFAULT_OPTS_FILE="$opts_file"

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
}

@test "a non-interactive --filter in FZF_DEFAULT_OPTS never reaches fzf" {
  export FZF_STUB_OPTS_DUMP="$BATS_TEST_TMPDIR/fzf-opts-seen"
  export FZF_DEFAULT_OPTS="--filter=zzz --height=40%"

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$FZF_STUB_OPTS_DUMP")" = "--height=40%" ]
}

@test "styling options in FZF_DEFAULT_OPTS reach fzf untouched" {
  export FZF_STUB_OPTS_DUMP="$BATS_TEST_TMPDIR/fzf-opts-seen"
  export FZF_DEFAULT_OPTS="--height=40% --layout=reverse --color=fg:#d0d0d0 --print-query --bind ctrl-a:select-all"

  run bash "$ROOT/palette.sh"

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$FZF_STUB_OPTS_DUMP")" = "--height=40% --layout=reverse --color=fg:#d0d0d0 --bind ctrl-a:select-all" ]
}
