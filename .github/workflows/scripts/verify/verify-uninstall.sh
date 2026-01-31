#!/bin/sh
set -o errexit
set -o nounset

log_file="$1"

echo "Checking for DRY-RUN messages"
if ! grep -q "DRY-RUN:" "$log_file"; then
  echo "Error: No DRY-RUN messages found in output"
  exit 1
fi
echo "✓ DRY-RUN mode confirmed"

echo "Checking for uninstall operations"
if ! grep -q "Would remove\|Skipping" "$log_file"; then
  echo "Error: Uninstall should show removal operations or skips"
  exit 1
fi
echo "✓ Uninstall operations confirmed"

echo "All uninstall assertions passed!"
