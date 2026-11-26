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
  status=0
  if python3 -B "$SCRIPT_DIR/ci_metadata.py" ruff-inputs "$DIR" > "$inputs"; then
    xargs -0 ruff check --config "$DIR/.github/linters/ruff.toml" -- < "$inputs" || status=$?
  else
    status=$?
  fi
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
