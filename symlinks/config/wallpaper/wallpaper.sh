#!/bin/sh
set -o errexit
set -o nounset

if ! command -v jq >/dev/null 2>&1 || ! command -v xdpyinfo >/dev/null 2>&1; then
  echo "jq or xdpyinfo not found" >&2
  exit 1
fi

if [ -f "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper ]; then
  feh --bg-fill --no-fehbg "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
else
  feh --bg-fill --no-fehbg "${XDG_CONFIG_HOME:-$HOME/.config}"/wallpaper/default.png
fi

tmpfile="$(mktemp)"
query="abstract+shapes+dark"

# Exclude people and text explicitly in the query to be safe
query="$query+-people+-women+-men+-model+-text+-quote+-quotes+-typography"

# Fetch list of wallpapers
# categories=100 ensures General only (no Anime/People categories)
# purity=100 ensures SFW only
response=$(curl -sSL "https://wallhaven.cc/api/v1/search?sorting=random&purity=100&categories=100&atleast=$(xdpyinfo | awk '/dimensions/{print $2}')&q=$query")

url=$(echo "$response" | jq -r '.data as $d | ($d | map(select(.favorites >= 1000))) | if length > 0 then . else $d end | .[].path' | shuf -n 1)

if [ -n "$url" ] && [ "$url" != "null" ]; then
  curl -sfSL "$url" > "$tmpfile"
  mv "$tmpfile" "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
  feh - --bg-fill --no-fehbg < "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
fi
