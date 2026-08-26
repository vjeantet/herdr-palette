#!/usr/bin/env bats
# Schema tests: commands.json must validate against commands.schema.json,
# and each fixture under tests/fixtures/schema/ must be rejected for the
# specific reason its filename describes.

setup() {
  load test_helper
  ROOT="$(repo_root)"
}

# check_jsonschema FILE — runs check-jsonschema the same way
# scripts/check-compat.sh does (prefer uvx, fall back to a plain install).
check_jsonschema() {
  if command -v uvx >/dev/null 2>&1; then
    uvx check-jsonschema --schemafile "$ROOT/commands.schema.json" "$1"
  else
    check-jsonschema --schemafile "$ROOT/commands.schema.json" "$1"
  fi
}

@test "the real commands.json validates against commands.schema.json" {
  run check_jsonschema "$ROOT/commands.json"
  [ "$status" -eq 0 ]
}

@test "accepts computed context keys as command arguments" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/valid-computed-context.json"
  [ "$status" -eq 0 ]
}

@test "rejects a computed context key as an input default" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-computed-default-context.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'next_tab_id'* ]]
}

@test "rejects a computed context key as a selector exclusion" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-computed-exclude-context.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'next_workspace_id'* ]]
}

@test "rejects an argument with an unknown source" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-unknown-source.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.commands[0].arguments[0]'* ]]
}

@test "rejects an argument with an extra unknown field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-extra-field.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.commands[0].arguments[0]'* ]]
}

@test "rejects the removed post_close field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-post-close-field.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *"'post_close' was unexpected"* ]]
}

@test "rejects a newline embedded in a string field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-newline-in-string.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.commands[0].title'* ]]
}

@test "rejects a trailing newline in a string field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-trailing-newline.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.commands[0].title'* ]]
}

@test "rejects a tab embedded in a string field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-tab-in-string.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.commands[0].title'* ]]
}

@test "rejects schema_version other than 1" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-schema-version.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *'$.schema_version'* ]]
}

@test "rejects a command missing a required field" {
  run check_jsonschema "$ROOT/tests/fixtures/schema/invalid-missing-title.json"
  [ "$status" -ne 0 ]
  [[ "$output" == *"'title' is a required property"* ]]
}
