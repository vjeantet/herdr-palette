#!/usr/bin/env bash
# Action `vjeantet.palette.open` entrypoint: reads commands.json, lets the
# user pick a command via fzf, resolves its `arguments` into a plain argv array,
# and runs the herdr CLI. Runs inside the popup pane opened by open.sh, so a
# real TTY is available for fzf.
#
# The error-display pattern (print the message via die(), then wait for a
# keypress before exiting) is adapted from Jan Tvrdík's jt.command-palette
# (MIT), https://github.com/JanTvrdik/herdr-command-palette.
#
# Bash 3.2 compatible (macOS stock bash): no mapfile, no associative arrays,
# no `${var,,}`.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"

# die prints an error, waits for a keypress so the popup doesn't vanish
# before the user can read it, then exits non-zero.
die() {
  printf '%s\n' "$1" >&2
  printf '\n(press any key to close)\n' >&2
  read -r -n 1 -s _ 2>/dev/null || true
  exit 1
}

# require_clean_fzf_rc RC — classifies fzf's exit status for the fzf calls
# that pick from a real candidate list (main picker, select, confirm; input
# has its own handling since --print-query makes "no match" a normal
# outcome there, not a cancel). 0 means the caller should proceed; 1 (no
# match) and 130 (interrupted, i.e. Esc/Ctrl-C) are clean cancels and exit
# the whole script with status 0; anything else (2: fzf usage/runtime
# error) is a genuine failure and must not be silently treated as a cancel.
require_clean_fzf_rc() {
  case "$1" in
    0) return 0 ;;
    1|130) exit 0 ;;
    *) die "command-palette: fzf exited with an unexpected status ($1)" ;;
  esac
}

# resolve_context maps a static context key to the ORIGIN_* value captured by open.sh.
# Prints the value and returns 0, or returns 1 for an unknown key.
resolve_context() {
  case "$1" in
    pane_id) printf '%s' "$ORIGIN_PANE_ID" ;;
    tab_id) printf '%s' "$ORIGIN_TAB_ID" ;;
    workspace_id) printf '%s' "$ORIGIN_WORKSPACE_ID" ;;
    cwd) printf '%s' "$ORIGIN_CWD" ;;
    *) return 1 ;;
  esac
}

# resolve_computed_context derives a neighboring resource ID from the ordered
# list returned by herdr and stores it in the global $value. It runs in the
# main shell so die() terminates the palette rather than a subshell.
resolve_computed_context() {
  computed_key="$1"

  case "$computed_key" in
    next_workspace_id)
      direction="next"
      list_desc="workspace list"
      collection="workspaces"
      id_field="workspace_id"
      origin_id="$ORIGIN_WORKSPACE_ID"
      raw=$("$herdr_bin" workspace list 2>&1)
      rc=$?
      ;;
    previous_workspace_id)
      direction="previous"
      list_desc="workspace list"
      collection="workspaces"
      id_field="workspace_id"
      origin_id="$ORIGIN_WORKSPACE_ID"
      raw=$("$herdr_bin" workspace list 2>&1)
      rc=$?
      ;;
    next_tab_id)
      direction="next"
      list_desc="tab list"
      collection="tabs"
      id_field="tab_id"
      origin_id="$ORIGIN_TAB_ID"
      raw=$("$herdr_bin" tab list --workspace "$ORIGIN_WORKSPACE_ID" 2>&1)
      rc=$?
      ;;
    previous_tab_id)
      direction="previous"
      list_desc="tab list"
      collection="tabs"
      id_field="tab_id"
      origin_id="$ORIGIN_TAB_ID"
      raw=$("$herdr_bin" tab list --workspace "$ORIGIN_WORKSPACE_ID" 2>&1)
      rc=$?
      ;;
    *)
      return 1
      ;;
  esac

  if [ "$rc" -ne 0 ]; then
    die "command-palette: herdr $list_desc failed:"$'\n'"$raw"
  fi
  if ! printf '%s' "$raw" | jq -e --arg collection "$collection" \
    '.result[$collection] | type == "array"' >/dev/null 2>&1; then
    die "command-palette: herdr $list_desc returned an unexpected shape"
  fi
  if printf '%s' "$raw" | jq -e --arg collection "$collection" --arg field "$id_field" '
    def has_invalid_id($field):
      if type != "object" then true
      else .[$field] as $id
      | if ($id | type) != "string" then true
        else ($id | explode) as $cp
        | $id == "" or ($cp | index(0) != null) or ($cp | index(10) != null)
        end
      end;
    [.result[$collection][] | select(has_invalid_id($field))] | length > 0
  ' >/dev/null 2>&1; then
    die "command-palette: herdr $list_desc returned a candidate without a valid $id_field"
  fi

  if ! value=$(printf '%s' "$raw" | jq -er \
    --arg collection "$collection" \
    --arg field "$id_field" \
    --arg origin "$origin_id" \
    --arg direction "$direction" '
      [.result[$collection][][ $field ]] as $ids
      | ($ids | index($origin)) as $index
      | if $index == null then empty
        elif $direction == "next" then $ids[(($index + 1) % ($ids | length))]
        else $ids[(($index + ($ids | length) - 1) % ($ids | length))]
        end
    '); then
    die "command-palette: herdr $list_desc did not include the origin $id_field"
  fi
}

