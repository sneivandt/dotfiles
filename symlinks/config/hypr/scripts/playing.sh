#!/bin/sh
set -o errexit
set -o nounset

status="$(playerctl status --player=spotify 2>/dev/null || true)"
case "$status" in
  Playing|Paused) ;;
  *)
    printf '{"text":"","tooltip":"","class":"stopped"}\n'
    exit 0
    ;;
esac

summary="$(playerctl metadata --player=spotify --format '{{ artist }} - {{ title }}')"
details="$(playerctl metadata --player=spotify --format '{{ artist }} - {{ album }} - {{ title }}')"
case "$summary" in
  " - "*) summary=$(printf '%s\n' "$summary" | cut -c4-) ;;
esac
case "$details" in
  " - "*) details=$(printf '%s\n' "$details" | cut -c4-) ;;
esac

summary=$(printf '%s\n' "$summary" | awk -v len=72 \
  '{ if (length($0) > len) print substr($0, 1, len-3) "..."; else print; }')
class=$(printf '%s' "$status" | tr '[:upper:]' '[:lower:]')
jq -nc --arg text "$summary" --arg tooltip "$details" --arg class "$class" \
  'def pango_escape:
     gsub("&"; "&amp;") | gsub("<"; "&lt;") | gsub(">"; "&gt;");
   {
     text:("&#xf001; " + ($text | pango_escape)),
     tooltip:($tooltip | pango_escape),
     class:$class
   }'
