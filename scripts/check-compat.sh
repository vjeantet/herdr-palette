#!/usr/bin/env bash
#
# check-compat.sh — verify commands.json is compatible with the installed herdr.
#
# Implements the checks from the "Compatibility checks" section of
# docs/design/command-catalog.md:
#   1. commands.json validates against commands.schema.json, and all "id"
#      values are unique (JSON Schema cannot express cross-item uniqueness).
#   2. commands.json's expected_herdr_protocol matches the protocol reported
#      by `herdr api schema`.
#   3. every unique [group, subcommand] pair in commands.json[].command
#      resolves to a real herdr subcommand.
#   4. the three named selectors' list commands (workspace list, tab list,
#      agent list) resolve the same way.
#   5. every command supplies positional arguments compatible in count and
#      name with its subcommand's required positionals.
#   6. every "key" (default keybinding hint) in commands.json matches the
#      default herdr's --default-config template records for its
#      "keys_action" field, where the template lists that field.
#
# The reverse direction — that the catalog exposes every herdr key action it
# reasonably can — is scripts/check-coverage.sh, not this script.
# It also checks that every literal "--flag" argument in commands.json
# actually appears in that subcommand's help output, without duplicating the
# flag list in this script — the flags are extracted from commands.json.
#
# commands.json is the single source of truth for subcommands and flags;
# nothing here hardcodes that catalog's contents beyond the three selector
# list commands, which are not represented in commands.json by design (see
# spec: selectors are named and implemented in palette.sh's case statement,
# not as JSON-declared shell commands).
#
# Bash 3.2 compatible (macOS default /bin/bash).

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
catalog="$repo_root/commands.json"
schema="$repo_root/commands.schema.json"
herdr_bin="${HERDR_BIN_PATH:-herdr}"

failed=0

fail() {
  echo "FAIL: $1"
  failed=1
}

# --- Fatal preconditions: nothing below can run meaningfully without these ---

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

if [ ! -f "$schema" ]; then
  echo "FAIL: schema not found: $schema"
  exit 1
fi

# --- Check 1: schema validation + id uniqueness ---

# Prefer uvx (no separate install step needed); fall back to a plain
# check-jsonschema on PATH if uvx isn't available (e.g. CI installed it
# directly). Either way it's the same tool and the same invocation shape.
if command -v uvx >/dev/null 2>&1; then
  jsonschema_output=$(uvx check-jsonschema --schemafile "$schema" "$catalog" 2>&1)
  jsonschema_exit=$?
elif command -v check-jsonschema >/dev/null 2>&1; then
  jsonschema_output=$(check-jsonschema --schemafile "$schema" "$catalog" 2>&1)
  jsonschema_exit=$?
else
  jsonschema_output="neither uvx nor check-jsonschema found on PATH"
  jsonschema_exit=1
fi

if [ "$jsonschema_exit" -ne 0 ]; then
  fail "commands.json does not validate against commands.schema.json:"
  printf '%s\n' "$jsonschema_output"
fi

