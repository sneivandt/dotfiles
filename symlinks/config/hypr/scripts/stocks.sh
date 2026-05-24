#!/bin/sh
set -o errexit
set -o nounset

# Stock/crypto ticker for waybar.
# Fetches quotes from Yahoo Finance and caches them to limit API calls.
# Outputs Waybar JSON with Pango markup (price plus red/green % change).

quotes='
%5EGSPC|S&amp;P|
MSFT|MSFT|$
BTC-USD|BTC|$
'
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/waybar-stocks"
cache_file="$cache_dir/quotes-sp500-prices.json"
lock_dir="$cache_dir/quotes-sp500-prices.lock"
cache_ttl=300
tmp_file=""

empty_output() {
  printf '{"text":"","tooltip":""}'
}

cached_or_empty() {
  if [ -s "$cache_file" ]; then
    cat "$cache_file"
  else
    empty_output
  fi
}

cleanup() {
  if [ -n "$tmp_file" ] && [ -f "$tmp_file" ]; then
    rm -f "$tmp_file"
  fi
  if [ -d "$lock_dir" ]; then
    rmdir "$lock_dir" 2>/dev/null || true
  fi
}

mkdir -p "$cache_dir"

for cmd in curl jq awk stat mktemp; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    cached_or_empty
    exit 0
  fi
done

now=$(date +%s)
mtime=0
if [ -f "$cache_file" ]; then
  mtime=$(stat -c %Y "$cache_file" 2>/dev/null || echo 0)
fi

if [ "$((now - mtime))" -lt "$cache_ttl" ] && [ -s "$cache_file" ]; then
  cat "$cache_file"
  exit 0
fi

if ! mkdir "$lock_dir" 2>/dev/null; then
  cached_or_empty
  exit 0
fi
trap cleanup EXIT HUP INT TERM

parts=""
while IFS='|' read -r sym label price_prefix; do
  if [ -z "$sym" ]; then
    continue
  fi

  json=$(curl -fsS --max-time 4 -H "User-Agent: Mozilla/5.0" \
    "https://query1.finance.yahoo.com/v8/finance/chart/$sym?interval=1d&range=1d" 2>/dev/null || true)
  if [ -z "$json" ]; then
    continue
  fi
  price=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.regularMarketPrice // empty' 2>/dev/null || true)
  prev=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.chartPreviousClose // .chart.result[0].meta.previousClose // empty' 2>/dev/null || true)
  if [ -z "$price" ] || [ "$price" = "null" ] || [ -z "$prev" ] || [ "$prev" = "null" ]; then
    continue
  fi

  formatted=$(awk -v p="$price" -v c="$prev" -v l="$label" -v prefix="$price_prefix" '
    BEGIN {
      pct = (p - c) / c * 100;
      color = (pct >= 0) ? "#a3be8c" : "#bf616a";
      sign  = (pct >= 0) ? "+" : "";
      printf "%s %s%.2f <span color=\"%s\">%s%.2f%%</span>", l, prefix, p, color, sign, pct;
    }')

  if [ -z "$parts" ]; then
    parts="$formatted"
  else
    parts="$parts  $formatted"
  fi
done <<EOF
$quotes
EOF

if [ -z "$parts" ]; then
  cached_or_empty
  exit 0
fi

out=$(jq -nc --arg t "$parts" '{text:$t, tooltip:"", class:"stocks"}')
tmp_file=$(mktemp "$cache_dir/.quotes-sp500-prices.XXXXXX")
printf '%s' "$out" > "$tmp_file"
mv -f "$tmp_file" "$cache_file"
tmp_file=""
printf '%s' "$out"
