#!/bin/sh
#
# check.sh — POSIX shim delegating to .github/workflows/scripts/linux/check.py.
#
# Usage:
#   sh .github/workflows/scripts/linux/check.sh                 # run the default stages
#   sh .github/workflows/scripts/linux/check.sh fmt clippy      # run only the named stages
#   sh .github/workflows/scripts/linux/check.sh --all           # include opt-in stages
#   sh .github/workflows/scripts/linux/check.sh --list          # list available stages

set -o nounset

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)

if command -v python3 >/dev/null 2>&1; then
  exec python3 -B "$SCRIPT_DIR/check.py" "$@"
elif command -v python >/dev/null 2>&1; then
  exec python -B "$SCRIPT_DIR/check.py" "$@"
else
  printf 'python3 is required to run %s/check.py\n' "$SCRIPT_DIR" >&2
  exit 1
fi
