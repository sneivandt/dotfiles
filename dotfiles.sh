#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases if no
# binary is present, then runs it. The binary handles its own updates.
# --build: builds the Rust binary from source (requires cargo).
#
# All options except --build are forwarded verbatim to the dotfiles binary.
# Commonly used flags: --profile <name>, --dry-run.
# Advanced flags (--skip, --only, --root, --no-parallel) require invoking
# the binary directly.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CONNECT_TIMEOUT=10   # seconds — TCP connect timeout
TRANSFER_TIMEOUT=120 # seconds — total transfer timeout
RETRY_COUNT=3        # number of download attempts
RETRY_DELAY=2        # seconds between retries
# NOTE: Keep these constants in sync with the equivalent values in dotfiles.ps1.
# dotfiles.ps1: $ConnectTimeout / $TransferTimeout / $RetryCount / $RetryDelay

# --------------------------------------------------------------------------- #
# Parse arguments — extract --build, pass everything else to the binary
# --------------------------------------------------------------------------- #
BUILD_MODE=false
_forward_args=""
for arg in "$@"; do
  if [ "$arg" = "--build" ]; then
    BUILD_MODE=true
  else
    _forward_args="$_forward_args $(printf '%s' "$arg" | sed "s/'/'\\\\''/g; s/^/'/; s/$/'/")"
  fi
done
# shellcheck disable=SC2086  # unquoted: eval must word-split the pre-quoted tokens
eval set -- $_forward_args
unset _forward_args

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
  exec "$DOTFILES_ROOT/cli/target/dev-opt/dotfiles" --root "$DOTFILES_ROOT" "$@"
fi

# --------------------------------------------------------------------------- #
# Production mode: ensure binary is present
# --------------------------------------------------------------------------- #

# Detect CPU architecture
detect_arch() {
  case "$(uname -m)" in
    aarch64|arm64) echo "aarch64" ;;
    *)             echo "x86_64"  ;;
  esac
}

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

# Get the latest release tag from GitHub
get_latest_version() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
      "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
      "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  else
    echo ""
  fi
}

# Download the binary for the given version tag
download_binary() {
  version="$1"
  arch=$(detect_arch)
  asset="dotfiles-linux-$arch"
  url="https://github.com/$REPO/releases/download/$version/$asset"

  mkdir -p "$BIN_DIR"

  echo "Downloading dotfiles $version..."
  if ! download_with_retry "$url" "$BINARY"; then
    echo "ERROR: Failed to download dotfiles $version after $RETRY_COUNT attempts." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    rm -f "$BINARY"
    exit 1
  fi

  chmod +x "$BINARY"

  # Download and verify checksum
  checksum_url="https://github.com/$REPO/releases/download/$version/checksums.sha256"
  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "ERROR: sha256sum not found. Cannot verify download integrity." >&2
    rm -f "$BINARY"
    exit 1
  fi
  tmpfile=$(mktemp)
  # Use a dedicated cleanup function so the trap survives re-entrant calls
  # and does not clobber any outer EXIT trap.
  _cleanup_download_tmpfile() { rm -f "$tmpfile"; }
  trap '_cleanup_download_tmpfile' EXIT
  if ! download_with_retry "$checksum_url" "$tmpfile"; then
    echo "ERROR: Failed to download checksum file for $version." >&2
    rm -f "$BINARY"
    exit 1
  fi
  expected=$(awk -v fname="$asset" '$2 == fname {print $1}' "$tmpfile")
  if [ -z "$expected" ]; then
    echo "ERROR: Checksum not found in checksum file for $asset." >&2
    rm -f "$BINARY"
    exit 1
  fi
  actual=$(sha256sum "$BINARY" | awk '{print $1}')
  if [ "$expected" != "$actual" ]; then
    echo "ERROR: Checksum verification failed!" >&2
    rm -f "$BINARY"
    exit 1
  fi
  _cleanup_download_tmpfile
  trap - EXIT
  unset -f _cleanup_download_tmpfile
}

# Bootstrap: download the latest binary only if no binary is present.
# Subsequent updates are handled by the binary itself.
if [ ! -x "$BINARY" ]; then
  latest=$(get_latest_version)
  if [ -z "$latest" ]; then
    echo "ERROR: Cannot determine latest version and no local binary found." >&2
    echo "Use --build to build from source, or check your internet connection." >&2
    exit 1
  fi
  download_binary "$latest"
fi

exec "$BINARY" --root "$DOTFILES_ROOT" "$@"
