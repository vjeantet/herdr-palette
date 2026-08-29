#!/usr/bin/env bats
# scripts/check-coverage.sh tests, driven against tests/stubs/herdr (a fake
# herdr CLI reproducing real herdr 0.8.0 output shapes) via HERDR_BIN_PATH.
# The script takes the coverage list as an optional argument, so a doctored
# list needs a fixture file, not a doctored repository tree.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  HERDR_BIN_PATH="$(herdr_stub)"
  export HERDR_BIN_PATH
  FIXTURES="$ROOT/tests/fixtures/coverage"
}

@test "OK on the repository's own catalog and coverage list" {
  run bash "$ROOT/scripts/check-coverage.sh"
  [ "$status" -eq 0 ]
  [[ "$output" == "OK: herdr 0.8.0 key actions are accounted for"* ]]
}

@test "FAILs when herdr declares a key action that is neither exposed nor classified" {
  HERDR_STUB_DEFAULT_CONFIG='[keys]
# teleport_pane = "prefix+t"'
  export HERDR_STUB_DEFAULT_CONFIG
  run bash "$ROOT/scripts/check-coverage.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"herdr's [keys] lists teleport_pane but commands.json does not expose it and the coverage list does not classify it"* ]]
}

@test "OK when a new herdr key action is classified in the coverage list" {
  HERDR_STUB_DEFAULT_CONFIG='[keys]
# teleport_pane = "prefix+t"'
  export HERDR_STUB_DEFAULT_CONFIG
  run bash "$ROOT/scripts/check-coverage.sh" "$FIXTURES/classifies-teleport-pane.json"
  [ "$status" -eq 0 ]
}

@test "OK when the template omits an action the coverage list classifies" {
  # copy_mode is a real KeysConfig field the hand-written template forgets;
  # a classified action the template does not list must not fail.
  HERDR_STUB_DEFAULT_CONFIG='[keys]
# new_tab = "prefix+c"'
  export HERDR_STUB_DEFAULT_CONFIG
  run bash "$ROOT/scripts/check-coverage.sh"
  [ "$status" -eq 0 ]
}

@test "FAILs when the coverage list declares an action the catalog exposes" {
  run bash "$ROOT/scripts/check-coverage.sh" "$FIXTURES/stale-entry.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *"the coverage list declares new_tab uncovered but commands.json exposes it"* ]]
}

@test "FAILs on a coverage entry with an unknown reason" {
  run bash "$ROOT/scripts/check-coverage.sh" "$FIXTURES/unknown-reason.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown reason"* ]]
}

@test "FAILs when the coverage list is missing" {
  run bash "$ROOT/scripts/check-coverage.sh" "$BATS_TEST_TMPDIR/absent.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *"coverage list not found"* ]]
}

@test "FAILs when the template has no keys section at all" {
  HERDR_STUB_DEFAULT_CONFIG='[theme]
# accent = "blue"'
  export HERDR_STUB_DEFAULT_CONFIG
  run bash "$ROOT/scripts/check-coverage.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"could not extract any key action"* ]]
}

@test "OK with a note on a herdr without --default-config" {
  HERDR_STUB_DEFAULT_CONFIG=""
  export HERDR_STUB_DEFAULT_CONFIG
  run bash "$ROOT/scripts/check-coverage.sh"
  [ "$status" -eq 0 ]
  [[ "$output" == *"nothing to check against"* ]]
}
