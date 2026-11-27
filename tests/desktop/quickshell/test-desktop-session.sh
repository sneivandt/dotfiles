#!/bin/sh
# Run only in the disposable Arch CI container; no host devices are exposed.
set -eu
test -e /.dockerenv
test "$(id -u)" = 0
test -e /etc/arch-release

pacman -Syu --noconfirm --needed sway swaylock quickshell qt6-wayland mesa dbus python hyprland hyprlock \
  systemd gammastep glib2 hypridle hyprpaper mako network-manager-applet xfconf \
  fuzzel alacritty lazygit btop
# The headless parent needs no real-time scheduling capability.
sway_binary=$(command -v sway)
if [ -n "$(getcap "$sway_binary")" ]; then setcap -r "$sway_binary"; fi
useradd -m smoke
# The workflow verifies the first-login unit without launching Dotfiles.
install -d -o smoke -g smoke /home/smoke/.local/bin
ln -s /usr/bin/true /home/smoke/.local/bin/dotfiles
exec runuser -u smoke -- env HOME=/home/smoke \
  dbus-run-session -- python /workspace/tests/desktop/quickshell/test_desktop_session.py "$@"
