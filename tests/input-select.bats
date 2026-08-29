#!/usr/bin/env bats
# Input, select, and confirm screens of the palette binary, driven headlessly
# against the real catalog. These paths had no end-to-end coverage in the bash
# era: the fzf stub answered every screen with the same value, so flows that
# need different answers on successive screens were untestable before
# PALETTE_STUB_SELECT_IDS / PALETTE_STUB_INPUTS existed.

setup() {
  load test_helper
  ROOT="$(repo_root)"

  export HERDR_PLUGIN_ROOT="$ROOT"
  export HERDR_BIN_PATH="$ROOT/tests/stubs/herdr"
  export HERDR_STUB_CALLS="$BATS_TEST_TMPDIR/herdr-calls"
  export ORIGIN_PANE_ID="w1:p1"
  export ORIGIN_TAB_ID="w1:t2"
  export ORIGIN_WORKSPACE_ID="w1"
  export ORIGIN_CWD="$BATS_TEST_TMPDIR"
  export PALETTE_STUB=1
  # Hermetic keybinding hints: never read the developer's real herdr config.
  export HERDR_CONFIG_PATH="$BATS_TEST_TMPDIR/herdr-config.toml"
  export PALETTE_STUB_DUMP="$BATS_TEST_TMPDIR/picker-dump"
}

@test "workspace.new passes the typed directory and --focus" {
  mkdir "$BATS_TEST_TMPDIR/projects"
  export PALETTE_STUB_SELECT_ID="workspace.new"
  export PALETTE_STUB_INPUT="$BATS_TEST_TMPDIR/projects"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace create --cwd $BATS_TEST_TMPDIR/projects --focus" ]
}

@test "workspace.new prefills the input with the origin cwd" {
  export PALETTE_STUB_SELECT_ID="workspace.new"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace create --cwd $ORIGIN_CWD --focus" ]
}

@test "a required input left empty cancels silently" {
  export PALETTE_STUB_SELECT_ID="workspace.rename"
  export PALETTE_STUB_INPUT=""

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -f "$HERDR_STUB_CALLS" ]
}

@test "a directory-validated input rejects a missing path" {
  export PALETTE_STUB_SELECT_ID="workspace.new"
  export PALETTE_STUB_INPUT="$BATS_TEST_TMPDIR/absent"

  run "$(palette_bin)" ui

  [ "$status" -eq 1 ]
  [[ "$output" == *"command-palette: input is not an existing directory"* ]]
}

@test "workspace.rename passes the typed label after the origin workspace id" {
  export PALETTE_STUB_SELECT_ID="workspace.rename"
  export PALETTE_STUB_INPUT="new label"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace rename w1 new label" ]
}

@test "workspace.switch lists the other workspaces and focuses the selection" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'workspace.switch\nw2'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace focus w2" ]
  grep -q "$(printf 'w2\ttwo (w2)')" "$PALETTE_STUB_DUMP"
}

@test "workspace.switch excludes the origin workspace from the candidates" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'workspace.switch\nw2'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  ! grep -q "$(printf 'w1\t')" "$PALETTE_STUB_DUMP"
}

@test "a selector with no candidates exits silently" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'workspace.switch\nw1'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace list" ]
}

@test "workspace labels are sanitized before display" {
  export HERDR_STUB_WORKSPACE_LIST_JSON="$(printf '{"result":{"workspaces":[{"workspace_id":"w2","label":"bad\\nlabel\\there"}]}}')"
  export PALETTE_STUB_SELECT_IDS=$'workspace.switch\nw2'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  grep -q "$(printf 'w2\tbad label here (w2)')" "$PALETTE_STUB_DUMP"
}

@test "tab.switch prefixes candidates with their workspace label" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}'
  export HERDR_STUB_TAB_LIST_JSON='{"result":{"tabs":[{"tab_id":"w2:t1","workspace_id":"w2","label":"edit"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'tab.switch\nw2:t1'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(sed -n '1p' "$HERDR_STUB_CALLS")" = "tab list" ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "tab focus w2:t1" ]
  grep -q "$(printf 'w2:t1\ttwo / edit (w2:t1)')" "$PALETTE_STUB_DUMP"
}

@test "agent.focus targets the selected agent pane" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w2:p9","workspace_id":"w2","terminal_title_stripped":"claude"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'agent.focus\nw2:p9'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "agent focus w2:p9" ]
  grep -q "$(printf 'w2:p9\ttwo / agent: claude (w2:p9)')" "$PALETTE_STUB_DUMP"
}

@test "agent.focus excludes the origin pane from the candidates" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w1:p1","workspace_id":"w1","terminal_title_stripped":"self"},{"pane_id":"w1:p2","workspace_id":"w1","terminal_title_stripped":"other"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'agent.focus\nw1:p2'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  ! grep -q "$(printf 'w1:p1\t')" "$PALETTE_STUB_DUMP"
}

@test "worktree.new passes the origin cwd and the typed branch" {
  export PALETTE_STUB_SELECT_ID="worktree.new"
  export PALETTE_STUB_INPUT="feature/palette"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "worktree create --cwd $ORIGIN_CWD --branch feature/palette --focus" ]
}

@test "worktree.open passes the origin cwd and the typed branch" {
  export PALETTE_STUB_SELECT_ID="worktree.open"
  export PALETTE_STUB_INPUT="feature/palette"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "worktree open --cwd $ORIGIN_CWD --branch feature/palette --focus" ]
}

@test "worktree.remove confirms before removing the selected workspace" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'worktree.remove\nw2\nYes'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "worktree remove --workspace w2" ]
}

@test "worktree.remove answered No leaves the checkout alone" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"},{"workspace_id":"w2","label":"two"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'worktree.remove\nw2\nNo'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -f "$HERDR_STUB_CALLS" ]
}

@test "agent.prompt passes the selected agent before the typed text" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w2:p9","workspace_id":"w2","terminal_title_stripped":"claude"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'agent.prompt\nw2:p9'
  export PALETTE_STUB_INPUT="run the tests"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "agent prompt w2:p9 run the tests" ]
}

@test "agent.prompt keeps the origin pane among the candidates" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w1","label":"one"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w1:p1","workspace_id":"w1","terminal_title_stripped":"self"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'agent.prompt\nw1:p1'
  export PALETTE_STUB_INPUT="status"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "agent prompt w1:p1 status" ]
}

@test "agent.rename passes the typed name after the selected agent" {
  export HERDR_STUB_WORKSPACE_LIST_JSON='{"result":{"workspaces":[{"workspace_id":"w2","label":"two"}]}}'
  export HERDR_STUB_AGENT_LIST_JSON='{"result":{"agents":[{"pane_id":"w2:p9","workspace_id":"w2","terminal_title_stripped":"claude"}]}}'
  export PALETTE_STUB_SELECT_IDS=$'agent.rename\nw2:p9'
  export PALETTE_STUB_INPUT="reviewer"

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "agent rename w2:p9 reviewer" ]
}

@test "a confirm answered No cancels without executing" {
  export PALETTE_STUB_SELECT_IDS=$'workspace.close\nNo'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ ! -f "$HERDR_STUB_CALLS" ]
  grep -q '^No$' "$PALETTE_STUB_DUMP"
  grep -q '^Yes$' "$PALETTE_STUB_DUMP"
}

@test "a confirm answered Yes executes the command" {
  export PALETTE_STUB_SELECT_IDS=$'workspace.close\nYes'

  run "$(palette_bin)" ui

  [ "$status" -eq 0 ]
  [ "$(tail -n 1 "$HERDR_STUB_CALLS")" = "workspace close w1" ]
}
