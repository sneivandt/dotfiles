#!/bin/sh

# Wayland/Hyprland
export MOZ_ENABLE_WAYLAND=1
# Respect explicit Qt backends and leave non-Wayland sessions to Qt's default.
if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -z "${QT_QPA_PLATFORM:-}" ]; then
  export QT_QPA_PLATFORM='wayland;xcb'
fi
