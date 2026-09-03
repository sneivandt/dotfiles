#!/bin/sh
set -o errexit
set -o nounset

# Stock/crypto ticker for the desktop bar.
# Fetches quotes from Yahoo Finance and caches them to limit API calls.
# Outputs structured JSON for Quickshell.

quotes='
MSFT|MSFT|Microsoft|$
TSLA|TSLA|Tesla|$
GOOG|GOOG|Alphabet|$
SPCX|SPCX|Procure Space ETF|$
BTC-USD|BTC|Bitcoin|$
'
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/quickshell-stocks"
cache_file="$cache_dir/quotes.json"
lock_dir="$cache_dir/quotes-prices.lock"
reap_dir="$lock_dir.reap"
cache_ttl=300
tmp_file=""
lock_owned=0

empty_output() {
  printf '{"quotes":[],"updated":0}'
}

cached_or_empty() {
  if [ -s "$cache_file" ]; then
    jq -c '{quotes:(.quotes // []),updated:(.updated // 0)}' \
      "$cache_file" 2>/dev/null || empty_output
  else
    empty_output
  fi
}

cleanup() {
  if [ -n "$tmp_file" ] && [ -f "$tmp_file" ]; then
    rm -f "$tmp_file"
  fi
  lock_pid=$(cat "$lock_dir/pid" 2>/dev/null || true)
  if [ "$lock_owned" -eq 1 ] && [ "$lock_pid" = "$$" ]; then
    rm -f "$lock_dir/pid"
    rmdir "$lock_dir" 2>/dev/null || true
  fi
}

write_lock_pid() {
  if printf '%s\n' "$$" > "$lock_dir/pid"; then
    lock_owned=1
    return 0
  fi

  rmdir "$lock_dir" 2>/dev/null || true
  return 1
}

release_reap_lock() {
  rmdir "$reap_dir" 2>/dev/null || true
}

acquire_lock() {
  if mkdir "$lock_dir" 2>/dev/null; then
    write_lock_pid
    return
  fi

  # Only one process may inspect and replace a stale lock at a time.
  if ! mkdir "$reap_dir" 2>/dev/null; then
    return 1
  fi

  # The previous owner may have released the lock before the reaper was acquired.
  if mkdir "$lock_dir" 2>/dev/null; then
    result=1
    if write_lock_pid; then
      result=0
    fi
    release_reap_lock
    return "$result"
  fi

  lock_pid=$(cat "$lock_dir/pid" 2>/dev/null || true)
  case "$lock_pid" in
    ''|*[!0-9]*)
      lock_mtime=$(stat -c %Y "$lock_dir" 2>/dev/null || echo "$now")
      if [ "$((now - lock_mtime))" -lt "$cache_ttl" ]; then
        release_reap_lock
        return 1
      fi
      ;;
    *)
      if kill -0 "$lock_pid" 2>/dev/null; then
        release_reap_lock
        return 1
      fi
      ;;
  esac

  if ! rm -f "$lock_dir/pid" 2>/dev/null ||
     ! rmdir "$lock_dir" 2>/dev/null ||
     ! mkdir "$lock_dir" 2>/dev/null; then
    release_reap_lock
    return 1
  fi

  result=1
  if write_lock_pid; then
    result=0
  fi
  release_reap_lock
  return "$result"
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
  cached_or_empty
  exit 0
fi

if ! acquire_lock; then
  cached_or_empty
  exit 0
fi
trap cleanup EXIT HUP INT TERM

quotes_json='[]'
while IFS='|' read -r query symbol name price_prefix; do
  if [ -z "$query" ]; then
    continue
  fi

  json=$(curl -fsS --max-time 4 -H "User-Agent: Mozilla/5.0" \
    "https://query1.finance.yahoo.com/v8/finance/chart/$query?interval=1d&range=1d" 2>/dev/null || true)
  if [ -z "$json" ]; then
    continue
  fi
  price=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.regularMarketPrice // empty' 2>/dev/null || true)
  prev=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.chartPreviousClose // .chart.result[0].meta.previousClose // empty' 2>/dev/null || true)
  if [ -z "$price" ] || [ "$price" = "null" ] || [ -z "$prev" ] || [ "$prev" = "null" ]; then
    continue
  fi

  change=$(awk -v p="$price" -v c="$prev" '
    BEGIN {
      pct = (p - c) / c * 100;
      printf "%.4f", pct;
    }')

  quotes_json=$(jq -cn \
    --argjson current "$quotes_json" \
    --arg symbol "$symbol" \
    --arg name "$name" \
    --arg prefix "$price_prefix" \
    --argjson price "$price" \
    --argjson change "$change" \
    '$current + [{
      symbol:$symbol,
      name:$name,
      prefix:$prefix,
      price:$price,
      change:$change
    }]')
done <<EOF
$quotes
EOF

if [ "$quotes_json" = '[]' ]; then
  cached_or_empty
  exit 0
fi

out=$(jq -nc --argjson quotes "$quotes_json" --argjson updated "$now" \
  '{quotes:$quotes,updated:$updated}')
tmp_file=$(mktemp "$cache_dir/.quotes-prices.XXXXXX")
printf '%s' "$out" > "$tmp_file"
mv -f "$tmp_file" "$cache_file"
tmp_file=""
printf '%s' "$out"
