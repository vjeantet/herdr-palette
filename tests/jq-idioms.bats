#!/usr/bin/env bats
# A static guard over the tracked tree rather than a behavioral test.
#
# Under jq 1.6, contains(NEEDLE) is true for EVERY string when NEEDLE begins
# with a NUL byte: jq's substring search walks NUL-terminated C strings, so
# such a needle behaves exactly like the empty one. That made
# resolve_computed_context reject every workspace and every tab, broke all
# four navigation commands on any host shipping jq 1.6 (Debian bookworm's jq,
# and this project's own development machine), and failed the whole of
# palette.bats. The fix compares code points via explode.
#
# jq 1.7 does not have the bug, so on a modern jq nothing at runtime catches a
# reintroduction. This does.

setup() {
  load test_helper
  ROOT="$(repo_root)"

  # An ERE matching a jq contains(...) whose needle carries a control
  # character: escaped (\u0000, \n, \r, \t, \b, \0 -- with any number of
  # leading backslashes, since these filters travel through shell quoting) or
  # present as a raw byte.
  CONTROL_CHAR_CONTAINS='contains\([^)]*(\\+(u00[0-1][0-9A-Fa-f]|[nrtb0])|[[:cntrl:]])'

  # This file necessarily contains the very pattern it searches for.
  SELF='tests/jq-idioms.bats'
}

@test "no tracked file searches for a control character with jq contains" {
  cd "$ROOT" || return 1

  # -I skips docs/assets/demo.gif and any binary added later. Rust sources
  # are excluded: str::contains('\0') is not jq's contains — the binary
  # never embeds a jq filter, jq being gone from its runtime path entirely.
  hits=$(git ls-files | grep -v "^$SELF\$" | grep -v '\.rs$' \
    | xargs grep -I -nE "$CONTROL_CHAR_CONTAINS" || true)
  printf '%s' "$hits" >&2

  [ -z "$hits" ]
}

@test "the guard pattern matches the jq 1.6 form it exists to catch" {
  backslash='\'
  probe="$BATS_TEST_TMPDIR/probe.jq"
  {
    printf 'select(.id | contains("%su0000"))\n' "$backslash"
    printf 'select(.id | contains("%sn"))\n' "$backslash"
  } >"$probe"

  run grep -cE "$CONTROL_CHAR_CONTAINS" "$probe"

  [ "$status" -eq 0 ]
  [ "$output" -eq 2 ]
}

@test "the guard pattern leaves an ordinary contains alone" {
  probe="$BATS_TEST_TMPDIR/probe.jq"
  printf 'select(.label | contains("workspace"))\n' >"$probe"

  run grep -qE "$CONTROL_CHAR_CONTAINS" "$probe"

  [ "$status" -ne 0 ]
}
