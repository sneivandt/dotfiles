#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases.
# --build: builds the Rust binary from source (requires cargo).
#
# All arguments (except --build) are forwarded to the dotfiles binary.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CACHE_FILE="$DOTFILES_ROOT/.dotfiles-version-cache"
CACHE_MAX_AGE=3600  # seconds

# --------------------------------------------------------------------------- #
# Parse --build flag (remove it from args forwarded to binary)
# --------------------------------------------------------------------------- #
BUILD_MODE=false
for arg in "$@"; do
  if [ "$arg" = "--build" ]; then
    BUILD_MODE=true
    break
  fi
done

# Rebuild args without --build
set_args() {
  FORWARD_ARGS=""
  for arg in "$@"; do
    if [ "$arg" != "--build" ]; then
      FORWARD_ARGS="$FORWARD_ARGS \"$arg\""
    fi
  done
}
set_args "$@"

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
  eval exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" "$FORWARD_ARGS"
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

# Get the latest release tag from GitHub
get_latest_version() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "https://api.github.com/repos/$REPO/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  else
    echo ""
  fi
}

# Download the binary for the given version tag
download_binary() {
  version="$1"
  url="https://github.com/$REPO/releases/download/$version/dotfiles-linux-x86_64"

  mkdir -p "$BIN_DIR"

  echo "Downloading dotfiles $version..."
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$BINARY" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$BINARY" "$url"
  else
    echo "ERROR: curl or wget required to download binary." >&2
    exit 1
  fi

  chmod +x "$BINARY"

  # Download and verify checksum if available
  checksum_url="https://github.com/$REPO/releases/download/$version/checksums.sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    tmpfile=$(mktemp)
    if curl -fsSL -o "$tmpfile" "$checksum_url" 2>/dev/null || \
       wget -qO "$tmpfile" "$checksum_url" 2>/dev/null; then
      expected=$(grep "dotfiles-linux-x86_64" "$tmpfile" | awk '{print $1}')
      actual=$(sha256sum "$BINARY" | awk '{print $1}')
      if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
        echo "ERROR: Checksum verification failed!" >&2
        rm -f "$BINARY"
        rm -f "$tmpfile"
        exit 1
      fi
    fi
    rm -f "$tmpfile"
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
      return 0
    fi
    echo "ERROR: Cannot determine latest version and no local binary found." >&2
    echo "Use --build to build from source, or check your internet connection." >&2
    exit 1
  fi

  # Download if missing or outdated
  if [ "$local_version" = "none" ] || [ "$local_version" != "$latest" ]; then
    download_binary "$latest"
  fi

  update_cache "$latest"
}

ensure_binary
eval exec "$BINARY" --root "$DOTFILES_ROOT" "$FORWARD_ARGS"
