#!/bin/sh
set -eu

case "${1:-}" in
  up) change=+5% ;;
  down) change=5%- ;;
  *) printf 'Usage: %s up|down\n' "$0" >&2; exit 2 ;;
esac

# A desktop may have keyboard LEDs but no display backlight. Do nothing there.
# Restrict brightnessctl to display backlights so it cannot select a keyboard LED.
for device in /sys/class/backlight/*; do
  [ -d "$device" ] || continue
  brightnessctl --class=backlight --min-value=1 set "$change"
  exit 0
done
