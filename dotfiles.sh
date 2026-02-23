#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases and
#          delegates version management to the Rust bootstrap command.
# --build: builds the Rust binary from source (requires cargo).

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"

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
# Production mode: ensure binary is present, then delegate version management
# --------------------------------------------------------------------------- #

# Initial bootstrap: perform a minimal first-time download if no binary exists.
# After this, the Rust bootstrap command takes over version management.
if [ ! -x "$BINARY" ]; then
  if command -v curl >/dev/null 2>&1; then
    _latest=$(curl -fsSL --connect-timeout 10 --max-time 120 \
      "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
      | grep '"tag_name"' | head -1 \
      | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  elif command -v wget >/dev/null 2>&1; then
    _latest=$(wget -qO- --connect-timeout=10 --timeout=120 \
      "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
      | grep '"tag_name"' | head -1 \
      | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  else
    echo "ERROR: curl or wget required for first-time setup." >&2
    exit 1
  fi
  if [ -z "$_latest" ]; then
    echo "ERROR: Cannot reach GitHub. Use --build to build from source." >&2
    exit 1
  fi
  case "$(uname -m)" in
    aarch64|arm64) _arch="aarch64" ;;
    *)             _arch="x86_64"  ;;
  esac
  mkdir -p "$BIN_DIR"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout 10 --max-time 120 \
      "https://github.com/$REPO/releases/download/$_latest/dotfiles-linux-$_arch" \
      -o "$BINARY"
  else
    wget -qO "$BINARY" --connect-timeout=10 --timeout=120 \
      "https://github.com/$REPO/releases/download/$_latest/dotfiles-linux-$_arch"
  fi
  chmod +x "$BINARY"
fi

# Delegate version checking, downloading, and cache management to Rust.
# If bootstrap returns non-zero, set -o errexit aborts the script.
"$BINARY" --root "$DOTFILES_ROOT" bootstrap --repo "$REPO"
exec "$BINARY" --root "$DOTFILES_ROOT" "$@"