dup_ids=$(jq -r '
  [.commands[].id]
  | group_by(.)
  | map(select(length > 1) | .[0])
  | .[]
' "$catalog")
if [ -n "$dup_ids" ]; then
  fail "duplicate command ids in commands.json:"
  printf '%s\n' "$dup_ids"
fi

# --- Check 2: expected_herdr_protocol vs `herdr api schema` ---

expected_protocol=$(jq -r '.expected_herdr_protocol' "$catalog")
actual_protocol=$("$herdr_bin" api schema 2>/dev/null | awk -F': ' '/^protocol:/ { print $2; exit }')

if [ -z "$actual_protocol" ]; then
  fail "could not read protocol from 'herdr api schema' output"
elif [ "$actual_protocol" != "$expected_protocol" ]; then
  fail "protocol mismatch: commands.json expects $expected_protocol, herdr reports $actual_protocol"
fi

# --- Helper: run "herdr <group> <sub> -h" and confirm it is really that
# subcommand's help, not a fallback.
#
# herdr quirk (measured against 0.8.0, 2026-08-16): an unrecognized group
# (e.g. a typo'd "command" pair) falls back to herdr's top-level help with
# exit 0 — "-h" and "--help" behave identically here, so the flag choice
# alone does not guard against this. An unrecognized subcommand within a
# known group does reliably exit nonzero. As defense in depth against both
# cases, and to avoid relying on exit-status behavior that could change,
# this also requires the output to contain a real subcommand usage line
# "Usage: herdr <group> <sub> ...", which only a genuine subcommand's help
# prints; the top-level fallback does not.
#
# Sets subcommand_help_output as a side effect for the caller to reuse (e.g.
# to check for specific flags) — this script targets bash 3.2, which cannot
# return strings from functions cleanly.
get_subcommand_help() {
  group="$1"
  sub="$2"
  subcommand_help_output=$("$herdr_bin" "$group" "$sub" -h 2>&1)
  subcommand_help_exit=$?
  if [ "$subcommand_help_exit" -ne 0 ]; then
    fail "herdr $group $sub -h exited $subcommand_help_exit"
    return 1
  fi
  if ! printf '%s\n' "$subcommand_help_output" | grep -qE "^Usage: herdr ${group} ${sub}( |\$)"; then
    fail "herdr $group $sub -h did not print that subcommand's usage line (looks like a fallback to group or top-level help)"
    return 1
  fi
  return 0
}

# compute_value_flags — parses $subcommand_help_output's Options section and
# sets value_flags to a space-separated list of "--flag" tokens that consume
# the next argv element as their value (identified by a "<...>" placeholder
# on the same Options line). Flags not in this list are treated as boolean.
compute_value_flags() {
  value_flags=$(printf '%s\n' "$subcommand_help_output" | awk '
    /^Options:/ { in_opts = 1; next }
    in_opts && /^ +--[a-zA-Z-]+/ && /</ {
      match($0, /--[a-zA-Z-]+/)
      print substr($0, RSTART, RLENGTH)
    }
  ' | tr '\n' ' ')
}

# flag_takes_value FLAG — returns success if FLAG is in $value_flags.
flag_takes_value() {
  case " $value_flags " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

# compute_known_flags — parses $subcommand_help_output's Options section and
# sets known_flags to a space-separated list of every "--flag" token the
# subcommand declares, value-taking or boolean. Used both to tell a
# genuinely missing flag value apart from a literal value that happens to
# start with "--" (Check 5), and to confirm a literal "--flag" argument in
# commands.json exactly matches a declared option rather than merely being a
# substring of one, e.g. "--focus" must not pass just because "--focused" is
# declared (Check 3).
compute_known_flags() {
  known_flags=$(printf '%s\n' "$subcommand_help_output" | awk '
    /^Options:/ { in_opts = 1; next }
    in_opts && /^ +--[a-zA-Z-]+/ {
      match($0, /--[a-zA-Z-]+/)
      print substr($0, RSTART, RLENGTH)
    }
  ' | tr '\n' ' ')
}

# flag_is_known FLAG — returns success if FLAG is in $known_flags.
flag_is_known() {
  case " $known_flags " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

# --- Check 3: every unique [group, subcommand] pair used by commands.json,
# plus (not duplicating the flag list) every literal "--flag" argument that
# pair uses.

pairs_and_flags=$(jq -r '
  .commands
  | group_by(.command)
  | map({
      group: .[0].command[0],
      sub: .[0].command[1],
      flags: ([.[].arguments[]? | select(.source == "literal" and (.value | startswith("--"))) | .value] | unique)
    })
  | .[]
  | [.group, .sub, (.flags | join(","))]
  | @tsv
' "$catalog")

while IFS=$'\t' read -r group sub flags; do
  [ -z "$group" ] && continue
  if get_subcommand_help "$group" "$sub"; then
    if [ -n "$flags" ]; then
      compute_known_flags
      old_ifs="$IFS"
      IFS=','
      # shellcheck disable=SC2086 # intentional: split flags on IFS=','
      set -- $flags
      IFS="$old_ifs"
      for flag in "$@"; do
        if ! flag_is_known "$flag"; then
          fail "herdr $group $sub -h does not mention $flag (used as a literal argument in commands.json)"
        fi
      done
    fi
  fi
done <<EOF
$pairs_and_flags
EOF

# --- Check 4: the three named selectors' list commands ---

get_subcommand_help workspace list
get_subcommand_help tab list
if ! printf '%s\n' "$subcommand_help_output" | grep -qF -- "--workspace"; then
  fail "herdr tab list -h does not mention --workspace (required by computed tab context)"
fi
get_subcommand_help agent list

# --- Check 5: required positional argument count and name for every command ---
#
# `herdr <group> <sub> -h` prints a Usage line whose trailing tokens describe
# required positionals as bare `<NAME>` tokens (optionally suffixed `...` for
# "one or more"), optional positionals as `[NAME]` / `[NAME]...`, and
# required options as a `--flag <VALUE>` pair outside `[OPTIONS]`.
# commands.json never spells out a positional directly — each is supplied via
# a `context`, `select`, `input`, or non-flag `literal` argv element. This
# check derives, for each command, which of its resolved argv elements are
# positionals (by walking off known `--flag`/`--flag value` pairs using the
# subcommand's own Options section) and confirms their count and name are
# compatible with what the subcommand requires.
#
# Name compatibility mapping (verified against herdr 0.8.0's help text for
# all 31 commands, 2026-08-16; see docs/design/command-catalog.md,
# "Compatibility checks"):
#   context.key workspace_id / next_workspace_id /
#     previous_workspace_id / select.selector workspaces   -> workspace_id
#   context.key tab_id / next_tab_id / previous_tab_id /
#     select.selector tabs                                 -> tab_id
#   context.key pane_id                                    -> pane_id
#   select.selector agents                                 -> target (herdr
#     accepts a pane id for `agent focus <target>`; see `herdr --skill`)
#   input, or a non-flag literal value                      -> matches any
#     required placeholder (free text; count is still enforced)
# Comparison is case-insensitive because herdr's own help text is
# inconsistent about it (e.g. `workspace close <workspace_id>` vs
# `workspace rename <WORKSPACE_ID>` name the same placeholder).

# compute_required_positionals GROUP SUB — parses $subcommand_help_output's
# Usage line and sets:
#   required_positional_names  — array (lowercased) of the fixed required
#                                 positionals, in order.
#   positional_open_ended      — 1 if a "..." (on a required or an optional
#                                 positional) makes the count unbounded above
#                                 required_positional_names; a plain
#                                 non-variadic "[NAME]" does NOT set this —
#                                 see optional_positional_count.
#   optional_positional_count  — count of non-variadic optional positionals
#                                 ("[NAME]" without "..."), each raising the
#                                 max supplied count by exactly one. Only
#                                 meaningful when positional_open_ended is 0.
#   required_option_flags      — array of "--flag" tokens that are required
#                                 options in the Usage line (outside
#                                 "[OPTIONS]"), e.g. "--direction" in
#                                 "herdr pane focus [OPTIONS] --direction <DIRECTION>".
compute_required_positionals() {
  group="$1"
  sub="$2"
  usage_line=$(printf '%s\n' "$subcommand_help_output" | grep -E "^Usage: herdr ${group} ${sub}( |\$)" | head -n 1)
  tail="${usage_line#"Usage: herdr $group $sub"}"

  required_positional_names=()
  required_option_flags=()
  optional_positional_count=0
  positional_open_ended=0
  prev_flag=0

  for tok in $tail; do
    if [ "$prev_flag" -eq 1 ]; then
      prev_flag=0
      continue
    fi
    first_char="${tok:0:1}"
    if [ "$first_char" = "-" ]; then
      # A "--flag" token in the Usage line is a required option; the "<...>"
      # token right after it is that option's value, not a positional.
      required_option_flags+=("$tok")
      prev_flag=1
      continue
    fi
    if [ "$tok" = "[OPTIONS]" ]; then
      continue
    fi
    if [ "$first_char" = "[" ]; then
      # An optional positional. "[LABEL]..." is variadic (unbounded count,
      # like a required "..." positional); a plain "[PANE_ID]" is a single
      # optional slot and only raises the max supplied count by one.
      case "$tok" in
        *'...')
          positional_open_ended=1
          ;;
        *)
          optional_positional_count=$((optional_positional_count + 1))
          ;;
      esac
      continue
    fi
    if [ "$first_char" = "<" ]; then
      name="${tok#<}"
      name="${name%%>*}"
      name_lower=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')
      required_positional_names+=("$name_lower")
      case "$tok" in
        *'>...')
          positional_open_ended=1
          ;;
      esac
    fi
  done
}

# compute_supplied_positionals ID CMD_JSON — walks CMD_JSON's "arguments"
# array in order and sets supplied_positionals (array) to the resolved argv
# elements that are NOT a "--flag" or a value consumed by a preceding
# "--flag". Each element is a name-compatibility descriptor: a context key,
# a selector's mapped name, or "any" for input/literal free values.
#
# A value-taking flag whose next token is itself a declared flag of the same
# subcommand (per $known_flags) means the catalog is missing that flag's
# value — herdr would misparse this the same way, consuming the next flag as
# a value string instead of running it as a flag. That's a FAIL, not a
# skipped token; a next token starting with "--" that ISN'T a declared flag
# is still treated as a legitimate value (matches how clap would parse it).
compute_supplied_positionals() {
  id="$1"
  supplied_positionals=()

  arg_descs=$(jq -r '
    .arguments[]
    | if .source == "literal" then
        if (.value | startswith("--")) then "FLAG:" + .value
        else "OTHER:any"
        end
      elif .source == "context" then
        "OTHER:" + (
          if (.key == "next_workspace_id" or .key == "previous_workspace_id") then "workspace_id"
          elif (.key == "next_tab_id" or .key == "previous_tab_id") then "tab_id"
          else .key
          end
        )
      elif .source == "input" then "OTHER:any"
      elif .source == "select" then
        "OTHER:" + (
          if .selector == "workspaces" then "workspace_id"
          elif .selector == "tabs" then "tab_id"
          elif .selector == "agents" then "target"
          else "any"
          end
        )
      else "OTHER:any"
      end
  ' <<<"$2")

  descs=()
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    descs+=("$line")
  done <<EOF2
$arg_descs
EOF2

  desc_count=${#descs[@]}
  i=0
  while [ "$i" -lt "$desc_count" ]; do
    desc="${descs[$i]}"
    case "$desc" in
      FLAG:*)
        flag="${desc#FLAG:}"
        if flag_takes_value "$flag"; then
          next_i=$((i + 1))
          if [ "$next_i" -ge "$desc_count" ]; then
            fail "$id: flag $flag is missing its value (no argument follows it)"
          else
            next_desc="${descs[$next_i]}"
            case "$next_desc" in
              FLAG:*)
                next_flag="${next_desc#FLAG:}"
                if flag_is_known "$next_flag"; then
                  fail "$id: flag $flag is missing its value (next token is $next_flag)"
                else
                  i="$next_i"
                fi
                ;;
              *)
                i="$next_i"
                ;;
            esac
          fi
        fi
        ;;
      OTHER:*)
        supplied_positionals+=("${desc#OTHER:}")
        ;;
    esac
    i=$((i + 1))
  done
}

pairs=$(jq -r '[.commands[].command] | unique | .[] | @tsv' "$catalog")

while IFS=$'\t' read -r group sub; do
  [ -z "$group" ] && continue
  if ! get_subcommand_help "$group" "$sub"; then
    continue
  fi

  compute_required_positionals "$group" "$sub"
  compute_value_flags
  compute_known_flags

  ids_for_pair=$(jq -r --arg g "$group" --arg s "$sub" '
    .commands[] | select(.command[0] == $g and .command[1] == $s) | .id
  ' "$catalog")

  while IFS= read -r id; do
    [ -z "$id" ] && continue

    cmd_json=$(jq -c --arg id "$id" '.commands[] | select(.id == $id)' "$catalog")
    compute_supplied_positionals "$id" "$cmd_json"

    required_count=${#required_positional_names[@]}
    supplied_count=${#supplied_positionals[@]}

    if [ "$positional_open_ended" -eq 1 ]; then
      if [ "$supplied_count" -lt "$required_count" ]; then
        fail "$id: herdr $group $sub requires at least $required_count positional argument(s), commands.json supplies $supplied_count"
        continue
      fi
    elif [ "$optional_positional_count" -eq 0 ]; then
      if [ "$supplied_count" -ne "$required_count" ]; then
        fail "$id: herdr $group $sub requires exactly $required_count positional argument(s), commands.json supplies $supplied_count"
        continue
      fi
    else
      # A non-variadic "[NAME]" raises the max allowed count by exactly one
      # per occurrence, unlike "..." which is unbounded (handled above).
      max_count=$((required_count + optional_positional_count))
      if [ "$supplied_count" -lt "$required_count" ] || [ "$supplied_count" -gt "$max_count" ]; then
        fail "$id: herdr $group $sub accepts between $required_count and $max_count positional argument(s), commands.json supplies $supplied_count"
        continue
      fi
    fi

    i=0
    while [ "$i" -lt "$required_count" ]; do
      required_name="${required_positional_names[$i]}"
      supplied_desc="${supplied_positionals[$i]}"
      if [ "$supplied_desc" != "any" ] && [ "$supplied_desc" != "$required_name" ]; then
        fail "$id: positional #$((i + 1)) is $supplied_desc but herdr $group $sub expects <$required_name>"
      fi
      i=$((i + 1))
    done

    i=0
    required_option_count=${#required_option_flags[@]}
    while [ "$i" -lt "$required_option_count" ]; do
      required_flag="${required_option_flags[$i]}"
      has_flag=$(jq -r --arg f "$required_flag" '
        [.arguments[]? | select(.source == "literal" and .value == $f)] | length
      ' <<<"$cmd_json")
      if [ "$has_flag" -eq 0 ]; then
        fail "$id: herdr $group $sub requires option $required_flag but commands.json does not supply it"
      fi
      i=$((i + 1))
    done
  done <<EOF3
$ids_for_pair
EOF3
done <<EOF
$pairs
EOF

# --- Check 6: catalog default keybindings vs `herdr --default-config` ---
# Every "key" in commands.json claims to be herdr's default binding for its
# "keys_action" field. The template prints those defaults as commented-out
# lines (`# next_tab = "prefix+n"`). Fields the template does not list
# (e.g. swap_pane_*, an omission in the template itself) cannot be checked
# and are skipped; a herdr without --default-config skips the whole check.

default_config=$("$herdr_bin" --default-config 2>/dev/null)
if [ -z "$default_config" ]; then
  echo "NOTE: herdr --default-config prints nothing; skipping default keybinding check"
else
  default_keys=$(printf '%s\n' "$default_config" \
    | sed -n 's/^# *\([a-z_][a-z_]*\) = "\([^"]*\)".*$/\1 \2/p')
  tab_char=$(printf '\t')
  while IFS="$tab_char" read -r id keys_action key; do
    [ -n "$id" ] || continue
    template_line=$(printf '%s\n' "$default_keys" | grep "^$keys_action " | head -n 1)
    if [ -z "$template_line" ]; then
      continue
    fi
    template_key="${template_line#* }"
    if [ "$template_key" != "$key" ]; then
      fail "$id: catalog says the default for $keys_action is '$key' but herdr --default-config says '$template_key'"
    fi
  done <<EOF
$(jq -r '.commands[] | select(.key != null) | [.id, .keys_action, .key] | @tsv' "$catalog")
EOF
fi

# --- Result ---

if [ "$failed" -ne 0 ]; then
  exit 1
fi

herdr_version=$("$herdr_bin" --version 2>/dev/null | awk '{ print $2 }')
echo "OK: herdr $herdr_version is compatible (protocol $actual_protocol)"
