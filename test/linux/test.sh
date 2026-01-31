#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test.sh
# -----------------------------------------------------------------------------
# Test and validation functions for static analysis and linting.
#
# Functions:
#   test_psscriptanalyzer  Run PSScriptAnalyzer on PowerShell files.
#   test_shellcheck        Run shellcheck on shell scripts.
#
# Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (is_program_installed, read_ini_section, is_shell_script)
# -----------------------------------------------------------------------------

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
    pwsh -Command "Import-Module $DIR/test/windows/Test.psm1 && Test-PSScriptAnalyzer -dir $DIR"
  else
    log_verbose "Skipping PSScriptAnalyzer: pwsh not installed"
  fi
)}

# test_shellcheck
#
# Execute shellcheck across all shell scripts discovered in symlinks/
# excluding any paths that reside within declared submodules (to avoid
# flagging third-party code). Non-zero shellcheck exit is swallowed (|| true)
# so the overall run continues; individual findings still surface.
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

    # Check if symlinks.ini exists (may be excluded by sparse checkout)
    if [ ! -f "$DIR"/conf/symlinks.ini ]
    then
      log_verbose "No symlinks.ini found, checking only main scripts"
      # shellcheck disable=SC2086
      shellcheck $scripts || true
      return
    fi

    # Read submodules to exclude from checking
    submodules=""
    if [ -f "$DIR"/conf/submodules.ini ]; then
      sections="$(grep -E '^\[.*\]$' "$DIR"/conf/submodules.ini | tr -d '[]')"
      for section in $sections; do
        # Read all submodules from this section and add to list
        if read_ini_section "$DIR"/conf/submodules.ini "$section" >/dev/null 2>&1; then
          read_ini_section "$DIR"/conf/submodules.ini "$section" | while IFS='' read -r submodule || [ -n "$submodule" ]; do
            if [ -n "$submodule" ]; then
              echo "$submodule"
            fi
          done
        fi
      done | {
        # Build space-separated list from all submodules
        sub_list=""
        while IFS='' read -r submodule; do
          sub_list="$sub_list $submodule"
        done
        # Trim leading space and output
        echo "${sub_list# }"
      } > "$DIR"/.submodules_tmp

      # Read from temp file to avoid subshell scope issues
      if [ -f "$DIR"/.submodules_tmp ]; then
        submodules="$(cat "$DIR"/.submodules_tmp)"
        rm -f "$DIR"/.submodules_tmp
      fi
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

    # Read all collected scripts from temp file
    scripts="$(cat "$scripts_tmp" | tr '\n' ' ')"
    rm -f "$scripts_tmp"

    log_verbose "Checking scripts: $scripts"
    # Run shellcheck on all collected scripts, ignoring errors
    # shellcheck disable=SC2086
    shellcheck $scripts || true
  fi
)}

# test_config_validation
#
# Validate configuration file consistency:
#   * All files in manifest.ini exist in symlinks/
#   * All profiles in profiles.ini are valid
#   * Section names in config files match documented categories
test_config_validation()
{(
  log_stage "Validating configuration consistency"

  local errors=0

  # Check if manifest.ini exists
  if [ ! -f "$DIR"/conf/manifest.ini ]; then
    log_verbose "Skipping validation: no manifest.ini found"
    return
  fi

  # Validate files in manifest.ini exist in symlinks/
  log_verbose "Checking manifest.ini references..."
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/manifest.ini | tr -d '[]')"

  for section in $sections; do
    read_ini_section "$DIR"/conf/manifest.ini "$section" | while IFS='' read -r file || [ -n "$file" ]; do
      if [ -z "$file" ]; then
        continue
      fi

      # Check if the file/directory exists in symlinks/
      if [ ! -e "$DIR"/symlinks/"$file" ]; then
        # Check if it's tracked in git (might be excluded by sparse checkout)
        if [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$file" 2>/dev/null | grep -q .; then
          log_verbose "File $file in manifest.ini is tracked but excluded by sparse checkout"
        else
          printf "${RED}ERROR: File listed in manifest.ini [$section] does not exist: symlinks/%s${NC}\n" "$file" >&2
          errors=$((errors + 1))
        fi
      fi
    done
  done

  # Validate profiles.ini structure
  if [ -f "$DIR"/conf/profiles.ini ]; then
    log_verbose "Checking profiles.ini structure..."
    profiles="$(list_available_profiles)"

    for profile in $profiles; do
      if ! parse_profile "$profile" 2>/dev/null; then
        printf "${RED}ERROR: Invalid profile definition in profiles.ini: %s${NC}\n" "$profile" >&2
        errors=$((errors + 1))
      else
        log_verbose "Profile $profile is valid"
      fi
    done
  fi

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Configuration validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Configuration validation passed"
  fi
)}
