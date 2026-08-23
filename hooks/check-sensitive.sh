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
PATTERNS_FILE="$SCRIPT_DIR/sensitive-patterns.ini"
ALLOWLIST_FILE="$SCRIPT_DIR/sensitive-allowlist.ini"

printf "Running sensitive content scan...\n"

if [ ! -f "$PATTERNS_FILE" ]; then
  printf '%sERROR: Patterns file not found: %s%s\n' "$RED" "$PATTERNS_FILE" "$NC"
  printf '%sCannot perform credential scanning without patterns file.%s\n' "$YELLOW" "$NC"
  exit 1
fi

PATTERNS=$(cat "$PATTERNS_FILE")

# Combine allowlist entries into a single alternation. Matching spans are
# structurally safe (see sensitive-allowlist.ini) and are redacted before
# scanning, so contributors are not pushed toward --no-verify. Redacting the
# span rather than dropping the line keeps the rest of the line scanned.
ALLOWLIST_RE=""
if [ -f "$ALLOWLIST_FILE" ]; then
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

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

found_secrets=0

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

changes="$tmpdir/changes"
content="$tmpdir/content"
linenos="$tmpdir/linenos"

# Enumerate staged changes as "newpath[<TAB>oldpath]" records. Deletions are
# excluded (lowercase 'd'); every other status is scanned, including renames.
# A rename that also introduces a secret must not escape the scan, so R/C
# records keep the old path too: passing both paths to `git diff` lets rename
# detection pair them, so only the genuinely added lines are reported.
git diff --cached --name-status --diff-filter=d -z "$against" | awk '
  BEGIN { RS = "\0"; state = 0 }
  state == 0 { state = ($0 ~ /^[RC]/) ? 2 : 1; next }
  state == 2 { old = $0; state = 3; next }
  state == 3 { print $0 "\t" old; state = 0; next }
  state == 1 { print $0; state = 0; next }
' > "$changes"

while IFS="$(printf '\t')" read -r file old; do
  [ -z "$file" ] && continue

  if [ -n "$old" ]; then
    diff_output=$(git diff --cached --unified=0 "$against" -- "$old" "$file" || true)
  else
    diff_output=$(git diff --cached --unified=0 "$against" -- "$file" || true)
  fi

  if [ -z "$diff_output" ]; then
    continue
  fi

  # Split added lines into two line-aligned files: raw content and its line
  # number. Patterns are matched against content alone so that '^' and '$'
  # anchors bind to the start and end of the added line itself.
  echo "$diff_output" | awk -v content="$content" -v linenos="$linenos" '
    /^diff --git / { in_hunk = 0; next }
    /^@@ / {
      # Parse +start from @@ -a,b +c,d @@
      s = $3
      sub(/^\+/, "", s)
      sub(/,.*/, "", s)
      lineno = s + 0
      in_hunk = 1
      next
    }
    # Only body lines of a hunk are additions. The "+++ b/file" header always
    # precedes the first "@@", so position excludes it without also discarding
    # added content that happens to begin with "++".
    in_hunk && /^\+/ {
      print substr($0, 2) > content
      print lineno > linenos
      lineno++
    }
  '

  if [ ! -s "$content" ]; then
    : > "$content"
    : > "$linenos"
    continue
  fi

  if [ -n "$ALLOWLIST_RE" ]; then
    # Use a control character as the sed delimiter so allow-list patterns may
    # contain '/' (action references) without escaping.
    sed_delim=$(printf '\001')
    sed -E "s${sed_delim}${ALLOWLIST_RE}${sed_delim}<allowlisted>${sed_delim}gI" \
      "$content" > "$content.redacted"
    mv "$content.redacted" "$content"
  fi

  while IFS= read -r pattern; do
    case "$pattern" in
      ''|'#'*|'['*']') continue ;;
    esac

    # -n numbers matches by position within the added-line list, which maps
    # back to the real file line number through the aligned linenos file.
    matches=$(grep -inE -- "$pattern" "$content" 2>/dev/null || true)

    if [ -n "$matches" ]; then
        if [ "$found_secrets" -eq 0 ]; then
          printf '%sERROR: Potential sensitive information detected!%s\n' "$RED" "$NC"
          printf '%s======================================================%s\n\n' "$RED" "$NC"
          found_secrets=1
        fi

        printf '%sIn file: %s%s\n' "$YELLOW" "$file" "$NC"
        echo "$matches" | while IFS=: read -r index _text; do
          lineno=$(sed -n "${index}p" "$linenos")
          printf '%s  Line %s: <redacted>%s\n' "$YELLOW" "$lineno" "$NC"
        done
        printf '\n'
    fi
  done <<PATTERNS_EOF
$PATTERNS
PATTERNS_EOF

  : > "$content"
  : > "$linenos"
done < "$changes"

if [ "$found_secrets" -eq 1 ]; then
  printf '%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted to prevent leaking sensitive data.%s\n' "$RED" "$NC"
  printf '%sPlease review and remove any sensitive information.%s\n' "$YELLOW" "$NC"
  printf '%sIf this is a false positive, use:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
fi

exit 0
