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
DOTFILES_WRAPPER="sh"
export DOTFILES_WRAPPER

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CONNECT_TIMEOUT=10   # seconds — TCP connect timeout
TRANSFER_TIMEOUT=120 # seconds — total transfer timeout

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

# Download a URL to a file.
# Usage: download_file <url> <output_file>
download_file() {
  _df_url="$1"
  _df_out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
         -o "$_df_out" "$_df_url" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$_df_out" --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
         "$_df_url" 2>/dev/null
  else
    echo "ERROR: curl or wget required to download binary." >&2
    exit 1
  fi
}

# Resolve the latest release tag from the GitHub API.
# Prints the tag (e.g. "v0.2.0") on success, or an empty string on failure.
resolve_release_tag() {
  _api_url="https://api.github.com/repos/$REPO/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
         "$_api_url" 2>/dev/null | \
      awk -F'"' '/"tag_name"/{print $4; exit}'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
         "$_api_url" 2>/dev/null | \
      awk -F'"' '/"tag_name"/{print $4; exit}'
  fi
}

# Verify checksum in a subshell to scope the trap safely.
_verify_checksum() {
  _vc_tag="$1"
  _vc_asset="$2"
  _vc_binary="$3"
  tmpfile=$(mktemp)
  trap 'rm -f "$tmpfile"' EXIT
  if ! download_file \
    "https://github.com/$REPO/releases/download/$_vc_tag/checksums.sha256" \
    "$tmpfile"; then
    echo "ERROR: Failed to download checksum file." >&2
    return 1
  fi
  expected=$(awk -v fname="$_vc_asset" '{ name=$2; sub(/^\*/, "", name); if (name == fname) print $1 }' "$tmpfile")
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
  _arch="$(uname -m)"
  case "$_arch" in
    x86_64|amd64)  asset="dotfiles-linux-x86_64" ;;
    aarch64|arm64) asset="dotfiles-linux-aarch64" ;;
    *)
      echo "ERROR: Unsupported architecture: $_arch" >&2
      echo "Supported architectures: x86_64, amd64, aarch64, arm64" >&2
      exit 1
      ;;
  esac

  tag=$(resolve_release_tag)
  if [ -z "$tag" ]; then
    echo "ERROR: Failed to resolve latest release tag." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    exit 1
  fi

  url="https://github.com/$REPO/releases/download/$tag/$asset"

  mkdir -p "$BIN_DIR"

  echo "Downloading dotfiles bootstrap binary..."
  if ! download_file "$url" "$BINARY"; then
    echo "ERROR: Failed to download dotfiles binary." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    rm -f "$BINARY"
    exit 1
  fi

  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "ERROR: sha256sum not found. Cannot verify download integrity." >&2
    rm -f "$BINARY"
    exit 1
  fi
  if ! ( _verify_checksum "$tag" "$asset" "$BINARY" ); then
    rm -f "$BINARY"
    exit 1
  fi

  chmod +x "$BINARY"
}

# Bootstrap: download the latest binary only if no binary is present.
# Subsequent updates are handled by the binary itself.
if [ ! -x "$BINARY" ]; then
  download_binary
fi

exec "$BINARY" "$@"
