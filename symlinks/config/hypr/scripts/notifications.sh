#!/bin/sh
set -o errexit
set -o nounset

# Notification indicator for waybar — shows pending mako notification count
count=$(makoctl list 2>/dev/null | jq '[.data[][]] | length' 2>/dev/null || echo 0)

if [ "$count" -gt 0 ]; then
  printf '{"text": "%s", "tooltip": "%s notification(s)", "class": "has-notifications"}\n' \
    "$count" "$count"
else
  printf '{"text": "", "tooltip": "No notifications", "class": ""}\n'
fi
