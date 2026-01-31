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
  echo "Error: Should link pacman.conf in arch-desktop profile"
  exit 1
fi
if ! grep "Would link.*config/paru/paru\.conf" "$log_file"; then
  echo "Error: Should link paru.conf in arch-desktop profile"
  exit 1
fi
echo "✓ Arch-specific symlinks confirmed"

echo "Checking for desktop-specific symlink operations"
if ! grep "Would link.*config/xmonad" "$log_file"; then
  echo "Error: Should link xmonad config in arch-desktop profile"
  exit 1
fi
if ! grep "Would link.*Xresources" "$log_file"; then
  echo "Error: Should link Xresources in arch-desktop profile"
  exit 1
fi
if ! grep "Would link.*xinitrc" "$log_file"; then
  echo "Error: Should link xinitrc in arch-desktop profile"
  exit 1
fi
if ! grep "Would link.*config/dunst" "$log_file"; then
  echo "Error: Should link dunst config in arch-desktop profile"
  exit 1
fi
echo "✓ Desktop-specific symlinks confirmed"

echo "Verifying Windows items are NOT linked"
if grep "Would link.*WindowsTerminal" "$log_file"; then
  echo "Error: Should not link WindowsTerminal in arch-desktop profile"
  exit 1
fi
if grep "Would link.*AppData" "$log_file"; then
  echo "Error: Should not link AppData items in arch-desktop profile"
  exit 1
fi
echo "✓ Windows items properly excluded"

echo "All arch-desktop profile assertions passed!"
