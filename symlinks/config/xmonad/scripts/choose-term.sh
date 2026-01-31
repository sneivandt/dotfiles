#!/bin/sh
set -o errexit
set -o nounset

for terminal in urxvt urxvt256c uxterm xterm gnome-terminal
do
  if command -v "$terminal" >/dev/null 2>&1
  then
    if [ "${1:-}" = "--class" ] && { [ "$terminal" = "urxvt" ] || [ "$terminal" = "xterm" ] || [ "$terminal" = "uxterm" ] || [ "$terminal" = "urxvt256c" ]; }; then
        shift
        exec $terminal -name "$@"
    else
        exec $terminal "$@"
    fi
  fi
done
unset terminal
