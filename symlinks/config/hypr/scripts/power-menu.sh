#!/bin/sh
set -o errexit
set -o nounset

choice=$(printf 'Log out\nShut down\n' | fuzzel --dmenu --prompt='Power: ')

case "$choice" in
  "Log out")
    hyprctl dispatch 'hl.dsp.exit()'
    ;;
  "Shut down")
    systemctl poweroff
    ;;
esac
