#!/bin/sh
set -o errexit
set -o nounset

if ! command -v jq >/dev/null 2>&1; then
  echo "jq not found" >&2
  exit 1
fi

if [ -f "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper ]; then
  feh --bg-fill --no-fehbg "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
else
  feh --bg-fill --no-fehbg "${XDG_CONFIG_HOME:-$HOME/.config}"/wallpaper/default.png
fi

tmpfile="$(mktemp)"
trap 'rm -f "$tmpfile"' EXIT

# Fetch top wallpapers from r/wallpapers for the past month
response=$(curl -sSL -H "User-Agent: wallpaper-script/1.0" \
  "https://www.reddit.com/r/wallpapers/top.json?t=month&limit=50")

# Pick the highest upvoted direct image link from the top posts
# Strip control characters that Reddit sometimes includes in JSON responses
url=$(printf '%s' "$response" | tr -d '\000-\011\013-\037' | jq -r '
  [.data.children[].data
  | select(.post_hint == "image")
  | select(.url | test("\\.(jpg|jpeg|png)$"; "i"))
  | .url] | first' )

if [ -n "$url" ] && [ "$url" != "null" ]; then
  if curl -sfSL "$url" > "$tmpfile" && [ -s "$tmpfile" ]; then
    mv "$tmpfile" "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
    feh --bg-fill --no-fehbg "${XDG_CACHE_HOME:-$HOME/.cache}"/wallpaper
  fi
fi
