#!/bin/sh
set -o errexit
set -o nounset

DIR=${DIR:-$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)}
SCRIPT="$DIR/symlinks/config/hypr/scripts/playing.sh"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

mkdir -p "$TEST_ROOT/bin"
cat > "$TEST_ROOT/bin/playerctl" <<'EOF'
#!/bin/sh
case "$*" in
  *status*) printf '%s\n' Playing ;;
  *album*) printf '%s\n' 'AC & DC - Back <Black> - Rock > Roll' ;;
  *metadata*) printf '%s\n' 'AC & DC - Rock <Roll>' ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$TEST_ROOT/bin/playerctl"

output=$(PATH="$TEST_ROOT/bin:/usr/bin:/bin" sh "$SCRIPT")
text=$(printf '%s\n' "$output" | jq -r .text)
tooltip=$(printf '%s\n' "$output" | jq -r .tooltip)
class=$(printf '%s\n' "$output" | jq -r .class)

if [ "$text" != '&#xf001; AC &amp; DC - Rock &lt;Roll&gt;' ]; then
  printf 'ERROR: unexpected playing text: %s\n' "$text" >&2
  exit 1
fi
if [ "$tooltip" != 'AC &amp; DC - Back &lt;Black&gt; - Rock &gt; Roll' ]; then
  printf 'ERROR: unexpected playing tooltip: %s\n' "$tooltip" >&2
  exit 1
fi
if [ "$class" != 'playing' ]; then
  printf 'ERROR: unexpected playing class: %s\n' "$class" >&2
  exit 1
fi
