#!/bin/sh
set -o errexit
set -o nounset

DIR=${DIR:-$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)}
SCRIPT="$DIR/symlinks/config/hypr/scripts/stocks.sh"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

mkdir -p \
  "$TEST_ROOT/bin" \
  "$TEST_ROOT/home/.cache/quickshell-stocks/quotes-prices.lock"
printf '%s\n' 99999999 > \
  "$TEST_ROOT/home/.cache/quickshell-stocks/quotes-prices.lock/pid"

cat > "$TEST_ROOT/bin/curl" <<EOF
#!/bin/sh
printf x >> "$TEST_ROOT/curl-calls"
sleep 0.1
exit 1
EOF

cat > "$TEST_ROOT/bin/jq" <<'EOF'
#!/bin/sh
exit 0
EOF

cat > "$TEST_ROOT/bin/mkdir" <<'EOF'
#!/bin/sh
case "$*" in
  *quotes-prices.lock*) sleep 0.02 ;;
esac
exec /usr/bin/mkdir "$@"
EOF

cat > "$TEST_ROOT/bin/rmdir" <<'EOF'
#!/bin/sh
case "$*" in
  *quotes-prices.lock*) sleep 0.02 ;;
esac
exec /usr/bin/rmdir "$@"
EOF

chmod +x "$TEST_ROOT/bin/"*

pids=""
i=0
while [ "$i" -lt 20 ]; do
  PATH="$TEST_ROOT/bin:/usr/bin:/bin" \
    HOME="$TEST_ROOT/home" \
    XDG_CACHE_HOME="$TEST_ROOT/home/.cache" \
    sh "$SCRIPT" >/dev/null &
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
