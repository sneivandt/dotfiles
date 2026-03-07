#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases if no
# binary is present, then runs it. The binary handles its own updates.
# --build: builds the Rust binary from source (requires cargo).
#
# The wrapper only handles bootstrap/build concerns and otherwise forwards
# arguments to the Rust binary unchanged.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CONNECT_TIMEOUT=10   # seconds — TCP connect timeout
TRANSFER_TIMEOUT=120 # seconds — total transfer timeout
RETRY_COUNT=3        # number of download attempts
RETRY_DELAY=2        # seconds between retries
# NOTE: Keep TRANSFER_TIMEOUT / RETRY_COUNT / RETRY_DELAY aligned with the
# corresponding constants in dotfiles.ps1.

BUILD_MODE=false
for arg in "$@"; do
  if [ "$arg" = "--build" ]; then
    BUILD_MODE=true
    break
  fi
done

# --------------------------------------------------------------------------- #
# Build mode: build from source and run
# --------------------------------------------------------------------------- #
if [ "$BUILD_MODE" = true ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo not found. Install Rust to use --build mode." >&2
    exit 1
  fi
  cd "$DOTFILES_ROOT/cli"
  cargo build --profile dev-opt
  exec "$DOTFILES_ROOT/cli/target/dev-opt/dotfiles" "$@"
fi

# --------------------------------------------------------------------------- #
# Production mode: ensure binary is present
# --------------------------------------------------------------------------- #

# Download a URL to a file with retries
# Usage: download_with_retry <url> <output_file>
download_with_retry() {
  _dwr_url="$1"
  _dwr_out="$2"
  _dwr_attempt=1
  while [ "$_dwr_attempt" -le "$RETRY_COUNT" ]; do
    if [ "$_dwr_attempt" -gt 1 ]; then
      echo "Retry $_dwr_attempt/$RETRY_COUNT after ${RETRY_DELAY}s..." >&2
      sleep "$RETRY_DELAY"
    fi
    if command -v curl >/dev/null 2>&1; then
      if curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
           -o "$_dwr_out" "$_dwr_url" 2>/dev/null; then
        return 0
      fi
    elif command -v wget >/dev/null 2>&1; then
      if wget -qO "$_dwr_out" --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
           "$_dwr_url" 2>/dev/null; then
        return 0
      fi
    else
      echo "ERROR: curl or wget required to download binary." >&2
      exit 1
    fi
    _dwr_attempt=$((_dwr_attempt + 1))
  done
  return 1
}

# Verify checksum in a subshell to scope the trap safely.
_verify_checksum() {
  _vc_asset="$1"
  _vc_binary="$2"
  tmpfile=$(mktemp)
  trap 'rm -f "$tmpfile"' EXIT
  if ! download_with_retry \
    "https://github.com/$REPO/releases/latest/download/checksums.sha256" \
    "$tmpfile"; then
    echo "ERROR: Failed to download checksum file." >&2
    return 1
  fi
  expected=$(awk -v fname="$_vc_asset" '$2 == fname {print $1}' "$tmpfile")
  if [ -z "$expected" ]; then
    echo "ERROR: Checksum not found in checksum file for $_vc_asset." >&2
    return 1
  fi
  actual=$(sha256sum "$_vc_binary" | awk '{print $1}')
  if [ "$expected" != "$actual" ]; then
    echo "ERROR: Checksum verification failed!" >&2
    return 1
  fi
}

# Download the bootstrap binary if needed.
download_binary() {
  case "$(uname -m)" in
    aarch64|arm64) asset="dotfiles-linux-aarch64" ;;
    *)             asset="dotfiles-linux-x86_64" ;;
  esac
  url="https://github.com/$REPO/releases/latest/download/$asset"

  mkdir -p "$BIN_DIR"

  echo "Downloading dotfiles bootstrap binary..."
  if ! download_with_retry "$url" "$BINARY"; then
    echo "ERROR: Failed to download dotfiles after $RETRY_COUNT attempts." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    rm -f "$BINARY"
    exit 1
  fi

  chmod +x "$BINARY"

  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "ERROR: sha256sum not found. Cannot verify download integrity." >&2
    rm -f "$BINARY"
    exit 1
  fi
  if ! ( _verify_checksum "$asset" "$BINARY" ); then
    rm -f "$BINARY"
    exit 1
  fi
}

# Bootstrap: download the latest binary only if no binary is present.
# Subsequent updates are handled by the binary itself.
if [ ! -x "$BINARY" ]; then
  download_binary
fi

exec "$BINARY" "$@"
