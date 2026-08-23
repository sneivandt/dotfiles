#!/bin/sh
set -o errexit
set -o nounset

# Stock/crypto ticker for waybar.
# Fetches quotes from Yahoo Finance and caches them to limit API calls.
# Outputs Waybar JSON with Pango markup (price plus red/green % change).

quotes='
MSFT|&#xf3ca;|$
BTC-USD|&#xf15a;|$
'
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/waybar-stocks"
cache_file="$cache_dir/quotes.json"
lock_dir="$cache_dir/quotes-prices.lock"
reap_dir="$lock_dir.reap"
collapsed_file="$cache_dir/collapsed"
cache_ttl=300
tmp_file=""
lock_owned=0

empty_output() {
  printf '{"text":"","tooltip":""}'
}

render() {
  parts="$1"
  if [ -z "$parts" ]; then
    empty_output
  elif [ -f "$collapsed_file" ]; then
    jq -nc --arg tooltip "$parts" \
      '{text:"&#xf201;", tooltip:$tooltip, class:"collapsed"}'
  else
    jq -nc --arg text "$parts" \
      '{text:$text, tooltip:"", class:"expanded"}'
  fi
}

cached_or_empty() {
  if [ -s "$cache_file" ]; then
    render "$(jq -r '.quotes // empty' "$cache_file" 2>/dev/null || true)"
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

if [ "${1:-}" = "--toggle" ]; then
  if [ -f "$collapsed_file" ]; then
    rm -f "$collapsed_file"
  else
    : > "$collapsed_file"
  fi
  systemctl --user kill --signal=SIGRTMIN+2 --kill-whom=main \
    waybar.service >/dev/null 2>&1 || true
  exit 0
fi

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
      color = (pct >= 0) ? "#9ece6a" : "#f7768e";
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

out=$(jq -nc --arg quotes "$parts" '{quotes:$quotes}')
tmp_file=$(mktemp "$cache_dir/.quotes-prices.XXXXXX")
printf '%s' "$out" > "$tmp_file"
mv -f "$tmp_file" "$cache_file"
tmp_file=""
render "$parts"
