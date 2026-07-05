#!/bin/sh
set -o errexit
set -o nounset

if pidof hyprlock >/dev/null 2>&1; then
  exit 0
fi

if ! command -v hyprlock >/dev/null 2>&1; then
  logger -t lock-screen "hyprlock not found"
  exit 1
fi

if command -v hyprctl >/dev/null 2>&1; then
  hyprctl dispatch dpms on >/dev/null 2>&1 || logger -t lock-screen "failed to wake displays before locking"
fi

exec hyprlock --immediate-render --no-fade-in
