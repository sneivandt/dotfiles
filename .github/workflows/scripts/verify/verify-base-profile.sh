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

echo "Checking for base profile symlink operations"
if ! grep "Would link.*bashrc" "$log_file"; then
  echo "Error: Should link bashrc in base profile"
  exit 1
fi
if ! grep "Would link.*zshrc" "$log_file"; then
  echo "Error: Should link zshrc in base profile"
  exit 1
fi
if ! grep "Would link.*config/git/config" "$log_file"; then
  echo "Error: Should link git config in base profile"
  exit 1
fi
echo "✓ Base profile symlinks confirmed"

echo "Verifying arch/desktop items are NOT linked"
if grep "Would link.*xmonad" "$log_file"; then
  echo "Error: Should not link xmonad in base profile"
  exit 1
fi
if grep "Would link.*Xresources" "$log_file"; then
  echo "Error: Should not link Xresources in base profile"
  exit 1
fi
if grep "Would link.*pacman\.conf" "$log_file"; then
  echo "Error: Should not link pacman.conf in base profile"
  exit 1
fi
echo "✓ Arch/desktop items properly excluded"

echo "All base profile assertions passed!"
