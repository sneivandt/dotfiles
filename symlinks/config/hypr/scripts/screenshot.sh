#!/bin/sh
set -o errexit
set -o nounset

# Screenshot via grim + slurp
# Saves to ~/screenshots/ with timestamp filename

SCREENSHOT_DIR="$HOME/screenshots"
mkdir -p "$SCREENSHOT_DIR"

FILENAME="$SCREENSHOT_DIR/$(date +%Y%m%d-%H%M%S).png"

if command -v slurp >/dev/null 2>&1 && command -v grim >/dev/null 2>&1; then
  REGION="$(slurp 2>/dev/null)" || exit 0
  grim -g "$REGION" "$FILENAME"
  wl-copy < "$FILENAME" 2>/dev/null || true
elif command -v grim >/dev/null 2>&1; then
  grim "$FILENAME"
  wl-copy < "$FILENAME" 2>/dev/null || true
else
  echo "ERROR: grim not found" >&2
  exit 1
fi
