#!/bin/sh
set -o errexit
set -o nounset

# Stock/crypto ticker for waybar.
# Fetches quotes from Yahoo Finance and caches them to limit API calls.
# Outputs Waybar JSON with Pango markup (red/green % change).

symbols="SPY MSFT BTC-USD"
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/waybar-stocks"
cache_file="$cache_dir/quotes.json"
cache_ttl=300

mkdir -p "$cache_dir"

now=$(date +%s)
mtime=0
if [ -f "$cache_file" ]; then
  mtime=$(stat -c %Y "$cache_file" 2>/dev/null || echo 0)
fi

if [ "$((now - mtime))" -lt "$cache_ttl" ] && [ -s "$cache_file" ]; then
  cat "$cache_file"
  exit 0
fi

parts=""
for sym in $symbols; do
  json=$(curl -fsS --max-time 4 -H "User-Agent: Mozilla/5.0" \
    "https://query1.finance.yahoo.com/v8/finance/chart/$sym?interval=1d&range=1d" 2>/dev/null || true)
  if [ -z "$json" ]; then
    continue
  fi
  price=$(echo "$json" | jq -r '.chart.result[0].meta.regularMarketPrice // empty' 2>/dev/null || true)
  prev=$(echo "$json" | jq -r '.chart.result[0].meta.chartPreviousClose // .chart.result[0].meta.previousClose // empty' 2>/dev/null || true)
  if [ -z "$price" ] || [ "$price" = "null" ] || [ -z "$prev" ] || [ "$prev" = "null" ]; then
    continue
  fi

  label=$(echo "$sym" | sed 's/-USD$//')
  formatted=$(awk -v p="$price" -v c="$prev" -v l="$label" '
    BEGIN {
      pct = (p - c) / c * 100;
      color = (pct >= 0) ? "#a3be8c" : "#bf616a";
      sign  = (pct >= 0) ? "+" : "";
      printf "%s <span color=\"%s\">%s%.2f%%</span>", l, color, sign, pct;
    }')

  if [ -z "$parts" ]; then
    parts="$formatted"
  else
    parts="$parts  $formatted"
  fi
done

if [ -z "$parts" ]; then
  printf '{"text":"","tooltip":""}'
  exit 0
fi

out=$(jq -nc --arg t "$parts" '{text:$t, tooltip:"", class:"stocks"}')
printf '%s' "$out" > "$cache_file"
printf '%s' "$out"
