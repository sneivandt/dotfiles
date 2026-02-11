#!/bin/sh
set -o errexit
set -o nounset

console_output="$1"

# Determine log file location
log_file="${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.log"

echo "Checking for DRY-RUN mode confirmation"
# Check for DRY-RUN in log file or DRY-RUN MODE message in console
if grep -q '\[DRY-RUN \]' "$log_file" || grep -q "DRY-RUN MODE" "$console_output"; then
  echo "✓ DRY-RUN mode confirmed"
else
  echo "Error: No DRY-RUN confirmation found"
  exit 1
fi

echo "Checking for uninstall operations"
if ! grep -q "Checking symlinks to remove\|Would remove\|Skipping uninstall" "$log_file" && ! grep -q "Checking symlinks to remove\|Would remove\|Skipping" "$console_output"; then
  echo "Error: Uninstall should show removal operations or skips"
  exit 1
fi
echo "✓ Uninstall operations confirmed"

echo "All uninstall assertions passed!"
