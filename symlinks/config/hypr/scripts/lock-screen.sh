#!/bin/sh
set -o errexit
set -o nounset

if pgrep -u "$(id -u)" -x hyprlock >/dev/null 2>&1; then
  exit 0
fi

state=$(systemctl --user show -p ActiveState --value hyprlock.service 2>/dev/null || true)
case "$state" in
  active|activating) exit 0 ;;
  failed) systemctl --user reset-failed hyprlock.service >/dev/null 2>&1 || true ;;
esac

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
