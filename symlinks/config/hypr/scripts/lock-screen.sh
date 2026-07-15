#!/bin/sh
set -o errexit
set -o nounset

if pidof hyprlock >/dev/null 2>&1 || systemctl --user --quiet is-active hyprlock.service; then
  exit 0
fi

if ! command -v systemd-run >/dev/null 2>&1 || [ ! -x /usr/bin/hyprlock ]; then
  logger -t lock-screen "systemd-run or hyprlock not found"
  exit 1
fi

exec systemd-run \
  --user \
  --quiet \
  --collect \
  --unit=hyprlock \
  --property=Type=exec \
  --property=NoNewPrivileges=no \
  --property=Restart=on-abnormal \
  --property=RestartSec=1 \
  /usr/bin/hyprlock --no-fade-in
