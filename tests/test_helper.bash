# Shared bats helpers for this repo's test suite.
#
# repo_root resolves the repository root from this file's own location, so
# tests work regardless of the working directory bats was invoked from.
repo_root() {
  cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
}

# setup_fixture_repo CATALOG_JSON — copies scripts/check-compat.sh and
# commands.schema.json into a fresh temp dir, alongside the given catalog
# fixture renamed to commands.json, and prints the temp dir's path.
#
# check-compat.sh resolves commands.json relative to its own script_dir
# (repo_root/commands.json), so exercising it against a doctored catalog
# means giving it a whole doctored tree, not just a different file argument.
# Callers must `rm -rf` the returned directory in their own teardown.
setup_fixture_repo() {
  local catalog="$1"
  local root
  root="$(repo_root)"
  local tmp
  tmp=$(mktemp -d)
  mkdir -p "$tmp/scripts"
  cp "$root/scripts/check-compat.sh" "$tmp/scripts/check-compat.sh"
  cp "$root/commands.schema.json" "$tmp/commands.schema.json"
  cp "$catalog" "$tmp/commands.json"
  printf '%s' "$tmp"
}

# herdr_stub — absolute path to the fake herdr CLI used by check-compat.bats.
herdr_stub() {
  printf '%s/tests/stubs/herdr' "$(repo_root)"
}

# palette_bin — absolute path to the Rust palette binary under test. Built by
# `cargo build --release` (CI does this before running bats); override with
# PALETTE_BIN to test another build.
palette_bin() {
  printf '%s' "${PALETTE_BIN:-$(repo_root)/target/release/herdr-palette}"
}
