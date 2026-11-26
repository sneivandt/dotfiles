#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../../../../tests/lib/test-helpers.sh
. "$SCRIPT_DIR/../../../../tests/lib/test-helpers.sh"
LINTER_DIR=$(CDPATH='' cd -- "$SCRIPT_DIR/../../../../cli/src/app/validation/scripts" && pwd)

test_psscriptanalyzer()
{
  if ! is_program_installed pwsh; then
    log_verbose "Skipping PSScriptAnalyzer: pwsh not installed"
    return 0
  fi
  pwsh -NoProfile -File "$LINTER_DIR/psscriptanalyzer.ps1" -Root "$DIR"
}

test_shellcheck()
{
  sh "$LINTER_DIR/shellcheck.sh" --root "$DIR"
}

test_ruff()
{
  ruff check --config "$DIR/ruff.toml" "$DIR"
}

test_luacheck()
{
  (
    cd "$DIR"
    find symlinks tests -type f -name '*.lua' -print0 |
      xargs -0 -r luacheck --config "$DIR/.luacheckrc"
  )
}

for test_name in "$@"; do
  "$test_name"
done