# fetch_workspace_list_for_labels runs `herdr workspace list` and validates
# its shape, storing the raw JSON in the global $ws_list_raw. Used by the
# tabs and agents selectors to resolve workspace_id -> label for prefixing
# cross-workspace candidates (they only carry workspace_id themselves). Not a
# function returning via command substitution: die() must run in the main
# shell, not a subshell, or `exit` inside it would only end the subshell.
fetch_workspace_list_for_labels() {
  if ! ws_list_raw=$("$herdr_bin" workspace list 2>&1); then
    die "command-palette: herdr workspace list failed:"$'\n'"$ws_list_raw"
  fi
  if ! printf '%s' "$ws_list_raw" | jq -e '.result.workspaces | type == "array"' >/dev/null 2>&1; then
    die "command-palette: herdr workspace list returned an unexpected shape"
  fi
}

# run_plugin_action QUALIFIED_ID — invoke one plugin action and wait for the
# dispatched run to reach a terminal state.
#
# `plugin action invoke` is fire-and-forget: a zero exit means "accepted", not
# "succeeded". An action whose command dies afterwards (a moved script exiting
# 127, say) would otherwise vanish silently along with the popup. Polling that
# run's own log entry surfaces the failure instead.
#
# Adapted from Jan Tvrdik's jt.command-palette (MIT),
# https://github.com/JanTvrdik/herdr-command-palette.
run_plugin_action() {
  action_id="$1"

  if ! resp=$("$herdr_bin" plugin action invoke "$action_id" 2>&1); then
    die "command-palette: failed to invoke $action_id"$'\n'"$resp"
  fi

  # Read plugin_id back from the response rather than splitting it off the
  # action id: a plugin id carries dots of its own (jt.command-palette), so the
  # split is ambiguous. An older herdr that reports no log is left alone — a
  # working invoke must never be made to look broken.
  log_id=$(printf '%s' "$resp" | jq -r '.result.log.log_id // empty' 2>/dev/null)
  plugin_id=$(printf '%s' "$resp" | jq -r '.result.log.plugin_id // empty' 2>/dev/null)
  if [ -z "$log_id" ] || [ -z "$plugin_id" ]; then
    return 0
  fi

  i=0
  while [ "$i" -lt 25 ]; do # ~5s at 0.2s per turn
    i=$((i + 1))
    entry=$("$herdr_bin" plugin log list --plugin "$plugin_id" --limit 20 2>/dev/null \
      | jq -c --arg id "$log_id" '.result.logs[]? | select(.log_id == $id)' 2>/dev/null)
    case "$(printf '%s' "$entry" | jq -r '.status // empty' 2>/dev/null)" in
      succeeded) return 0 ;;
      failed)
        code=$(printf '%s' "$entry" | jq -r '.exit_code // "?"' 2>/dev/null)
        err=$(printf '%s' "$entry" | jq -r '.stderr // empty' 2>/dev/null)
        die "command-palette: $action_id failed (exit $code)"$'\n'"$err"
        ;;
    esac
    sleep 0.2
  done

  # Still running at the deadline: assume a healthy long-running action.
  return 0
}

