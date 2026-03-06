#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases if no
# binary is present, then runs it. The binary handles its own updates.
# --build: builds the Rust binary from source (requires cargo).
#
# --build is consumed by this script and stripped before forwarding remaining
# arguments to the dotfiles binary.
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

die() {
  echo "ERROR: $1" >&2
  exit 1
}

# --------------------------------------------------------------------------- #
# Parse arguments — validate wrapper-supported flags and detect --build
# --------------------------------------------------------------------------- #
BUILD_MODE=false
ACTION=""
PROFILE=""
DRY_RUN=false
VERBOSE=false

while [ "$#" -gt 0 ]; do
  case "$1" in
    --build)
      BUILD_MODE=true
      ;;
    install|uninstall|test|version)
      if [ -n "$ACTION" ]; then
        die "multiple actions provided: '$ACTION' and '$1'"
      fi
      ACTION="$1"
      ;;
    -p|--profile)
      shift
      if [ "$#" -eq 0 ]; then
        die "--profile requires a value"
      fi
      case "$1" in
        base|desktop)
          PROFILE="$1"
          ;;
        *)
          die "invalid profile '$1'"
          ;;
      esac
      ;;
    -d|--dry-run)
      DRY_RUN=true
      ;;
    -v|--verbose)
      VERBOSE=true
      ;;
    *)
      die "unsupported argument '$1'. Use the binary directly for advanced flags."
      ;;
  esac
  shift
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
  set -- --root "$DOTFILES_ROOT"
  if [ -n "$ACTION" ]; then
    set -- "$@" "$ACTION"
  fi
  if [ -n "$PROFILE" ]; then
    set -- "$@" --profile "$PROFILE"
  fi
  if [ "$DRY_RUN" = true ]; then
    set -- "$@" --dry-run
  fi
  if [ "$VERBOSE" = true ]; then
    set -- "$@" --verbose
  fi
  exec "$DOTFILES_ROOT/cli/target/dev-opt/dotfiles" "$@"
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

# Verify checksum in a subshell to scope the trap safely.
_verify_checksum() {
  _vc_asset="$1"
  _vc_binary="$2"
  _vc_url="$3"
  tmpfile=$(mktemp)
  trap 'rm -f "$tmpfile"' EXIT
  if ! download_with_retry "$_vc_url" "$tmpfile"; then
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
  if ! ( _verify_checksum "$asset" "$BINARY" "$checksum_url" ); then
    rm -f "$BINARY"
    exit 1
  fi
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

set -- --root "$DOTFILES_ROOT"
if [ -n "$ACTION" ]; then
  set -- "$@" "$ACTION"
fi
if [ -n "$PROFILE" ]; then
  set -- "$@" --profile "$PROFILE"
fi
if [ "$DRY_RUN" = true ]; then
  set -- "$@" --dry-run
fi
if [ "$VERBOSE" = true ]; then
  set -- "$@" --verbose
fi

exec "$BINARY" "$@"
