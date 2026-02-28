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

printf "Running sensitive content scan...\n"

if [ ! -f "$PATTERNS_FILE" ]; then
  printf '%sERROR: Patterns file not found: %s%s\n' "$RED" "$PATTERNS_FILE" "$NC"
  printf '%sCannot perform credential scanning without patterns file.%s\n' "$YELLOW" "$NC"
  exit 1
fi

PATTERNS=$(cat "$PATTERNS_FILE")

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

found_secrets=0

tmpfile=$(mktemp)
trap 'rm -f "$tmpfile"' EXIT
git diff --cached --name-only --diff-filter=ACM -z "$against" | tr '\0' '\n' > "$tmpfile"

while IFS= read -r file; do
  [ -z "$file" ] && continue

  if [ ! -f "$file" ]; then
    continue
  fi

  diff_output=$(git diff --cached --unified=0 "$against" -- "$file" || true)

  if [ -z "$diff_output" ]; then
    continue
  fi

  # Build a list of "lineno:content" for added lines by tracking hunk headers
  numbered_adds=$(echo "$diff_output" | awk '
    /^@@ / {
      # Parse +start from @@ -a,b +c,d @@
      split($3, a, /[,+]/)
      lineno = a[2]
      next
    }
    /^\+/ && !/^\+\+\+/ {
      print lineno ":" substr($0, 2)
      lineno++
    }
  ')

  if [ -z "$numbered_adds" ]; then
    continue
  fi

  while IFS= read -r pattern; do
    case "$pattern" in
      ''|'#'*|'['*']') continue ;;
    esac

    matches=$(echo "$numbered_adds" | grep -iE -- "$pattern" 2>/dev/null || true)

    if [ -n "$matches" ]; then
        if [ "$found_secrets" -eq 0 ]; then
          printf '%sERROR: Potential sensitive information detected!%s\n' "$RED" "$NC"
          printf '%s======================================================%s\n\n' "$RED" "$NC"
          found_secrets=1
        fi

        printf '%sIn file: %s%s\n' "$YELLOW" "$file" "$NC"
        printf '%sPattern matched: %s%s\n' "$YELLOW" "$pattern" "$NC"
        echo "$matches" | while IFS=: read -r lineno content; do
          printf '%s  Line %s: %s%s\n' "$YELLOW" "$lineno" "$content" "$NC"
        done
        printf '\n'
    fi
  done <<PATTERNS_EOF
$PATTERNS
PATTERNS_EOF
done < "$tmpfile"

if [ "$found_secrets" -eq 1 ]; then
  printf '%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted to prevent leaking sensitive data.%s\n' "$RED" "$NC"
  printf '%sPlease review and remove any sensitive information.%s\n' "$YELLOW" "$NC"
  printf '%sIf this is a false positive, use:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
fi

exit 0
