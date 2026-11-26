#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../../../../tests/lib/test-helpers.sh
. "$SCRIPT_DIR/../../../../tests/lib/test-helpers.sh"
LINTER_DIR=$(CDPATH='' cd -- "$SCRIPT_DIR/../../../../cli/src/app/validation/scripts" && pwd)

test_psscriptanalyzer()
{
  if ! is_program_installed pwsh; then
    printf 'ERROR: pwsh is not installed; cannot run PSScriptAnalyzer\n' >&2
    return 1
  fi
  pwsh -NoProfile -File "$LINTER_DIR/psscriptanalyzer.ps1" -Root "$DIR"
}

test_shellcheck()
{
  sh "$LINTER_DIR/shellcheck.sh" --root "$DIR"
}

test_ruff()
{
  inputs=$(mktemp)
  printf '%s\0' "$DIR" > "$inputs"
  for dir in symlinks hooks tests .github; do
    [ -d "$DIR/$dir" ] || continue
    find "$DIR/$dir" -type f ! -name '*.py' ! -name '*.pyi' -exec sh -c '
      for file do
        if LC_ALL=C awk '\''
          NR == 1 {
            if (substr($0, 1, 2) != "#!") exit 1
            sub(/^#![ \t]*/, "")
            count = split($0, words, /[ \t]+/)
            name = words[1]
            sub(/^.*\//, "", name)
            if (name == "env") {
              name = ""
              for (i = 2; i <= count; i++) {
                if (words[i] !~ /^-/) { name = words[i]; break }
              }
            }
            exit !(name ~ /^python([0-9]+(\.[0-9]+)*)?$/)
          }
          END { if (NR == 0) exit 1 }
        '\'' "$file"; then
          printf "%s\0" "$file"
        fi
      done
    ' sh {} + >> "$inputs"
  done
  status=0
  xargs -0 ruff check --config "$DIR/.github/linters/ruff.toml" -- < "$inputs" || status=$?
  rm -f "$inputs"
  return "$status"
}

test_luacheck()
{
  (
    cd "$DIR"
    find symlinks tests -type f -name '*.lua' -print0 |
      xargs -0 -r luacheck --config "$DIR/.github/linters/luacheckrc"
  )
}

for test_name in "$@"; do
  "$test_name"
done
