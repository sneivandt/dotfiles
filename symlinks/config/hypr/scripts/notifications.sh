#!/bin/sh
set -o nounset

# Notification indicator for waybar — shows pending mako notification count
count=$(makoctl list -j 2>/dev/null | jq 'length' 2>/dev/null || true)
count=${count:-0}

if [ "$count" -gt 0 ] 2>/dev/null; then
  printf '{"text": "%s", "tooltip": "%s notification(s)", "class": "has-notifications"}\n' \
    "$count" "$count"
else
  printf '{"text": "", "tooltip": "No notifications", "class": ""}\n'
fi
