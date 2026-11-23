#!/bin/sh
set -eu

export LC_ALL=C

case "${1:-}" in
  up) change=+5% ;;
  down) change=5%- ;;
  *) printf 'Usage: %s up|down\n' "$0" >&2; exit 2 ;;
esac

# A desktop may have keyboard LEDs but no display backlight. Do nothing there.
# Restrict brightnessctl to display backlights so it cannot select a keyboard LED.
for device in "${BACKLIGHT_DIR:-/sys/class/backlight}"/*; do
  [ -d "$device" ] || continue
  brightnessctl --class=backlight --quiet --min-value=1 set "$change"

  # Feedback is optional; a notification failure must not undo a brightness change.
  command -v notify-send >/dev/null 2>&1 || exit 0
  current=$(brightnessctl --class=backlight get) || exit 0
  maximum=$(brightnessctl --class=backlight max) || exit 0
  percent=$(awk -v current="$current" -v maximum="$maximum" 'BEGIN { if (maximum > 0) printf "%.0f", current * 100 / maximum }')
  [ -n "$percent" ] || exit 0
  notify-send --app-name=dotfiles --category=progress --expire-time=2000 \
    --hint="string:x-canonical-private-synchronous:dotfiles-brightness" \
    --hint="int:value:$percent" --icon=display-brightness-symbolic \
    "Brightness $percent%" || true
  exit 0
done