# neutralize_fzf_output_options rewrites FZF_DEFAULT_OPTS in place, dropping
# only the options that change the SHAPE of what fzf prints, and folding
# FZF_DEFAULT_OPTS_FILE (fzf >= 0.48, a second source of the same options)
# into it beforehand.
#
# Inheriting the user's fzf options is deliberate and worth keeping: theme,
# colors, height, layout and key bindings are what make the palette look like
# the rest of their tools. But this script reads fzf's output as "one line,
# id in field 1", and a few options break that silently:
#
#   --print-query   prepends the query, so the id read from line 1 is the query
#   --expect=KEY    prepends the pressed key, same damage
#   --print0        separates results with NUL instead of newline
#   --read0         reads the candidate list the same way
#   --filter=STR    makes fzf non-interactive: no picker is ever shown, and
#                   the matches are printed instead
#
# --multi/-m is handled at the call sites instead: fzf accepts --no-multi
# (verified against fzf 0.38.0, where it is absent from --help but honored),
# and a command-line option beats both environment sources.
neutralize_fzf_output_options() {
  fzf_opts_raw=""
  if [ -n "${FZF_DEFAULT_OPTS_FILE:-}" ] && [ -r "${FZF_DEFAULT_OPTS_FILE}" ]; then
    # Whole-line comments go; a '#' inside a token does not start one (it
    # opens every hex color), so nothing else is stripped.
    fzf_opts_raw=$(grep -v '^[[:space:]]*#' "${FZF_DEFAULT_OPTS_FILE}" 2>/dev/null || true)
  fi
  # The file is applied before FZF_DEFAULT_OPTS by fzf, so it goes first here.
  fzf_opts_raw="$fzf_opts_raw ${FZF_DEFAULT_OPTS:-}"

  # Deliberate word splitting: this reproduces fzf's own tokenization. `set -f`
  # keeps a token such as '*' inside a binding from being expanded as a glob.
  set -f
  # shellcheck disable=SC2206
  fzf_opt_words=($fzf_opts_raw)
  set +f

  fzf_opts_kept=""
  fzf_skip_value=0
  if [ "${#fzf_opt_words[@]}" -gt 0 ]; then
    for word in "${fzf_opt_words[@]}"; do
      if [ "$fzf_skip_value" -eq 1 ]; then
        fzf_skip_value=0
        continue
      fi
      case "$word" in
        --print-query|--print0|--read0) continue ;;
        --expect|--filter|-f) fzf_skip_value=1; continue ;;
        --expect=*|--filter=*|-f?*) continue ;;
      esac
      fzf_opts_kept="${fzf_opts_kept:+$fzf_opts_kept }$word"
    done
  fi

  FZF_DEFAULT_OPTS="$fzf_opts_kept"
  export FZF_DEFAULT_OPTS
  unset FZF_DEFAULT_OPTS_FILE
}

# 1. Check fzf and jq are available.
for bin in fzf jq; do
  command -v "$bin" >/dev/null 2>&1 || die "command-palette: $bin is not installed"
done

# 1b. Keep the user's fzf styling, refuse what would change fzf's output shape.
neutralize_fzf_output_options

# 2. Check the origin context captured by open.sh.
if [ -z "${ORIGIN_PANE_ID:-}" ] || [ -z "${ORIGIN_TAB_ID:-}" ] || [ -z "${ORIGIN_WORKSPACE_ID:-}" ]; then
  die "command-palette: missing origin context (ORIGIN_PANE_ID/ORIGIN_TAB_ID/ORIGIN_WORKSPACE_ID)"
fi

# 3. Read the catalog. Die on missing/unreadable/invalid JSON; no schema
# validation at runtime (done in CI, see scripts/check-compat.sh).
if [ -z "${HERDR_PLUGIN_ROOT:-}" ]; then
  die "command-palette: HERDR_PLUGIN_ROOT is not set"
fi
catalog_path="$HERDR_PLUGIN_ROOT/commands.json"
if [ ! -r "$catalog_path" ]; then
  die "command-palette: cannot read catalog: $catalog_path"
