#!/bin/sh

# xmonad (X11 fallback)
export XMONAD_CONFIG_DIR="$HOME"/.config/xmonad
export XMONAD_DATA_DIR="$HOME"/.config/xmonad
export XMONAD_CACHE_DIR="$HOME"/.config/xmonad

# Wayland/Hyprland
if [ "$XDG_SESSION_TYPE" = "wayland" ]; then
  export MOZ_ENABLE_WAYLAND=1
  export QT_QPA_PLATFORM=wayland
fi
