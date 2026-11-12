#!/bin/sh
# Run only in the disposable Arch CI container; no host devices are exposed.
set -eu
test -e /.dockerenv
test "$(id -u)" = 0
test -e /etc/arch-release

pacman -Syu --noconfirm --needed sway swaylock quickshell qt6-wayland mesa dbus python hyprland hyprlock
# The headless parent needs no real-time scheduling capability.
sway_binary=$(command -v sway)
if [ -n "$(getcap "$sway_binary")" ]; then setcap -r "$sway_binary"; fi
useradd -m smoke
exec runuser -u smoke -- env HOME=/home/smoke \
  dbus-run-session -- python /workspace/.github/workflows/scripts/linux/test-desktop-session.py "$@"