fi
#
# Everything the open path needs from the catalog comes out of this single jq
# invocation: line 1 is the metadata record (schema_version, then
# expected_herdr_protocol, tab-separated), and every line after it is one
# picker row, in catalog order. jq costs ~100 ms to start on a slow host, so
# the four separate reads this replaces (validate, schema_version,
# expected_herdr_protocol, rows) cost four startups on every single open —
# measured at 403 ms against 123 ms here (jq 1.6, armhf, mean of 3).
#
# Invalid JSON still fails first and loudest: jq exits nonzero and its own
# parser message is captured through 2>&1, exactly as the separate `jq empty`
# used to report it.
if ! catalog_read=$(jq -r '
      "\(.schema_version)\t\(.expected_herdr_protocol)",
      (.commands[] | "\(.id)\t\(.title)")
    ' "$catalog_path" 2>&1); then
  die "command-palette: commands.json is not valid JSON: $catalog_read"
fi
catalog_meta=$(printf '%s\n' "$catalog_read" | sed -n '1p')
catalog_rows=$(printf '%s\n' "$catalog_read" | sed -n '2,$p')

# 3b. This plugin defines exactly one catalog format, schema_version 1 (see
# docs/design/command-catalog.md); a catalog claiming a different version
# was not written for this palette.sh. A catalog carrying no schema_version
# at all interpolates as the string "null" above and is rejected here too.
schema_version=$(printf '%s' "$catalog_meta" | cut -f1)
if [ "$schema_version" != "1" ]; then
  die "command-palette: unsupported commands.json schema_version: $schema_version (expected 1)"
fi

# 4-5. Compare the herdr socket API protocol against expected_herdr_protocol,
# read above with the rest of the catalog. Neither an unreadable protocol nor
# a mismatch blocks execution; both are shown as a header warning only.
expected_protocol=$(printf '%s' "$catalog_meta" | cut -f2)
schema_output=$("$herdr_bin" api schema 2>&1)
actual_protocol=$(printf '%s\n' "$schema_output" | sed -n 's/^protocol: *//p' | head -n 1)
main_header="herdr command palette"
if [ -z "$actual_protocol" ]; then
  main_header="warning: could not read protocol from herdr api schema"
elif [ "$actual_protocol" != "$expected_protocol" ]; then
  main_header="warning: catalog expects herdr protocol $expected_protocol, herdr reports $actual_protocol"
fi

# 6a. Collect the other half of the palette: every action exposed by every
# OTHER installed plugin. herdr's built-in operations come from the catalog;
# these come from `plugin action list`. Rows are keyed "plugin:<qualified_id>",
# which no catalog id can collide with — commands.schema.json restricts ids to
# ^[a-z0-9._-]+$, and that excludes the colon.
#
# A failure here is deliberately not fatal: the built-in half must stay usable
# on a herdr too old to list plugin actions, or when none are installed.
#
# That is also why this jq call stays separate from the catalog read above,
# rather than being folded into it with --argjson/--slurpfile to save one more
# jq startup: a single merged invocation fails as a whole, so one malformed
# plugin entry — or a `plugin action list` that errors out — would take the
# built-in half down with it. The additive guarantee is worth ~100 ms.
self_plugin="${HERDR_PLUGIN_ID:-}"
case "$(uname -s)" in
  Darwin) host_platform="macos" ;;
  Linux) host_platform="linux" ;;
  *) host_platform="" ;;
esac

# Actions are filtered to this host's platform. Without it a Linux user is
# offered the powershell twins that several plugins declare for Windows, which
# can only fail. An action that declares no platforms at all runs anywhere.
plugin_rows=$(
  "$herdr_bin" plugin action list 2>/dev/null \
    | jq -r --arg self "$self_plugin" --arg platform "$host_platform" '
        .result.actions[]?
        | select($self == "" or .plugin_id != $self)
        | select(
            $platform == ""
            or (.platforms == null)
            or ((.platforms | index($platform)) != null)
          )
        | (.plugin_id + "." + .action_id) as $qid
        | [("plugin:" + $qid), ("Plugin: " + .title + "  " + $qid)]
        | @tsv
      ' 2>/dev/null \
    | sort -t"$(printf '\t')" -k2,2
)

