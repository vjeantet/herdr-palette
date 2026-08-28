#!/usr/bin/env bats
# scripts/fetch-or-build.sh — the install-time build step. Every path through
# it is exercised here, none of them over the network: the release is a local
# directory served through curl's file:// scheme, and uname/getconf/cargo are
# stubbed so one machine can stand in for every platform the script maps.
#
# The cargo stub matters twice over: it records that a fallback happened, and
# it keeps a fallback test from actually compiling this crate. HOME is moved
# aside for the same reason — build_from_source sources ~/.cargo/env, which
# would otherwise put the real cargo ahead of the stub on PATH.

setup() {
  load test_helper
  ROOT="$(repo_root)"
  SCRIPT="$ROOT/scripts/fetch-or-build.sh"
  REPO="$BATS_TEST_TMPDIR/repo"
  RELEASE="$BATS_TEST_TMPDIR/releases/v9.9.9"
  STUBS="$BATS_TEST_TMPDIR/stubs"
  CARGO_LOG="$BATS_TEST_TMPDIR/cargo-calls"
  mkdir -p "$REPO" "$RELEASE" "$STUBS" "$BATS_TEST_TMPDIR/home"

  # A version no real release carries, so a mistake here can never reach the
  # actual GitHub releases of this repository.
  printf '[package]\nname = "herdr-palette"\nversion = "9.9.9"\n' >"$REPO/Cargo.toml"

  export HOME="$BATS_TEST_TMPDIR/home"
  export PALETTE_REPO_ROOT="$REPO"
  export PALETTE_CARGO_TOML="$REPO/Cargo.toml"
  export PALETTE_OUT="$REPO/target/release/herdr-palette"
  export PALETTE_BASE_URL="file://$BATS_TEST_TMPDIR/releases"
  export CARGO_LOG
  export STUB_OS=Linux
  export STUB_ARCH=x86_64
  export STUB_LONG_BIT=64

  write_stubs
  PATH="$STUBS:$PATH"
}

write_stubs() {
  cat >"$STUBS/uname" <<'STUB'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' "$STUB_OS" ;;
  -m) printf '%s\n' "$STUB_ARCH" ;;
  *)  printf '%s\n' "$STUB_OS" ;;
esac
STUB
  cat >"$STUBS/getconf" <<'STUB'
#!/bin/sh
[ "$1" = "LONG_BIT" ] && printf '%s\n' "$STUB_LONG_BIT"
STUB
  cat >"$STUBS/cargo" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" >>"$CARGO_LOG"
STUB
  chmod +x "$STUBS/uname" "$STUBS/getconf" "$STUBS/cargo"
}

# sha256_line FILE — the one line the release job publishes, in whichever of
# the two tools the host has.
sha256_line() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  else
    shasum -a 256 "$1"
  fi
}

# publish TRIPLE [EXIT_CODE] — a released asset plus its checksum file. The
# asset is a script that exits with EXIT_CODE (default 2, what the real binary
# does when it is run with no subcommand), which is what the script's exec
# check reads.
publish() {
  local triple="$1" code="${2:-2}" asset
  asset="herdr-palette-$triple"
  printf '#!/bin/sh\nexit %s\n' "$code" >"$RELEASE/$asset"
  chmod +x "$RELEASE/$asset"
  (cd "$RELEASE" && sha256_line "$asset" >"$asset.sha256")
}

cargo_was_called() {
  [ -s "$CARGO_LOG" ]
}

@test "installs the prebuilt binary when the checksum matches" {
  publish x86_64-unknown-linux-musl
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ -x "$PALETTE_OUT" ]
  ! cargo_was_called
  [[ "$output" == *"installed prebuilt v9.9.9 (x86_64-unknown-linux-musl)"* ]]
}

@test "falls back to a source build when no prebuilt is published for the version" {
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_OUT" ]
  cargo_was_called
  [ "$(cat "$CARGO_LOG")" = "build --release" ]
}

@test "falls back to a source build when the checksum does not match" {
  publish x86_64-unknown-linux-musl
  printf '%s  herdr-palette-x86_64-unknown-linux-musl\n' \
    "0000000000000000000000000000000000000000000000000000000000000000" \
    >"$RELEASE/herdr-palette-x86_64-unknown-linux-musl.sha256"
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_OUT" ]
  cargo_was_called
  [[ "$output" == *"checksum mismatch"* ]]
}

@test "falls back to a source build when the checksum file names another asset" {
  publish x86_64-unknown-linux-musl
  (cd "$RELEASE" && sha256_line herdr-palette-x86_64-unknown-linux-musl \
    | sed 's/herdr-palette-x86_64-unknown-linux-musl/herdr-palette-somewhere-else/' \
    >herdr-palette-x86_64-unknown-linux-musl.sha256)
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_OUT" ]
  cargo_was_called
  [[ "$output" == *"no checksum listed"* ]]
}

@test "falls back to a source build on a platform with no prebuilt" {
  STUB_ARCH=riscv64
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  cargo_was_called
  [[ "$output" == *"no prebuilt binary for Linux/riscv64"* ]]
}

@test "falls back to a source build when the prebuilt cannot run here" {
  publish x86_64-unknown-linux-musl 126
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ ! -e "$PALETTE_OUT" ]
  cargo_was_called
  [[ "$output" == *"does not run on this machine"* ]]
}

@test "maps a 32-bit userland on a 64-bit kernel to the armv7 target" {
  STUB_ARCH=aarch64
  STUB_LONG_BIT=32
  publish armv7-unknown-linux-musleabihf
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ -x "$PALETTE_OUT" ]
  ! cargo_was_called
  [[ "$output" == *"(armv7-unknown-linux-musleabihf)"* ]]
}

@test "maps a 64-bit arm userland to the aarch64 target" {
  STUB_ARCH=aarch64
  publish aarch64-unknown-linux-musl
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ -x "$PALETTE_OUT" ]
  [[ "$output" == *"(aarch64-unknown-linux-musl)"* ]]
}

@test "maps an apple silicon station to the aarch64-apple-darwin target" {
  STUB_OS=Darwin
  STUB_ARCH=arm64
  publish aarch64-apple-darwin
  run sh "$SCRIPT"
  [ "$status" -eq 0 ]
  [ -x "$PALETTE_OUT" ]
  [[ "$output" == *"(aarch64-apple-darwin)"* ]]
}
