#!/bin/sh
set -o errexit
set -o nounset

# Screenshot via grim + slurp
# Saves to ~/screenshots/ after selection, reserving a unique destination.

if ! command -v grim >/dev/null 2>&1; then
  echo "ERROR: grim not found" >&2
  exit 1
fi

set --
if command -v slurp >/dev/null 2>&1; then
  REGION="$(slurp 2>/dev/null)" || exit 0
  set -- -g "$REGION"
fi

SCREENSHOT_DIR="$HOME/screenshots"
mkdir -p "$SCREENSHOT_DIR"
FILENAME="$SCREENSHOT_DIR/$(date +%Y%m%d-%H%M%S)-$$.png"
partial_file=""
cleanup() {
  if [ -n "$partial_file" ]; then
    rm -f -- "$partial_file"
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# noclobber also protects an existing destination if a PID is ever reused.
(umask 077; set -C; : > "$FILENAME")
partial_file=$FILENAME
grim "$@" "$FILENAME"
partial_file=""
wl-copy < "$FILENAME" 2>/dev/null || true
