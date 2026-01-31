#!/bin/sh
set -o errexit
set -o nounset

vol=$(amixer sget Master | awk -F "[][]" '/dB/ { print $2 }')

if [ "$(amixer sget Master | awk -F "[][]" '/dB/ { print $6 }')" = "on" ]; then
  echo "${1:-} $vol"
else
  echo "${2:-} $vol"
fi