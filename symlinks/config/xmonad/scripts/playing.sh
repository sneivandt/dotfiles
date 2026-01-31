#!/bin/sh
set -o errexit
set -o nounset

if [ "$(playerctl status --player=spotify 2>/dev/null || true)" = "Playing" ]; then
  metadata="$(playerctl metadata --player=spotify --format '{{ artist }} - {{ album }} - {{ title }}')"
  case "$metadata" in
    " - "*) metadata=$(echo "$metadata" | cut -c4-)
  esac
  echo "${1:-} $metadata" | awk -v len=128 '{ if (length($0) > len) print substr($0, 1, len-3) "..."; else print; }'
fi