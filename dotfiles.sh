#!/bin/sh
set -eu

# dotfiles.sh — Minimal entry point for the dotfiles management engine.
#
# Handles --build (cargo must exist before the binary does) and performs a
# one-time initial download.  Everything else — argument parsing, version
# management, help text — is handled by the Rust binary itself.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT
REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"

# --build must be handled here: cargo is needed before the binary exists.
if [ "${1:-}" = "--build" ]; then
    shift
    command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found. Install Rust from https://rustup.rs/" >&2; exit 1; }
    cd "$DOTFILES_ROOT/cli" && cargo build --release
    exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" "$@"
fi

# First-time setup: minimal one-shot download when no binary exists yet.
# Subsequent version checks and updates are handled by `dotfiles bootstrap`.
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
        echo "ERROR: curl or wget is required for first-time setup." >&2; exit 1
    fi
    [ -n "$_latest" ] || { echo "ERROR: Cannot reach GitHub. Use --build." >&2; exit 1; }
    case "$(uname -m)" in aarch64|arm64) _arch=aarch64 ;; *) _arch=x86_64 ;; esac
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

# Delegate version management to Rust; set -e aborts on failure.
"$BINARY" --root "$DOTFILES_ROOT" bootstrap --repo "$REPO"
exec "$BINARY" --root "$DOTFILES_ROOT" "$@"
