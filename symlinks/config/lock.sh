#!/bin/sh
set -o errexit
set -o nounset

if command -v playerctl >/dev/null 2>&1 && command -v amixer >/dev/null 2>&1; then

  # Pause player
  STATUS="$(playerctl status 2>/dev/null || true)"
  playerctl pause 2>/dev/null || true

  # Mute
  SOUND="$(amixer sget Master | grep -E -o "\[on\]" | head -n 1 || true)"
  amixer -q sset Master mute 2>/dev/null || true

  # Lock and wait
  slock || true

  # Unmute
  if [ "$SOUND" = "[on]" ]; then
    amixer -q sset Master unmute 2>/dev/null || true
    amixer scontrols | awk -F "'" '{print $2}' | while read -r channel
    do
      amixer -q sset "$channel" unmute 2>/dev/null || true
    done
  fi

  # Resume player
  if [ "$STATUS" = "Playing" ]; then
    playerctl play 2>/dev/null || true
  fi

elif command -v slock >/dev/null 2>&1; then

  # Fallback 1
  slock

elif command -v xsecurelock >/dev/null 2>&1; then

  # Fallback 2
  xsecurelock

fi
