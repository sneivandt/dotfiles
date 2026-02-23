#!/bin/sh
# dotfiles.sh — Minimal entry point. Handles --build and first-time download;
# everything else (args, version management, help) is handled by the Rust binary.
set -eu

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT
REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"

# --build must be handled here (any position): cargo is needed before the binary exists.
case " $* " in *" --build "*)
    _n=; for _a in "$@"; do [ "$_a" = --build ] || set -- ${_n:+"$@"} "$_a" && _n=y; done
    [ -n "$_n" ] || set --
    command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found. Install Rust from https://rustup.rs/" >&2; exit 1; }
    cd "$DOTFILES_ROOT/cli" && cargo build --release
    exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" "$@"
    ;; esac

# First-time setup: one-shot download; subsequent updates handled by bootstrap.
if [ ! -x "$BINARY" ]; then
    command -v curl >/dev/null 2>&1 || { echo "ERROR: curl is required for first-time setup." >&2; exit 1; }
    _latest=$(curl -fsSL --connect-timeout 10 --max-time 120 \
        "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    [ -n "$_latest" ] || { echo "ERROR: Cannot reach GitHub. Use --build." >&2; exit 1; }
    case "$(uname -m)" in aarch64|arm64) _arch=aarch64 ;; *) _arch=x86_64 ;; esac
    _asset="dotfiles-linux-$_arch"
    _base="https://github.com/$REPO/releases/download/$_latest"
    mkdir -p "$BIN_DIR"
    curl -fsSL --connect-timeout 10 --max-time 120 "$_base/$_asset" -o "$BINARY"
    _expected=$(curl -fsSL --connect-timeout 10 --max-time 120 "$_base/checksums.sha256" 2>/dev/null \
        | grep "$_asset" | cut -d' ' -f1)
    if [ -n "$_expected" ]; then
        _actual=$(sha256sum "$BINARY" | cut -d' ' -f1)
        [ "$_expected" = "$_actual" ] || { rm -f "$BINARY"; echo "ERROR: checksum mismatch" >&2; exit 1; }
    fi
    chmod +x "$BINARY"
fi

"$BINARY" --root "$DOTFILES_ROOT" bootstrap --repo "$REPO"
exec "$BINARY" --root "$DOTFILES_ROOT" "$@"
