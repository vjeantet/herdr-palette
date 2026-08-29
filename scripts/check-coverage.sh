#!/usr/bin/env bash
#
# check-coverage.sh — verify commands.json still exposes every herdr key
# action it reasonably can.
#
# scripts/check-compat.sh runs from the catalog toward herdr: everything
# commands.json declares must resolve against the installed CLI. This script
# runs the other way. docs/design/command-catalog.md sets the goal it checks:
# "Expose herdr's existing built-in keybindings as searchable commands
# wherever the public CLI can achieve the same effect safely." So every key
# action herdr declares must be either exposed by commands.json (matched on
# "keys_action") or classified in keys-coverage.json with a reason. A herdr
# release that adds an action turns this red instead of silently widening the
# gap.
#
# The reference list is the [keys] section of `herdr --default-config`,
# stopping at the next section header, real or commented out (e.g.
# "# [[keys.command]]"), so the example custom-command block's key/type/
# command lines are not mistaken for actions. Only a full `# name = "value"`
# line counts, with an optional trailing "# ..." comment; the prose lines
# above that block ('# type = "shell" runs detached in the background.') do
# not match.
#
# That template is hand-written in herdr's src/main.rs, not generated from
# the KeysConfig struct, and it omits real fields (measured against 0.8.2:
# copy_mode and the four swap_pane_*). This is therefore a ratchet, not a
# proof: an action the template also omits stays invisible here. A classified
# action the template does not list is accepted for that same reason.
# Re-audit KeysConfig in herdr's src/config/model.rs by hand when upgrading
# herdr.
#
# Usage: check-coverage.sh [COVERAGE_JSON]   (default: repo's keys-coverage.json)
#
# Bash 3.2 compatible (macOS default /bin/bash).

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
catalog="$repo_root/commands.json"
coverage="${1:-$repo_root/keys-coverage.json}"
herdr_bin="${HERDR_BIN_PATH:-herdr}"

valid_reasons='["no-cli", "by-design", "redundant", "not-an-action"]'

failed=0

fail() {
  echo "FAIL: $1"
  failed=1
}

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq not found on PATH"
  exit 1
fi

if ! command -v "$herdr_bin" >/dev/null 2>&1; then
  echo "FAIL: herdr binary not found (looked for: $herdr_bin)"
  exit 1
fi

if [ ! -f "$catalog" ]; then
  echo "FAIL: catalog not found: $catalog"
  exit 1
fi

if [ ! -f "$coverage" ]; then
  echo "FAIL: coverage list not found: $coverage"
  exit 1
fi

default_config=$("$herdr_bin" --default-config 2>/dev/null)
if [ -z "$default_config" ]; then
  echo "NOTE: herdr --default-config prints nothing; nothing to check against"
  exit 0
fi

template_actions=$(printf '%s\n' "$default_config" | awk '
  /^\[keys\]/ { in_keys = 1; next }
  in_keys && (/^\[/ || /^# *\[/) { exit }
  in_keys
' | sed -nE 's/^# *([a-z_]+) = "[^"]*" *(#.*)?$/\1/p' | sort -u)

catalog_actions=$(jq -r '.commands[].keys_action // empty' "$catalog" | sort -u)
coverage_actions=$(jq -r '.uncovered[]?.keys_action // empty' "$coverage" 2>/dev/null | sort -u)

if [ -z "$template_actions" ]; then
  fail "could not extract any key action from the [keys] section of herdr --default-config"
fi

if [ -z "$coverage_actions" ]; then
  fail "$coverage lists no uncovered action (unreadable, or .uncovered is empty)"
fi

bad_reasons=$(jq -r --argjson valid "$valid_reasons" '
  .uncovered[]?
  | select(.reason as $r | $valid | index($r) | not)
  | .keys_action + " (" + (.reason // "missing") + ")"
' "$coverage" 2>/dev/null)
if [ -n "$bad_reasons" ]; then
  fail "entries with an unknown reason:"
  printf '%s\n' "$bad_reasons"
fi

dup_coverage=$(jq -r '
  [.uncovered[]?.keys_action]
  | group_by(.)
  | map(select(length > 1) | .[0])
  | .[]
' "$coverage" 2>/dev/null)
if [ -n "$dup_coverage" ]; then
  fail "duplicate keys_action in the coverage list:"
  printf '%s\n' "$dup_coverage"
fi

while IFS= read -r action; do
  [ -n "$action" ] || continue
  if printf '%s\n' "$catalog_actions" | grep -qx -- "$action"; then
    continue
  fi
  if printf '%s\n' "$coverage_actions" | grep -qx -- "$action"; then
    continue
  fi
  fail "herdr's [keys] lists $action but commands.json does not expose it and the coverage list does not classify it"
done <<EOF
$template_actions
EOF

while IFS= read -r action; do
  [ -n "$action" ] || continue
  if printf '%s\n' "$catalog_actions" | grep -qx -- "$action"; then
    fail "the coverage list declares $action uncovered but commands.json exposes it"
  fi
done <<EOF
$coverage_actions
EOF

if [ "$failed" -ne 0 ]; then
  exit 1
fi

covered_count=$(printf '%s\n' "$catalog_actions" | grep -c '[^[:space:]]')
uncovered_count=$(printf '%s\n' "$coverage_actions" | grep -c '[^[:space:]]')
herdr_version=$("$herdr_bin" --version 2>/dev/null | awk '{ print $2 }')
echo "OK: herdr $herdr_version key actions are accounted for ($covered_count exposed, $uncovered_count classified)"
