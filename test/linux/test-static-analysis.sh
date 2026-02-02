#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-static-analysis.sh
# -----------------------------------------------------------------------------
# Static analysis and linting tests for shell scripts and PowerShell files.
#
# Functions:
#   test_psscriptanalyzer  Run PSScriptAnalyzer on PowerShell files
#   test_shellcheck        Run shellcheck on shell scripts
#
# Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (is_program_installed, read_ini_section, is_shell_script)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

# test_psscriptanalyzer
#
# Run PowerShell static analysis across repo when pwsh + analyzer module
# available. Skips silently otherwise to keep CI resilient on systems without
# PowerShell.
test_psscriptanalyzer()
{(
  # Check if PowerShell Core is installed
  if is_program_installed "pwsh"
  then
    log_stage "Running PSScriptAnalyzer"
    # Import the helper module and run the analysis function
    pwsh -NoProfile -Command "Import-Module '$DIR/test/windows/Test-StaticAnalysis.psm1'; Test-PSScriptAnalyzer -dir '$DIR'"
  else
    log_verbose "Skipping PSScriptAnalyzer: pwsh not installed"
  fi
)}

# test_shellcheck
#
# Execute shellcheck across all shell scripts discovered in symlinks/ and
# other directories. Non-zero shellcheck exit is swallowed (|| true) so the
# overall run continues; individual findings still surface.
test_shellcheck()
{(
  # Check if shellcheck is installed
  if ! is_program_installed "shellcheck"
  then
    log_error "shellcheck not installed"
  else
    log_stage "Running shellcheck"
    # Start with the main entry point script and source scripts
    scripts="$DIR/dotfiles.sh $DIR/src/linux/*.sh"

    # Add shell scripts from test/ directory
    if [ -d "$DIR"/test ]; then
      tmpfile="$(mktemp)"
      find "$DIR"/test -type f -name "*.sh" > "$tmpfile"
      while IFS='' read -r line || [ -n "$line" ]; do
        if is_shell_script "$line"; then
          scripts="$scripts $line"
        fi
      done < "$tmpfile"
      rm "$tmpfile"
    fi

    # Add shell scripts from .github/ directory
    if [ -d "$DIR"/.github ]; then
      tmpfile="$(mktemp)"
      find "$DIR"/.github -type f -name "*.sh" > "$tmpfile"
      while IFS='' read -r line || [ -n "$line" ]; do
        if is_shell_script "$line"; then
          scripts="$scripts $line"
        fi
      done < "$tmpfile"
      rm "$tmpfile"
    fi

    # Check if symlinks.ini exists (may be excluded by sparse checkout)
    if [ ! -f "$DIR"/conf/symlinks.ini ]
    then
      log_verbose "No symlinks.ini found, checking only main scripts"
      # shellcheck disable=SC2086  # Word splitting intentional: $scripts is space-separated list
      shellcheck $scripts || true
      return
    fi

    # Iterate through symlinks.ini sections to find scripts
    # Get list of sections from symlinks.ini
    sections="$(grep -E '^\[.*\]$' "$DIR"/conf/symlinks.ini | tr -d '[]')"

    # Use temp file to collect scripts to avoid subshell scope issues
    scripts_tmp="$(mktemp)"
    echo "$scripts" > "$scripts_tmp"

    for section in $sections
    do
      read_ini_section "$DIR"/conf/symlinks.ini "$section" | while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        # Skip empty lines
        if [ -z "$symlink" ]
        then
          continue
        fi

        # Check if source exists (may be excluded by sparse checkout)
        if [ ! -e "$DIR"/symlinks/"$symlink" ]
        then
          continue
        fi

        # Handle directories containing scripts
        if [ -d "$DIR"/symlinks/"$symlink" ]
        then
          tmpfile="$(mktemp)"

          # Find all files within the symlinked directory
          find "$DIR"/symlinks/"$symlink" -type f > "$tmpfile"
          while IFS='' read -r line || [ -n "$line" ]
          do
            ignore=false

            # Check if the file belongs to a submodule (third-party code) to exclude it
            for submodule in $submodules
            do
              case "$line" in
                "$DIR"/"$submodule"/*)
                  ignore=true
                  break
                  ;;
              esac
            done

            # If not ignored and is a shell script, add to the list
            if ! "$ignore" \
              && is_shell_script "$line"
            then
              echo "$line" >> "$scripts_tmp"
            fi
          done < "$tmpfile"
          rm "$tmpfile"

        # Handle individual script files
        elif is_shell_script "$DIR"/symlinks/"$symlink"
        then
          echo "$DIR/symlinks/$symlink" >> "$scripts_tmp"
        fi
      done
    done

    # Read a# If is a shell script, add to the list
            if