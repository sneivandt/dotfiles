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
SPCX|SPCX|SpaceX|$
BTC-USD|BTC|Bitcoin|$
'
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/quickshell-stocks"
cache_file="$cache_dir/quotes.json"
lock_dir="$cache_dir/quotes-prices.lock"
reap_dir="$lock_dir.reap"
cache_ttl=300
cache_version=3
tmp_file=""
lock_owned=0

empty_output() {
  printf '{"quotes":[],"updated":0}'
}

cached_or_empty() {
  if [ -s "$cache_file" ] &&
     jq -e --argjson version "$cache_version" \
       '.version == $version' "$cache_file" >/dev/null 2>&1; then
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

if [ "$((now - mtime))" -lt "$cache_ttl" ] && [ -s "$cache_file" ] &&
   jq -e --argjson version "$cache_version" \
     '.version == $version' "$cache_file" >/dev/null 2>&1; then
  cached_or_empty
  exit 0
fi

if ! acquire_lock; then
  cached_or_empty
  exit 0
fi
trap cleanup EXIT HUP INT TERM

quotes_json='[]'
while IFS='|' read -r query symbol fallback_name price_prefix; do
  if [ -z "$query" ]; then
    continue
  fi

  json=$(curl -fsS --max-time 4 -H "User-Agent: Mozilla/5.0" \
    "https://query1.finance.yahoo.com/v8/finance/chart/$query?interval=1wk&range=1y" 2>/dev/null || true)
  if [ -z "$json" ]; then
    continue
  fi
  name=$(printf '%s' "$json" | jq -r \
    '(.chart.result[0].meta.shortName // .chart.result[0].meta.longName // empty) | gsub("^\\s+|\\s+$"; "")' \
    2>/dev/null || true)
  if [ -z "$name" ]; then
    name=$fallback_name
  fi
  price=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.regularMarketPrice // empty' 2>/dev/null || true)
  change=$(printf '%s' "$json" | jq -r '.chart.result[0].meta.regularMarketChangePercent // empty' 2>/dev/null || true)
  if [ -z "$price" ] || [ "$price" = "null" ] || [ -z "$change" ] || [ "$change" = "null" ]; then
    continue
  fi

  history=$(printf '%s' "$json" | jq -c \
    '[.chart.result[0].indicators.quote[0].close[]? | select(. != null)]' \
    2>/dev/null || printf '[]')
  if [ "$history" = '[]' ]; then
    history=$(jq -cn --argjson price "$price" '[$price]')
  fi
  history_start=$(printf '%s' "$json" | jq -r '
    .chart.result[0] as $result |
    [range(0; ($result.timestamp | length)) as $index |
      select($result.indicators.quote[0].close[$index] != null) |
      $result.timestamp[$index]][0] // $result.meta.firstTradeDate // empty
  ' 2>/dev/null || true)
  if [ -z "$history_start" ] || [ "$history_start" = "null" ]; then
    history_start=$now
  fi
  year_open=$(printf '%s' "$history" | jq -r 'first // empty')
  year_low=$(printf '%s' "$json" | jq -r \
    '.chart.result[0].meta.fiftyTwoWeekLow // empty' 2>/dev/null || true)
  year_high=$(printf '%s' "$json" | jq -r \
    '.chart.result[0].meta.fiftyTwoWeekHigh // empty' 2>/dev/null || true)
  if [ -z "$year_low" ] || [ "$year_low" = "null" ]; then
    year_low=$(printf '%s' "$history" | jq -r 'min // empty')
  fi
  if [ -z "$year_high" ] || [ "$year_high" = "null" ]; then
    year_high=$(printf '%s' "$history" | jq -r 'max // empty')
  fi
  year_change=$(awk -v p="$price" -v o="$year_open" '
    BEGIN {
      if (o == 0) {
        printf "0.0000";
      } else {
        printf "%.4f", (p - o) / o * 100;
      }
    }')

  quotes_json=$(jq -cn \
    --argjson current "$quotes_json" \
    --arg symbol "$symbol" \
    --arg name "$name" \
    --arg prefix "$price_prefix" \
    --argjson price "$price" \
    --argjson change "$change" \
    --argjson history "$history" \
    --argjson historyStart "$history_start" \
    --argjson yearChange "$year_change" \
    --argjson yearLow "$year_low" \
    --argjson yearHigh "$year_high" \
    '$current + [{
      symbol:$symbol,
      name:$name,
      prefix:$prefix,
      price:$price,
      change:$change,
      history:$history,
      historyStart:$historyStart,
      yearChange:$yearChange,
      yearLow:$yearLow,
      yearHigh:$yearHigh
    }]')
done <<EOF
$quotes
EOF

if [ "$quotes_json" = '[]' ]; then
  cached_or_empty
  exit 0
fi

out=$(jq -nc \
  --argjson version "$cache_version" \
  --argjson quotes "$quotes_json" \
  --argjson updated "$now" \
  '{version:$version,quotes:$quotes,updated:$updated}')
tmp_file=$(mktemp "$cache_dir/.quotes-prices.XXXXXX")
printf '%s' "$out" > "$tmp_file"
mv -f "$tmp_file" "$cache_file"
tmp_file=""
printf '%s' "$out"
