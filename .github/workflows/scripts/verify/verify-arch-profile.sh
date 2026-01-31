#!/bin/sh
set -o errexit
set -o nounset

log_file="$1"

echo "Checking for DRY-RUN messages"
if ! grep -q "DRY-RUN:" "$log_file"; then
  echo "Error: No DRY-RUN messages found"
  exit 1
fi
echo "✓ DRY-RUN mode confirmed"

echo "Checking for arch-specific symlink operations"
if ! grep "Would link.*config/pacman\.conf" "$log_file"; then
  echo "Error: Should link pacman.conf in arch profile"
  exit 1
fi
if ! grep "Would link.*config/paru/paru\.conf" "$log_file"; then
  echo "Error: Should link paru.conf in arch profile"
  exit 1
fi
if ! grep "Would link.*bashrc" "$log_file"; then
  echo "Error: Should link bashrc (from base) in arch profile"
  exit 1
fi
echo "✓ Arch-specific symlinks confirmed"

echo "Verifying desktop items are NOT linked"
if grep "Would link.*xmonad" "$log_file"; then
  echo "Error: Should not link xmonad in arch profile"
  exit 1
fi
if grep "Would link.*Xresources" "$log_file"; then
  echo "Error: Should not link Xresources in arch profile"
  exit 1
fi
if grep "Would link.*xinitrc" "$log_file"; then
  echo "Error: Should not link xinitrc in arch profile"
  exit 1
fi
echo "✓ Desktop items properly excluded"

echo "All arch profile assertions passed!"
