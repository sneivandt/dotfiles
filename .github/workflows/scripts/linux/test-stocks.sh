#!/bin/sh
set -o errexit
set -o nounset

DIR=${DIR:-$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)}
SCRIPT="$DIR/symlinks/config/hypr/scripts/stocks.py"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

mkdir -p \
  "$TEST_ROOT/bin" \
  "$TEST_ROOT/home/.cache/quickshell-stocks/quotes-prices.lock" \
  "$TEST_ROOT/home/.cache/quickshell-stocks/quotes-prices.lock.reap"
# Orphaned directories from the old protocol must not block the kernel lock.
printf '%s\n' 99999999 > \
  "$TEST_ROOT/home/.cache/quickshell-stocks/quotes-prices.lock/pid"

cat > "$TEST_ROOT/bin/curl" <<EOF
#!/bin/sh
printf x >> "$TEST_ROOT/curl-calls"
# Hold the owner until every contender has observed the busy lock and exited.
# This avoids relying on process startup timing as fetch concurrency changes.
attempt=0
while [ "\$attempt" -lt 100 ]; do
  completed=0
  for result in "$TEST_ROOT"/result-*; do
    if [ -s "\$result" ]; then completed=\$((completed + 1)); fi
  done
  if [ "\$completed" -ge 19 ]; then exit 1; fi
  attempt=\$((attempt + 1))
  sleep 0.05
done
exit 2
EOF

cat > "$TEST_ROOT/bin/jq" <<'EOF'
#!/bin/sh
exit 0
EOF

chmod +x "$TEST_ROOT/bin/"*

pids=""
i=0
while [ "$i" -lt 20 ]; do
  PATH="$TEST_ROOT/bin:/usr/bin:/bin" \
    HOME="$TEST_ROOT/home" \
    XDG_CACHE_HOME="$TEST_ROOT/home/.cache" \
    python3 "$SCRIPT" >"$TEST_ROOT/result-$i" &
  pids="$pids $!"
  i=$((i + 1))
done

for pid in $pids; do
  wait "$pid"
done

calls=$(wc -c < "$TEST_ROOT/curl-calls")
if [ "$calls" -ne 5 ]; then
  printf 'ERROR: expected one stock refresh, observed %s curl calls\n' "$calls" >&2
  exit 1
fi

rm -rf "$TEST_ROOT/home/.cache/quickshell-stocks"
mkdir -p "$TEST_ROOT/data-bin"

cat > "$TEST_ROOT/data-bin/curl" <<'EOF'
#!/bin/sh
printf '%s\n' '{
  "chart": {
    "result": [{
      "meta": {
        "regularMarketPrice": 110,
        "regularMarketChangePercent": 5.5,
        "chartPreviousClose": 80,
        "fiftyTwoWeekLow": 70,
        "fiftyTwoWeekHigh": 120,
        "shortName": "Test Instrument "
      },
      "timestamp": [1000, 2000, 3000, 4000],
      "indicators": {
        "quote": [{"close": [80, null, 100, 110]}]
      }
    }]
  }
}'
EOF
chmod +x "$TEST_ROOT/data-bin/curl"

output=$(PATH="$TEST_ROOT/data-bin:/usr/bin:/bin" \
  HOME="$TEST_ROOT/home" \
  XDG_CACHE_HOME="$TEST_ROOT/home/.cache" \
  python3 "$SCRIPT")

printf '%s' "$output" | jq -e '
  .version == 4 and
  .updated > 0 and
  (.quotes | length == 5) and
  (.quotes[] |
    .updated > 0 and
    .price == 110 and
    .name == "Test Instrument" and
    .change == 5.5 and
    .history == [80, 100, 110] and
    .historyStart == 1000 and
    .yearChange == 37.5 and
    .yearLow == 70 and
    .yearHigh == 120)
' >/dev/null