# 6. Show the main list: id is a hidden key, title is displayed, catalog order
# preserved. Plugin rows follow the catalog, sorted among themselves; their
# qualified id rides in the display field so typing a plugin name finds them
# (fzf matches on what --with-nth shows).
selected_line=$(
  {
    if [ -n "$catalog_rows" ]; then
      printf '%s\n' "$catalog_rows"
    fi
    if [ -n "$plugin_rows" ]; then
      printf '%s\n' "$plugin_rows"
    fi
  } | fzf --no-multi --delimiter=$'\t' --with-nth=2 --header="$main_header" --prompt="herdr > "
)
rc=$?
require_clean_fzf_rc "$rc"
selected_id=$(printf '%s' "$selected_line" | cut -f1)

# 6b. A plugin row dispatches straight to herdr's plugin runner. None of the
# catalog machinery below (arguments, confirmation, argv assembly) applies to
# it: a plugin action takes no arguments from us.
case "$selected_id" in
  plugin:*)
    run_plugin_action "${selected_id#plugin:}"
    exit 0
    ;;
esac

# 6c. Read the selected command in one jq call, in the shape the rest of this
# script consumes. Six separate reads of the same object (description,
# command[0], command[1], argument count, confirm) plus two more per argument
# each paid for their own jq startup; this pays for one.
#
# The record is line-oriented because a compact JSON object never contains a
# raw newline or tab (jq escapes both), so one argument definition per line is
# unambiguous. The argument count leads so that a command whose every text
# field is empty still yields a non-empty record — "no output" then means, as
# before, "no such id in the catalog".
cmd_record=$(jq -r --arg id "$selected_id" '
  .commands[]
  | select(.id == $id)
  | (.arguments | length),
    .description,
    .command[0],
    .command[1],
    (.confirm // ""),
    (.arguments[]? | "\(.source)\t\(tojson)")
' "$catalog_path")
if [ -z "$cmd_record" ]; then
  die "command-palette: internal error: selected command not found in catalog"
fi
cmd_fields=()
while IFS= read -r cmd_field; do
  cmd_fields+=("$cmd_field")
done <<<"$cmd_record"
argc="${cmd_fields[0]}"
cmd_description="${cmd_fields[1]}"
group="${cmd_fields[2]}"
subcommand="${cmd_fields[3]}"
confirm_text="${cmd_fields[4]}"
cmd_fields_header=5

# 7. Resolve `arguments` left to right into a plain bash array. Each resolved
# value becomes exactly one argv element; nothing is re-parsed by the shell.
# The definitions were hoisted out of this loop above, in catalog order, which
# is the order they are consumed in and is significant.
argv=("$group" "$subcommand")
tab=$'\t'
i=0
while [ "$i" -lt "$argc" ]; do
  arg_line="${cmd_fields[$((cmd_fields_header + i))]}"
  source="${arg_line%%"$tab"*}"
  arg_def="${arg_line#*"$tab"}"

  case "$source" in
    literal)
      value=$(jq -r '.value' <<<"$arg_def")
      argv+=("$value")
      ;;

    context)
      key=$(jq -r '.key' <<<"$arg_def")
      case "$key" in
        next_workspace_id|previous_workspace_id|next_tab_id|previous_tab_id)
          resolve_computed_context "$key"
          ;;
        *)
          if ! value=$(resolve_context "$key"); then
            die "command-palette: unexpected context key: $key"
          fi
          ;;
      esac
      if [ "$key" = "cwd" ]; then
        if [ -z "$value" ] || [ ! -d "$value" ]; then
          die "command-palette: origin working directory is unavailable or missing"
        fi
      fi
      argv+=("$value")
      ;;

    input)
      prompt=$(jq -r '.prompt' <<<"$arg_def")
      input_description=$(jq -r '.description // empty' <<<"$arg_def")
      if [ -z "$input_description" ]; then
        input_description="$cmd_description"
      fi
      required=$(jq -r '.required' <<<"$arg_def")
      default_context=$(jq -r '.default_context // empty' <<<"$arg_def")
      validation=$(jq -r '.validation // empty' <<<"$arg_def")

      initial_query=""
      if [ -n "$default_context" ]; then
        if ! initial_query=$(resolve_context "$default_context"); then
          die "command-palette: unexpected default_context key: $default_context"
        fi
      fi

      query_output=$(printf '' | fzf --no-multi --print-query --query="$initial_query" \
        --header="$input_description" --prompt="$prompt")
      rc=$?
      # --print-query always prints the query as its first line, even on a
      # "no match" exit (1, the normal outcome here since there are no real
      # candidates to match against) — so unlike the other fzf call sites,
      # 1 is not a cancel. Only 130 (Esc/Ctrl-C) is; 2 is a genuine fzf
      # error and must not be treated as if the user simply typed nothing.
      if [ $rc -eq 130 ]; then
        exit 0
      fi
      if [ $rc -ne 0 ] && [ $rc -ne 1 ]; then
        die "command-palette: fzf exited with an unexpected status ($rc)"
      fi
      value=$(printf '%s\n' "$query_output" | sed -n '1p')

      if [ "$required" = "true" ] && [ -z "$value" ]; then
        exit 0
      fi
      if [ "$validation" = "directory" ] && [ -n "$value" ] && [ ! -d "$value" ]; then
        die "command-palette: input is not an existing directory"
      fi
      argv+=("$value")
      ;;

    select)
      selector=$(jq -r '.selector' <<<"$arg_def")
      select_prompt=$(jq -r '.prompt' <<<"$arg_def")
      select_description=$(jq -r '.description // empty' <<<"$arg_def")
      if [ -z "$select_description" ]; then
        select_description="$cmd_description"
      fi
      exclude_key=$(jq -r '.exclude_context // empty' <<<"$arg_def")
      exclude_value=""
      if [ -n "$exclude_key" ]; then
        if ! exclude_value=$(resolve_context "$exclude_key"); then
          die "command-palette: unexpected exclude_context key: $exclude_key"
        fi
      fi

      # Named selectors are mapped to their herdr list command and jq shape
      # here; commands.json never carries a list command or jq filter itself.
      case "$selector" in
        workspaces)
          list_desc="workspace list"
          raw=$("$herdr_bin" workspace list 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            die "command-palette: herdr $list_desc failed:"$'\n'"$raw"
          fi
          if ! printf '%s' "$raw" | jq -e '.result.workspaces | type == "array"' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned an unexpected shape"
          fi
          if printf '%s' "$raw" | jq -e '[.result.workspaces[] | select(.workspace_id == null or .workspace_id == "")] | length > 0' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned a candidate without workspace_id"
          fi
          # Sanitize label control characters (newline/tab/CR): a label is
          # herdr-supplied, not catalog-controlled, and gets embedded raw
          # into this tab-delimited candidate row; left unsanitized, a
          # label containing \n or \t could forge extra rows or shift which
          # field fzf treats as the id.
          if ! candidates=$(printf '%s' "$raw" | jq -r --arg excl "$exclude_value" '
            .result.workspaces[]
            | select($excl == "" or .workspace_id != $excl)
            | ((.label // "") | gsub("[\\n\\r\\t]"; " ")) as $label
            | "\(.workspace_id)\t\($label) (\(.workspace_id))"
          '); then
            die "command-palette: failed to build $list_desc candidates"
          fi
          ;;
        tabs)
          list_desc="tab list"
          raw=$("$herdr_bin" tab list 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            die "command-palette: herdr $list_desc failed:"$'\n'"$raw"
          fi
          if ! printf '%s' "$raw" | jq -e '.result.tabs | type == "array"' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned an unexpected shape"
          fi
          if printf '%s' "$raw" | jq -e '[.result.tabs[] | select(.tab_id == null or .tab_id == "")] | length > 0' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned a candidate without tab_id"
          fi

          # tab list spans all workspaces but only carries workspace_id, so
          # resolve workspace_id -> label to prefix each candidate. Labels
          # are sanitized here (see the "workspaces" case above for why).
          fetch_workspace_list_for_labels
          if ! ws_labels=$(printf '%s' "$ws_list_raw" | jq -c '
            [.result.workspaces[] | select(.workspace_id != null) | {(.workspace_id): ((.label // "") | gsub("[\\n\\r\\t]"; " "))}] | add // {}
          '); then
            die "command-palette: failed to build workspace label lookup"
          fi

          if ! candidates=$(printf '%s' "$raw" | jq -r --arg excl "$exclude_value" --argjson ws "$ws_labels" '
            .result.tabs[]
            | select($excl == "" or .tab_id != $excl)
            | ($ws[.workspace_id] // .workspace_id) as $ws_label
            | ((.label // "") | gsub("[\\n\\r\\t]"; " ")) as $label
            | "\(.tab_id)\t\($ws_label) / \($label) (\(.tab_id))"
          '); then
            die "command-palette: failed to build $list_desc candidates"
          fi
          ;;
        agents)
          list_desc="agent list"
          raw=$("$herdr_bin" agent list 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            die "command-palette: herdr $list_desc failed:"$'\n'"$raw"
          fi
          if ! printf '%s' "$raw" | jq -e '.result.agents | type == "array"' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned an unexpected shape"
          fi
          if printf '%s' "$raw" | jq -e '[.result.agents[] | select(.pane_id == null or .pane_id == "")] | length > 0' >/dev/null 2>&1; then
            die "command-palette: herdr $list_desc returned a candidate without pane_id"
          fi

          # agent list spans all workspaces but only carries workspace_id, so
          # resolve workspace_id -> label to prefix each candidate. Labels
          # are sanitized here (see the "workspaces" case above for why).
          fetch_workspace_list_for_labels
          if ! ws_labels=$(printf '%s' "$ws_list_raw" | jq -c '
            [.result.workspaces[] | select(.workspace_id != null) | {(.workspace_id): ((.label // "") | gsub("[\\n\\r\\t]"; " "))}] | add // {}
          '); then
            die "command-palette: failed to build workspace label lookup"
          fi

          if ! candidates=$(printf '%s' "$raw" | jq -r --arg excl "$exclude_value" --argjson ws "$ws_labels" '
            .result.agents[]
            | select($excl == "" or .pane_id != $excl)
            | ($ws[.workspace_id] // .workspace_id) as $ws_label
            | ((.terminal_title_stripped // "") | gsub("[\\n\\r\\t]"; " ")) as $title
            | "\(.pane_id)\t\($ws_label) / agent: \($title) (\(.pane_id))"
          '); then
            die "command-palette: failed to build $list_desc candidates"
          fi
          ;;
        *)
          die "command-palette: unexpected selector: $selector"
          ;;
      esac

      if [ -z "$candidates" ]; then
        exit 0
      fi

      selected=$(printf '%s\n' "$candidates" \
        | fzf --no-multi --delimiter=$'\t' --with-nth=2 --header="$select_description" --prompt="$select_prompt")
      rc=$?
      require_clean_fzf_rc "$rc"
      selected_id=$(printf '%s' "$selected" | cut -f1)
      if [ -z "$selected_id" ]; then
        die "command-palette: internal error: selected candidate has no id"
      fi
      argv+=("$selected_id")
      ;;

    *)
      die "command-palette: unexpected argument source: $source"
      ;;
  esac

  i=$((i + 1))
done

# 8. Confirm, if the command asks for it ($confirm_text came out of the
# single catalog read in 6c). No is listed first so fzf's cursor starts on it:
# a reflexive Enter cancels instead of confirming. No and Esc are clean
# cancels.
if [ -n "$confirm_text" ]; then
  choice=$(printf 'No\nYes\n' | fzf --no-multi --header="$confirm_text" --prompt="confirm > ")
  rc=$?
  require_clean_fzf_rc "$rc"
  if [ "$choice" != "Yes" ]; then
    exit 0
  fi
fi

# 9. Run the herdr CLI. Every command runs synchronously: under popup
# placement, a synchronous focus change survives the popup closing (herdr
# 0.8.0, measured 2026-08-16), so no deferred/post-close handling is needed.
# On failure, show group+subcommand+output only; never echo free-input
# values, selected ids, or the working directory.
output=$("$herdr_bin" "${argv[@]}" 2>&1)
rc=$?
if [ $rc -ne 0 ]; then
  die "command-palette: herdr $group $subcommand failed:"$'\n'"$output"
fi

# 10. Success: exit and let the popup close.
exit 0
