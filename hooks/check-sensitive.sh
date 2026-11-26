#!/bin/sh
#
# Scans staged files for sensitive information like credentials, API keys,
# private keys, secrets, PII, and other data that should not be committed.
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-sensitive.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
NC=$(printf '\033[0m')

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FALLBACK_PATTERNS_FILE="$SCRIPT_DIR/sensitive-patterns.ini"
FALLBACK_ALLOWLIST_FILE="$SCRIPT_DIR/sensitive-allowlist.ini"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

load_hook_config() {
  rel_path="$1"
  fallback="$2"
  dest="$3"
  if git cat-file -e ":$rel_path" 2>/dev/null; then
    git show ":$rel_path" > "$dest"
    return 0
  fi
  if git cat-file -e "HEAD:$rel_path" 2>/dev/null; then
    # Tracked in HEAD and removed from the index: honor the staged deletion.
    return 1
  fi
  if [ -f "$fallback" ]; then
    cp "$fallback" "$dest"
    return 0
  fi
  return 1
}

PATTERNS_FILE="$tmpdir/sensitive-patterns.ini"
ALLOWLIST_FILE="$tmpdir/sensitive-allowlist.ini"

printf "Running sensitive content scan...\n"

if ! load_hook_config "hooks/sensitive-patterns.ini" "$FALLBACK_PATTERNS_FILE" "$PATTERNS_FILE"; then
  printf '%sERROR: Patterns file not found: %s%s\n' "$RED" "$FALLBACK_PATTERNS_FILE" "$NC"
  printf '%sCannot perform credential scanning without patterns file.%s\n' "$YELLOW" "$NC"
  exit 1
fi

PATTERNS=$(cat "$PATTERNS_FILE")

# Combine allowlist entries into a single alternation. Matching spans are
# structurally safe (see sensitive-allowlist.ini) and are redacted before
# scanning, so contributors are not pushed toward --no-verify. Redacting the
# span rather than dropping the line keeps the rest of the line scanned.
ALLOWLIST_RE=""
if load_hook_config "hooks/sensitive-allowlist.ini" "$FALLBACK_ALLOWLIST_FILE" "$ALLOWLIST_FILE"; then
  ALLOWLIST_RE=$(awk '
    /^$/ || /^#/ || /^\[.*\]$/ { next }
    {
      if (entries++) {
        printf "|"
      }
      printf "%s", $0
    }
    END {
      if (entries) {
        printf "\n"
      }
    }
  ' "$ALLOWLIST_FILE")
fi

# Validate every detection rule even when the staged diff is empty. grep's
# status 1 means no match; any other failure means the scan cannot be trusted.
while IFS= read -r pattern; do
  case "$pattern" in
    ''|'#'*|'['*']') continue ;;
  esac
  if grep -iE -- "$pattern" /dev/null >/dev/null 2>&1; then
    :
  else
    status=$?
    if [ "$status" -ne 1 ]; then
      printf 'ERROR: Cannot validate sensitive-data detection rules (grep status %s).\n' "$status" >&2
      exit 1
    fi
  fi
done <<PATTERNS_EOF
$PATTERNS
PATTERNS_EOF

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

found_secrets=0

diff_file="$tmpdir/diff"
content="$tmpdir/content"
linenos="$tmpdir/linenos"
filenames="$tmpdir/filenames"
: > "$content"
: > "$linenos"
: > "$filenames"

# Scan one staged patch rather than splitting filenames into shell records.
# Git quotes tabs/newlines in patch headers; keep those escaped names for
# diagnostics only. No pathname is decoded or passed back to Git. Taking the
# complete diff also preserves rename detection and staged-only content.
git diff --cached --unified=0 --inter-hunk-context=0 --no-color --no-ext-diff --no-textconv \
  --src-prefix=a/ --dst-prefix=b/ "$against" -- > "$diff_file"

# Keep content, source line numbers and display filenames in aligned files.
# Anchored detection rules bind to the added content itself, not metadata.
awk -v content="$content" -v linenos="$linenos" -v filenames="$filenames" '
  /^diff --git / { in_hunk = 0; next }
  !in_hunk && /^\+\+\+ / {
    file = substr($0, 5)
    sub(/\t$/, "", file)
    sub(/^b\//, "", file)
    sub(/^"b\//, "\"", file)
    next
  }
  /^@@ / {
    s = $3
    sub(/^\+/, "", s)
    sub(/,.*/, "", s)
    lineno = s + 0
    in_hunk = 1
    next
  }
  # The +++ header precedes the first hunk. Added content beginning with ++
  # still belongs to the scan once inside a hunk.
  in_hunk && /^\+/ {
    print substr($0, 2) > content
    print lineno > linenos
    print file > filenames
    lineno++
  }
' "$diff_file"

if [ -n "$ALLOWLIST_RE" ]; then
  # Detection is case-insensitive too, so lowercasing both sides preserves
  # matching without relying on sed's non-POSIX case-insensitive flag.
  if ! DOTFILES_ALLOWLIST_RE="$ALLOWLIST_RE" awk '
    BEGIN {
      pattern = tolower(ENVIRON["DOTFILES_ALLOWLIST_RE"])
      if (match("", pattern)) exit 1
    }
    {
      line = tolower($0)
      gsub(pattern, "<allowlisted>", line)
      print line
    }
  ' "$content" > "$content.redacted"; then
    printf 'ERROR: Cannot apply sensitive-data allowlist.\n' >&2
    exit 1
  fi
  mv "$content.redacted" "$content"
fi

while IFS= read -r pattern; do
  case "$pattern" in
    ''|'#'*|'['*']') continue ;;
  esac

  if matches=$(grep -inE -- "$pattern" "$content" 2>/dev/null); then
    if [ "$found_secrets" -eq 0 ]; then
      printf '%sERROR: Potential sensitive information detected!%s\n' "$RED" "$NC"
      printf '%s======================================================%s\n\n' "$RED" "$NC"
      found_secrets=1
    fi

    printf '%s\n' "$matches" | while IFS=: read -r index _text; do
      file=$(sed -n "${index}p" "$filenames")
      lineno=$(sed -n "${index}p" "$linenos")
      printf '%sIn file: %s%s\n' "$YELLOW" "$file" "$NC"
      printf '%s  Line %s: <redacted>%s\n' "$YELLOW" "$lineno" "$NC"
    done
    printf '\n'
  else
    status=$?
    if [ "$status" -ne 1 ]; then
      printf 'ERROR: Sensitive-data scan failed (grep status %s).\n' "$status" >&2
      exit 1
    fi
  fi
done <<PATTERNS_EOF
$PATTERNS
PATTERNS_EOF

if [ "$found_secrets" -eq 1 ]; then
  printf '%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted to prevent leaking sensitive data.%s\n' "$RED" "$NC"
  printf '%sPlease review and remove any sensitive information.%s\n' "$YELLOW" "$NC"
  printf '%sIf this is a false positive, use:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
fi

exit 0
