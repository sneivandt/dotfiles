#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-static-analysis.sh — ShellCheck and PSScriptAnalyzer CI tests.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

# shellcheck disable=SC2154,SC3054
# When sourced with `.`, use BASH_SOURCE if available (bash); otherwise use pwd
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  # Fallback: assume we're already in the scripts directory or use relative path
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Run PSScriptAnalyzer on all .ps1/.psm1 files.
test_psscriptanalyzer()
{(
  if ! is_program_installed "pwsh"; then
    log_verbose "Skipping PSScriptAnalyzer: pwsh not installed"
    return 0
  fi
  log_stage "Running PSScriptAnalyzer"
  pwsh -NoProfile -Command "
    Import-Module PSScriptAnalyzer -Force
    \$hasErrors = \$false
    Get-ChildItem -Path '$DIR' -Include '*.ps1','*.psm1' -Recurse -File | ForEach-Object {
      \$results = Invoke-ScriptAnalyzer -Path \$_.FullName -Severity Warning,Error
      if (\$results) {
        \$results | Format-Table -AutoSize
        \$hasErrors = \$true
      }
    }
    if (\$hasErrors) { exit 1 }
  "
)}

# Run shellcheck on all shell scripts in the repository.
test_shellcheck()
{(
  if ! is_program_installed "shellcheck"; then
    log_error "shellcheck not installed"
  fi
  log_stage "Running shellcheck"

  scripts="$DIR/dotfiles.sh $DIR/install.sh"

  # Collect .sh files from key directories
  for search_dir in "$DIR"/.github "$DIR"/hooks; do
    [ -d "$search_dir" ] || continue
    while IFS= read -r f; do
      is_shell_script "$f" && scripts="$scripts $f"
    done <<EOF
$(find "$search_dir" -type f -name "*.sh")
EOF
  done

  # Add shell scripts from symlinks/
  if [ -d "$DIR/symlinks" ]; then
    while IFS= read -r f; do
      is_shell_script "$f" && scripts="$scripts $f"
    done <<EOF
$(find "$DIR/symlinks" -type f -name "*.sh" 2>/dev/null)
EOF
  fi

  log_verbose "Checking: $scripts"
  # shellcheck disable=SC2086  # intentional word splitting
  shellcheck $scripts
)}
