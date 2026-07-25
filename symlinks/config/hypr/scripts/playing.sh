#!/bin/sh
set -o errexit
set -o nounset

# Playing indicator for waybar
if [ "$(playerctl status --player=spotify 2>/dev/null || true)" = "Playing" ]; then
  metadata="$(playerctl metadata --player=spotify --format '{{ artist }} - {{ album }} - {{ title }}')"
  case "$metadata" in
    " - "*) metadata=$(echo "$metadata" | cut -c4-)
  esac
  # Truncate first, then escape, so truncation can never split an entity.
  metadata=$(echo "$metadata" | awk -v len=128 '{ if (length($0) > len) print substr($0, 1, len-3) "..."; else print; }')
  # Waybar renders module text as Pango markup, so &, < and > must be escaped
  # or the module fails to render (e.g. artists like "Simon & Garfunkel").
  printf '%s\n' "$metadata" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
fi
