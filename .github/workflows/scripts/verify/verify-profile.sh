#!/bin/sh
set -o errexit
set -o nounset

profile="$1"
log_file="$2"

# Validate DRY-RUN mode
echo "Checking for DRY-RUN messages"
if ! grep -q "DRY-RUN:" "$log_file"; then
  echo "Error: No DRY-RUN messages found"
  exit 1
fi
echo "✓ DRY-RUN mode confirmed"

# Profile-specific assertions
case "$profile" in
  base)
    echo "Checking for base profile symlink operations"
    if ! grep -q "Would link.*bashrc" "$log_file"; then
      echo "Error: Should link bashrc in base profile"
      exit 1
    fi
    if ! grep -q "Would link.*zshrc" "$log_file"; then
      echo "Error: Should link zshrc in base profile"
      exit 1
    fi
    if ! grep -q "Would link.*config/git/config" "$log_file"; then
      echo "Error: Should link git config in base profile"
      exit 1
    fi
    echo "✓ Base profile symlinks confirmed"

    echo "Verifying arch/desktop items are NOT linked"
    if grep -q "Would link.*xmonad" "$log_file"; then
      echo "Error: Should not link xmonad in base profile"
      exit 1
    fi
    if grep -q "Would link.*Xresources" "$log_file"; then
      echo "Error: Should not link Xresources in base profile"
      exit 1
    fi
    if grep -q "Would link.*pacman\.conf" "$log_file"; then
      echo "Error: Should not link pacman.conf in base profile"
      exit 1
    fi
    echo "✓ Arch/desktop items properly excluded"
    ;;

  arch)
    echo "Checking for arch-specific symlink operations"
    if ! grep -q "Would link.*config/pacman\.conf" "$log_file"; then
      echo "Error: Should link pacman.conf in arch profile"
      exit 1
    fi
    if ! grep -q "Would link.*config/paru/paru\.conf" "$log_file"; then
      echo "Error: Should link paru.conf in arch profile"
      exit 1
    fi
    if ! grep -q "Would link.*bashrc" "$log_file"; then
      echo "Error: Should link bashrc (from base) in arch profile"
      exit 1
    fi
    echo "✓ Arch-specific symlinks confirmed"

    echo "Verifying desktop items are NOT linked"
    if grep -q "Would link.*xmonad" "$log_file"; then
      echo "Error: Should not link xmonad in arch profile"
      exit 1
    fi
    if grep -q "Would link.*Xresources" "$log_file"; then
      echo "Error: Should not link Xresources in arch profile"
      exit 1
    fi
    if grep -q "Would link.*xinitrc" "$log_file"; then
      echo "Error: Should not link xinitrc in arch profile"
      exit 1
    fi
    echo "✓ Desktop items properly excluded"
    ;;

  arch-desktop)
    echo "Checking for arch-specific symlink operations"
    if ! grep -q "Would link.*config/pacman\.conf" "$log_file"; then
      echo "Error: Should link pacman.conf in arch-desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*config/paru/paru\.conf" "$log_file"; then
      echo "Error: Should link paru.conf in arch-desktop profile"
      exit 1
    fi
    echo "✓ Arch-specific symlinks confirmed"

    echo "Checking for desktop-specific symlink operations"
    if ! grep -q "Would link.*config/xmonad" "$log_file"; then
      echo "Error: Should link xmonad config in arch-desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*Xresources" "$log_file"; then
      echo "Error: Should link Xresources in arch-desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*xinitrc" "$log_file"; then
      echo "Error: Should link xinitrc in arch-desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*config/dunst" "$log_file"; then
      echo "Error: Should link dunst config in arch-desktop profile"
      exit 1
    fi
    echo "✓ Desktop-specific symlinks confirmed"

    echo "Verifying Windows items are NOT linked"
    if grep -q "Would link.*WindowsTerminal" "$log_file"; then
      echo "Error: Should not link WindowsTerminal in arch-desktop profile"
      exit 1
    fi
    if grep -q "Would link.*AppData" "$log_file"; then
      echo "Error: Should not link AppData items in arch-desktop profile"
      exit 1
    fi
    echo "✓ Windows items properly excluded"
    ;;

  desktop)
    echo "Checking for base profile symlink operations"
    if ! grep -q "Would link.*bashrc" "$log_file"; then
      echo "Error: Should link bashrc in desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*zshrc" "$log_file"; then
      echo "Error: Should link zshrc in desktop profile"
      exit 1
    fi
    echo "✓ Base symlinks confirmed"

    echo "Checking for desktop-specific symlink operations"
    if ! grep -q "Would link.*config/Code/User/settings\.json" "$log_file"; then
      echo "Error: Should link VS Code settings in desktop profile"
      exit 1
    fi
    if ! grep -q "Would link.*config/shell/aliases-desktop\.sh" "$log_file"; then
      echo "Error: Should link desktop aliases in desktop profile"
      exit 1
    fi
    echo "✓ Desktop-specific symlinks confirmed"

    echo "Verifying arch-specific items are NOT linked"
    if grep -q "Would link.*config/pacman\.conf" "$log_file"; then
      echo "Error: Should not link pacman.conf in desktop profile"
      exit 1
    fi
    if grep -q "Would link.*xmonad" "$log_file"; then
      echo "Error: Should not link xmonad in desktop profile"
      exit 1
    fi
    if grep -q "Would link.*Xresources" "$log_file"; then
      echo "Error: Should not link Xresources in desktop profile"
      exit 1
    fi
    echo "✓ Arch-specific items properly excluded"

    echo "Verifying Windows items are NOT linked"
    if grep -q "Would link.*WindowsTerminal" "$log_file"; then
      echo "Error: Should not link WindowsTerminal in desktop profile"
      exit 1
    fi
    if grep -q "Would link.*AppData" "$log_file"; then
      echo "Error: Should not link AppData items in desktop profile"
      exit 1
    fi
    echo "✓ Windows items properly excluded"
    ;;

  *)
    echo "Error: Unknown profile '$profile'"
    echo "Supported profiles: base, arch, arch-desktop, desktop"
    exit 1
    ;;
esac

echo "All $profile profile assertions passed!"
