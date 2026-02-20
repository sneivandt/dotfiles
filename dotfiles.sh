#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases.
# --build: builds the Rust binary from source (requires cargo).
#
# Only recognised options are forwarded to the dotfiles binary.
# Developer flags (--skip, --only, --root, --no-parallel) require
# invoking the binary directly.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CACHE_FILE="$BIN_DIR/.dotfiles-version-cache"
CACHE_MAX_AGE=3600   # seconds
CONNECT_TIMEOUT=10   # seconds — TCP connect timeout
TRANSFER_TIMEOUT=120 # seconds — total transfer timeout
RETRY_COUNT=3        # number of download attempts
RETRY_DELAY=2        # seconds between retries

# --------------------------------------------------------------------------- #
# Usage
# --------------------------------------------------------------------------- #
usage() {
  echo "Usage: dotfiles.sh [--build] <command> [options]"
  echo ""
  echo "Commands:"
  echo "  install     Install dotfiles and configure system"
  echo "  uninstall   Remove installed dotfiles"
  echo "  test        Run configuration validation"
  echo "  version     Print version information"
  echo ""
  echo "Options:"
  echo "  --build           Build and run from source (requires cargo)"
  echo "  -p, --profile P   Use specific profile (base, desktop)"
  echo "  -d, --dry-run     Preview changes without applying"
  echo "  -v, --verbose     Enable verbose logging"
  echo "  -h, --help        Show this help message"
  exit 0
}

# --------------------------------------------------------------------------- #
# Parse arguments — only recognised options are accepted
# --------------------------------------------------------------------------- #
BUILD_MODE=false
CLI_ARGS=""
EXPECT_VALUE=""

for arg in "$@"; do
  if [ -n "$EXPECT_VALUE" ]; then
    CLI_ARGS="$CLI_ARGS $(printf "'%s'" "$(printf '%s' "$arg" | sed "s/'/'\\\\''/g")")"
    EXPECT_VALUE=""
    continue
  fi
  case "$arg" in
    --build)                BUILD_MODE=true ;;
    -h|--help)              usage ;;
    -p|--profile)           CLI_ARGS="$CLI_ARGS '$arg'"; EXPECT_VALUE=1 ;;
    -d|--dry-run)           CLI_ARGS="$CLI_ARGS '$arg'" ;;
    -v|--verbose)           CLI_ARGS="$CLI_ARGS '$arg'" ;;
    install|uninstall|test|version)
                            CLI_ARGS="$CLI_ARGS '$arg'" ;;
    -*)                     echo "ERROR: Unknown option: $arg" >&2
                            echo "Run 'dotfiles.sh --help' for usage." >&2
                            exit 1 ;;
    *)                      echo "ERROR: Unknown argument: $arg" >&2
                            echo "Run 'dotfiles.sh --help' for usage." >&2
                            exit 1 ;;
  esac
done

if [ -n "$EXPECT_VALUE" ]; then
  echo "ERROR: Option requires a value: --profile" >&2
  exit 1
fi

eval set -- "$CLI_ARGS"

# --------------------------------------------------------------------------- #
# Build mode: build from source and run
# --------------------------------------------------------------------------- #
if [ "$BUILD_MODE" = true ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo not found. Install Rust to use --build mode." >&2
    exit 1
  fi
  cd "$DOTFILES_ROOT/cli"
  cargo build --release
  exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" "$@"
fi

# --------------------------------------------------------------------------- #
# Production mode: ensure latest binary is available
# --------------------------------------------------------------------------- #

# Check if the cached version is still fresh
is_cache_fresh() {
  if [ ! -f "$CACHE_FILE" ]; then
    return 1
  fi
  cached_ts=$(sed -n '2p' "$CACHE_FILE" 2>/dev/null || echo "0")
  now=$(date +%s)
  age=$((now - cached_ts))
  [ "$age" -lt "$CACHE_MAX_AGE" ]
}

# Get the currently installed binary version
get_local_version() {
  if [ -x "$BINARY" ]; then
    "$BINARY" version 2>/dev/null | awk '{print $2}' || echo "none"
  else
    echo "none"
  fi
}

# Get the version tag from the cache file (line 1)
get_cached_version() {
  if [ -f "$CACHE_FILE" ]; then
    sed -n '1p' "$CACHE_FILE" 2>/dev/null || echo ""
  else
    echo ""
  fi
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

  # Download and verify checksum if available
  checksum_url="https://github.com/$REPO/releases/download/$version/checksums.sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    tmpfile=$(mktemp)
    trap 'rm -f "$tmpfile"' EXIT
    if download_with_retry "$checksum_url" "$tmpfile"; then
      expected=$(grep "$asset" "$tmpfile" | awk '{print $1}')
      actual=$(sha256sum "$BINARY" | awk '{print $1}')
      if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
        echo "ERROR: Checksum verification failed!" >&2
        rm -f "$BINARY"
        rm -f "$tmpfile"
        exit 1
      fi
    fi
    rm -f "$tmpfile"
    trap - EXIT
  else
    echo "WARNING: sha256sum not found, skipping checksum verification" >&2
  fi
}

# Update the version cache
update_cache() {
  version="$1"
  echo "$version" > "$CACHE_FILE"
  date +%s >> "$CACHE_FILE"
}

# Ensure binary is present and up to date
ensure_binary() {
  local_version=$(get_local_version)

  # Fast path: binary exists and cache is fresh
  if [ "$local_version" != "none" ] && is_cache_fresh; then
    return 0
  fi

  # Check latest version
  latest=$(get_latest_version)
  if [ -z "$latest" ]; then
    # Can't reach GitHub — use existing binary if available
    if [ "$local_version" != "none" ]; then
      echo "Using cached dotfiles $local_version (offline)"
      return 0
    fi
    echo "ERROR: Cannot determine latest version and no local binary found." >&2
    echo "Use --build to build from source, or check your internet connection." >&2
    exit 1
  fi

  # Compare cached release tag (not binary's self-reported version) to avoid
  # unnecessary re-downloads when git-describe output differs from release tag.
  cached=$(get_cached_version)
  if [ "$local_version" = "none" ] || [ "$cached" != "$latest" ]; then
    download_binary "$latest"
  fi

  update_cache "$latest"
}

ensure_binary
exec "$BINARY" --root "$DOTFILES_ROOT" "$@"
