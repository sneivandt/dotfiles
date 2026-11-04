#!/bin/sh
# Exercise the real Quickshell config on a disposable Wayland session.
set -eu
test -e /.dockerenv
test "$(id -u)" = 0
test -e /etc/arch-release

pacman -Syu --noconfirm --needed sway quickshell qt6-wayland mesa dbus
# The headless test needs no real-time scheduling capability.
sway_binary=$(command -v sway)
if [ -n "$(getcap "$sway_binary")" ]; then setcap -r "$sway_binary"; fi
useradd -m smoke
install -d -m 0700 -o smoke -g smoke /home/smoke/runtime
printf 'output * mode 1280x720\n' > /tmp/sway-ci.conf
# shellcheck disable=SC2016
exec runuser -u smoke -- env HOME=/home/smoke XDG_RUNTIME_DIR=/home/smoke/runtime \
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  QT_QUICK_BACKEND=software dbus-run-session -- sh -eu -c '
    sway -c /tmp/sway-ci.conf >/tmp/sway-ci.log 2>&1 &
    compositor=$!
    shell=
    cleanup() {
      if [ -n "$shell" ]; then kill "$shell" 2>/dev/null || :; fi
      kill "$compositor" 2>/dev/null || :
      cat /tmp/sway-ci.log
      if [ -f /tmp/quickshell-ci.log ]; then cat /tmp/quickshell-ci.log; fi
    }
    trap cleanup EXIT
    export WAYLAND_DISPLAY=wayland-1
    for try in $(seq 1 20); do
      if [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then break; fi
      kill -0 "$compositor"
      sleep 1
    done
    test -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
    quickshell --no-duplicate --path /workspace/symlinks/config/quickshell >/tmp/quickshell-ci.log 2>&1 &
    shell=$!
    for try in $(seq 1 20); do
      kill -0 "$shell"
      if grep -Fq "Configuration Loaded" /tmp/quickshell-ci.log; then
        sleep 2
        kill -0 "$shell"
        exit 0
      fi
      sleep 1
    done
    exit 1
  '
