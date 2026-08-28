#!/bin/sh
# fetch-or-build.sh - the [[build]] step herdr runs after cloning this plugin.
#
# Fast path: download the prebuilt binary matching this checkout's declared
# version and this machine's platform, verify its SHA-256, and install it at
# target/release/herdr-palette - the path every manifest entrypoint points at.
#
# Fallback: on ANY miss (no release for this version, no prebuilt for this
# platform, download or checksum failure, a binary that will not run here)
# build from source with cargo, which is what this step did unconditionally
# before. Installing never gets harder than it was; it only stops requiring a
# Rust toolchain when a matching release exists.
#
# The download-and-verify shape is borrowed from herdr-file-viewer's
# scripts/fetch-or-build.sh by Saeed Marzban (MIT).
#
# The match is by declared VERSION, not by commit: a checkout ahead of the
# last tag still installs that tag's binary instead of forcing a compile.
# Integrity is unaffected - the asset is still SHA-256 verified, and a version
# with no published release 404s straight to the source build.
#
# PALETTE_REPO_ROOT / PALETTE_CARGO_TOML / PALETTE_OUT / PALETTE_BASE_URL are
# overridable so tests/fetch-or-build.bats can exercise every path of this
# script against local files, with no network.
set -u

repo="vjeantet/herdr-palette"

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root="${PALETTE_REPO_ROOT:-$script_dir/..}"
cargo_toml="${PALETTE_CARGO_TOML:-$repo_root/Cargo.toml}"
out="${PALETTE_OUT:-$repo_root/target/release/herdr-palette}"
base_url="${PALETTE_BASE_URL:-https://github.com/$repo/releases/download}"

have() { command -v "$1" >/dev/null 2>&1; }

# Build from source - the original, unconditional behaviour. ~/.cargo/env is
# sourced because herdr may have been launched from the Dock, where the process
# inherits launchd's PATH and not the login shell's; the `[ -f ]` guard keeps a
# missing env file from aborting the build.
build_from_source() {
  # shellcheck source=/dev/null
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! have cargo; then
    echo "herdr-palette needs a Rust toolchain to build, but cargo was not found. Install it from https://rustup.rs then re-run: herdr plugin install $repo" >&2
    exit 1
  fi
  exec cargo build --release
}

fallback() {
  echo "herdr-palette: $1 - building from source instead." >&2
  [ -n "${tmpdir:-}" ] && rm -rf "$tmpdir"
  build_from_source
}

download() { # download <url> <dest>
  if have curl; then
    curl -fsSL -o "$2" "$1"
  elif have wget; then
    wget -q -O "$2" "$1"
  else
    return 127
  fi
}

sha256_of() { # prints the hex digest of file $1
  if have sha256sum; then
    sha256sum "$1" | awk '{print $1}'
  elif have shasum; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 127
  fi
}

# --- resolve the target triple from the platform ----------------------------
# Every Linux target is static musl on purpose: a glibc build made on the CI
# runner would refuse to start on an older distribution (Debian 12 ships glibc
# 2.36), which is precisely the machine this is most useful on.
os=$(uname -s 2>/dev/null || echo unknown)
arch=$(uname -m 2>/dev/null || echo unknown)
triple=""
case "$os" in
  Darwin)
    case "$arch" in
      arm64|aarch64) triple="aarch64-apple-darwin" ;;
      x86_64|amd64)  triple="x86_64-apple-darwin" ;;
    esac
    ;;
  Linux)
    case "$arch" in
      x86_64|amd64)  triple="x86_64-unknown-linux-musl" ;;
      # A 64-bit kernel running a 32-bit userland - a stock Raspberry Pi OS
      # armhf install on recent hardware - reports aarch64 or armv8l here while
      # every binary on the system is 32-bit arm. getconf settles it; the exec
      # check further down catches whatever getconf gets wrong.
      aarch64|arm64)
        if [ "$(getconf LONG_BIT 2>/dev/null || echo 64)" = "32" ]; then
          triple="armv7-unknown-linux-musleabihf"
        else
          triple="aarch64-unknown-linux-musl"
        fi
        ;;
      armv7l|armv8l) triple="armv7-unknown-linux-musleabihf" ;;
    esac
    ;;
esac
[ -n "$triple" ] || fallback "no prebuilt binary for $os/$arch"

# --- read the version this checkout declares --------------------------------
version=$(grep -E '^version *= *"' "$cargo_toml" 2>/dev/null | head -n 1 | sed -E 's/^version *= *"([^"]+)".*/\1/')
[ -n "$version" ] || fallback "could not read version from $cargo_toml"

asset="herdr-palette-$triple"

tmpdir=$(mktemp -d 2>/dev/null) || fallback "could not create a temp dir"
trap 'rm -rf "$tmpdir"' EXIT

# For transparency only, never a failure: when this is a git work tree and the
# release publishes a COMMIT marker, note that the checkout carries source the
# released binary does not. A missing marker is not an error.
ahead_note=""
if have git && git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  head_rev=$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo nohead)
  if download "$base_url/v$version/COMMIT" "$tmpdir/COMMIT" 2>/dev/null; then
    release_commit=$(tr -d '[:space:]' < "$tmpdir/COMMIT" 2>/dev/null)
    if [ -n "$release_commit" ] && [ "$head_rev" != "$release_commit" ]; then
      ahead_note=" Note: this checkout ($head_rev) is ahead of the v$version release commit ($release_commit), so unreleased source is not in this binary."
    fi
  fi
fi

tmpbin="$tmpdir/$asset"
tmpsum="$tmpdir/$asset.sha256"

download "$base_url/v$version/$asset" "$tmpbin" || fallback "no prebuilt binary published for v$version ($asset)"
download "$base_url/v$version/$asset.sha256" "$tmpsum" || fallback "no checksum published for v$version ($asset.sha256)"

# One checksum file per asset, each holding the single line `sha256sum` emits,
# so the release job never has to collect the matrix's results into a shared
# file. The name is still matched, not just the digest: that is what makes a
# checksum file served for the wrong asset a miss instead of a false pass. The
# separator is two spaces in coreutils text mode and ` *` in binary mode;
# accept either rather than forcing a source build over that detail.
expected=$(grep -E "^[0-9a-f]{64} [ *]$asset\$" "$tmpsum" 2>/dev/null | awk '{print $1}' | head -n 1)
[ -n "$expected" ] || fallback "no checksum listed for $asset"

actual=$(sha256_of "$tmpbin") || fallback "no sha-256 tool (sha256sum/shasum) available"
if [ "$actual" != "$expected" ]; then
  fallback "checksum mismatch for $asset (expected $expected, got $actual)"
fi

chmod +x "$tmpbin"

# Last gate before installing: run it once. The palette prints its usage and
# exits 2 when given no subcommand (src/main.rs), so this has no side effect -
# what it proves is that the kernel accepts the binary at all. 126/127 are the
# shell's "could not execute" codes, which is what a wrong triple looks like
# when uname or getconf misled the mapping above.
"$tmpbin" >/dev/null 2>&1
rc=$?
if [ "$rc" -eq 126 ] || [ "$rc" -eq 127 ]; then
  fallback "the prebuilt for $triple does not run on this machine (exit $rc)"
fi

mkdir -p "$(dirname "$out")"
mv -f "$tmpbin" "$out" || fallback "could not install the verified binary to $out"
echo "herdr-palette: installed prebuilt v$version ($triple), verified SHA-256.$ahead_note"
exit 0
