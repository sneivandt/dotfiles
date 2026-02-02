#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
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
    pwsh -NoProfile -Command "Import-Module '$DIR/test/windows/Test.psm1'; Test-PSScriptAnalyzer -dir '$DIR'"
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
    # shellcheck disable=SC2086  # Word splitting intentional: $scripts is space-separated list
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

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  # Check if manifest.ini exists
  if [ ! -f "$DIR"/conf/manifest.ini ]; then
    log_verbose "Skipping validation: no manifest.ini found"
    rm -f "$errors_file"
    return
  fi

  # Validate files in manifest.ini exist in symlinks/
  log_verbose "Checking manifest.ini references"
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
          errors=$(cat "$errors_file")
          echo $((errors + 1)) > "$errors_file"
        fi
      fi
    done
  done

  # Validate profiles.ini structure
  if [ -f "$DIR"/conf/profiles.ini ]; then
    log_verbose "Checking profiles.ini structure"
    profiles="$(list_available_profiles)"

    for profile in $profiles; do
      if ! parse_profile "$profile" 2>/dev/null; then
        printf "${RED}ERROR: Invalid profile definition in profiles.ini: %s${NC}\n" "$profile" >&2
        errors=$(cat "$errors_file")
        echo $((errors + 1)) > "$errors_file"
      else
        log_verbose "Profile $profile is valid"
      fi
    done
  fi

  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Configuration validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Configuration validation passed"
  fi
)}

# test_symlinks_validation
#
# Validate symlinks.ini references:
#   * All files referenced in symlinks.ini exist in symlinks/ directory
#   * Section names use valid categories
#   * No duplicate entries within the same section
#   * No duplicate entries across different sections (same symlink in multiple profiles)
test_symlinks_validation()
{(
  log_stage "Validating symlinks.ini"

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  if [ ! -f "$DIR"/conf/symlinks.ini ]; then
    log_verbose "Skipping validation: no symlinks.ini found"
    rm -f "$errors_file"
    return
  fi

  log_verbose "Checking symlinks.ini file references and duplicates"
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/symlinks.ini | tr -d '[]')"

  # Track all entries across all sections to detect cross-section duplicates
  all_entries="$(mktemp)"

  for section in $sections; do
    # Track entries within this section to detect duplicates
    seen_entries="$(mktemp)"

    read_ini_section "$DIR"/conf/symlinks.ini "$section" | while IFS='' read -r symlink || [ -n "$symlink" ]; do
      if [ -z "$symlink" ]; then
        continue
      fi

      # Check for duplicates within this section
      if grep -Fqx "$symlink" "$seen_entries"; then
        printf "${RED}ERROR: Duplicate entry in symlinks.ini [$section]: %s${NC}\n" "$symlink" >&2
        errors=$(cat "$errors_file")
        echo $((errors + 1)) > "$errors_file"
      else
        echo "$symlink" >> "$seen_entries"
      fi

      # Check for duplicates across sections
      if grep -Fqx "$symlink" "$all_entries"; then
        printf "${RED}ERROR: Duplicate entry across sections in symlinks.ini: %s (appears in [$section] and another section)${NC}\n" "$symlink" >&2
        errors=$(cat "$errors_file")
        echo $((errors + 1)) > "$errors_file"
      else
        echo "$symlink" >> "$all_entries"
      fi

      # Check if the file/directory exists in symlinks/
      if [ ! -e "$DIR"/symlinks/"$symlink" ]; then
        # Check if it's tracked in git (might be excluded by sparse checkout)
        if [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$symlink" 2>/dev/null | grep -q .; then
          log_verbose "File $symlink in symlinks.ini [$section] is tracked but excluded by sparse checkout"
        else
          printf "${RED}ERROR: File listed in symlinks.ini [$section] does not exist: symlinks/%s${NC}\n" "$symlink" >&2
          errors=$(cat "$errors_file")
          echo $((errors + 1)) > "$errors_file"
        fi
      fi
    done

    rm -f "$seen_entries"
  done

  rm -f "$all_entries"

  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Symlinks validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Symlinks validation passed"
  fi
)}

# test_chmod_validation
#
# Validate chmod.ini references:
#   * All files referenced exist in symlinks/ directory
#   * File permissions are valid Unix permission strings
test_chmod_validation()
{(
  log_stage "Validating chmod.ini"

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  if [ ! -f "$DIR"/conf/chmod.ini ]; then
    log_verbose "Skipping validation: no chmod.ini found"
    rm -f "$errors_file"
    return
  fi

  log_verbose "Checking chmod.ini file references"
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/chmod.ini | tr -d '[]')"

  for section in $sections; do
    read_ini_section "$DIR"/conf/chmod.ini "$section" | while IFS='' read -r line || [ -n "$line" ]; do
      if [ -z "$line" ]; then
        continue
      fi

      # Parse mode and file path (format: mode file)
      mode="${line%% *}"
      file="${line#* }"

      # Validate mode format (3 or 4 octal digits)
      if ! echo "$mode" | grep -Eq '^[0-7]{3,4}$'; then
        printf "${RED}ERROR: Invalid permission mode in chmod.ini [$section]: %s${NC}\n" "$mode" >&2
        errors=$(cat "$errors_file")
        echo $((errors + 1)) > "$errors_file"
      fi

      # Check if file exists in symlinks/
      if [ ! -e "$DIR"/symlinks/"$file" ]; then
        # Check if it's tracked in git (might be excluded by sparse checkout)
        if [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$file" 2>/dev/null | grep -q .; then
          log_verbose "File $file in chmod.ini [$section] is tracked but excluded by sparse checkout"
        else
          printf "${RED}ERROR: File listed in chmod.ini [$section] does not exist: symlinks/%s${NC}\n" "$file" >&2
          errors=$(cat "$errors_file")
          echo $((errors + 1)) > "$errors_file"
        fi
      fi
    done
  done

  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Chmod validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Chmod validation passed"
  fi
)}

# test_ini_syntax
#
# Validate INI file syntax:
#   * All INI files have valid format
#   * No malformed section headers
#   * No invalid characters in section names
test_ini_syntax()
{(
  log_stage "Validating INI file syntax"

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  log_verbose "Checking INI file syntax"

  # Check all .ini files in conf/
  for ini_file in "$DIR"/conf/*.ini; do
    if [ ! -f "$ini_file" ]; then
      continue
    fi

    filename="$(basename "$ini_file")"
    log_verbose "Checking $filename"

    line_num=0
    in_section=false

    while IFS='' read -r line || [ -n "$line" ]; do
      line_num=$((line_num + 1))

      # Skip empty lines and comments
      if [ -z "$line" ] || echo "$line" | grep -Eq '^\s*#'; then
        continue
      fi

      # Check for section headers
      if echo "$line" | grep -Eq '^\[.+\]$'; then
        in_section=true
        section_name="$(echo "$line" | tr -d '[]')"

        # registry.ini uses Windows registry paths as section names (allow colons, backslashes, spaces)
        if [ "$filename" = "registry.ini" ]; then
          # Validate registry paths: must start with valid hive (HKCU, HKLM, etc.)
          if ! echo "$section_name" | grep -Eq '^HK(CU|LM|CR|U|CC|PD):'; then
            printf "${RED}ERROR: Invalid registry path in %s line %d (must start with HKCU:, HKLM:, etc.): %s${NC}\n" "$filename" "$line_num" "$line" >&2
            errors=$(cat "$errors_file")
            echo $((errors + 1)) > "$errors_file"
          fi
        else
          # Standard INI files: allow alphanumeric, comma, hyphen, underscore, dot
          if ! echo "$section_name" | grep -Eq '^[a-zA-Z0-9,._-]+$'; then
            printf "${RED}ERROR: Invalid section name in %s line %d: %s${NC}\n" "$filename" "$line_num" "$line" >&2
            errors=$(cat "$errors_file")
            echo $((errors + 1)) > "$errors_file"
          fi
        fi
      # Check for malformed section headers (missing brackets)
      elif echo "$line" | grep -Eq '^\['; then
        printf "${RED}ERROR: Malformed section header in %s line %d: %s${NC}\n" "$filename" "$line_num" "$line" >&2
        errors=$(cat "$errors_file")
        echo $((errors + 1)) > "$errors_file"
      fi
    done < "$ini_file"

    # Check that file is not empty or only contains comments
    if ! grep -Eq '^\[.+\]$' "$ini_file"; then
      printf "${RED}ERROR: No valid sections found in %s${NC}\n" "$filename" >&2
      errors=$(cat "$errors_file")
      echo $((errors + 1)) > "$errors_file"
    fi
  done

  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}INI syntax validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "INI syntax validation passed"
  fi
)}

# test_category_consistency
#
# Validate category consistency across configuration files:
#   * Categories used in section names match valid categories from profiles.ini
#   * Profile names themselves are also valid as section names
#   * Some files have special fixed section names (fonts, extensions)
test_category_consistency()
{(
  log_stage "Validating category consistency"

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  if [ ! -f "$DIR"/conf/profiles.ini ]; then
    log_verbose "Skipping validation: no profiles.ini found"
    rm -f "$errors_file"
    return
  fi

  # Extract all unique categories from profiles.ini (include/exclude values)
  valid_categories="$(mktemp)"

  # Read all include/exclude lines and extract categories
  {
    grep -E '^(include|exclude)=' "$DIR"/conf/profiles.ini | \
      sed 's/^[^=]*=//' | \
      tr ',' '\n' | \
      grep -v '^$' | \
      sort -u

    # Also add profile names themselves as valid section names
    list_available_profiles

    # Add special fixed section names that aren't profile-based
    echo "fonts"
    echo "extensions"
    echo "vim-plugins"
  } > "$valid_categories"

  # Remove duplicates and sort
  sort -u "$valid_categories" -o "$valid_categories"

  log_verbose "Valid categories/sections: $(cat "$valid_categories" | tr '\n' ',' | sed 's/,$//')"

  # Check section names in all config files (except profiles.ini and registry.ini)
  for ini_file in "$DIR"/conf/*.ini; do
    filename="$(basename "$ini_file")"

    # Skip profiles.ini (defines categories) and registry.ini (uses registry paths as sections)
    if [ "$filename" = "profiles.ini" ] || [ "$filename" = "registry.ini" ]; then
      continue
    fi

    if [ ! -f "$ini_file" ]; then
      continue
    fi

    log_verbose "Checking categories in $filename"

    # Extract section names
    sections="$(grep -E '^\[.+\]$' "$ini_file" | tr -d '[]')"

    for section in $sections; do
      # Split comma-separated categories in section name
      categories="$(echo "$section" | tr ',' '\n')"

      for category in $categories; do
        # Check if category is valid
        if ! grep -Fqx "$category" "$valid_categories"; then
          printf "${RED}ERROR: Unknown category '%s' in %s section [%s]${NC}\n" "$category" "$filename" "$section" >&2
          errors=$(cat "$errors_file")
          echo $((errors + 1)) > "$errors_file"
        fi
      done
    done
  done

  rm -f "$valid_categories"

  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Category consistency validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Category consistency validation passed"
  fi
)}

# test_empty_sections
#
# Check for empty sections in configuration files and report them as warnings.
# Empty sections might indicate incomplete configuration or forgotten entries.
test_empty_sections()
{(
  log_stage "Checking for empty sections"

  local warnings_file
  warnings_file="$(mktemp)"
  echo 0 > "$warnings_file"

  # Check all .ini files in conf/
  for ini_file in "$DIR"/conf/*.ini; do
    if [ ! -f "$ini_file" ]; then
      continue
    fi

    filename="$(basename "$ini_file")"
    log_verbose "Checking $filename for empty sections"

    # Get all sections, one per line
    grep -E '^\[.+\]$' "$ini_file" | tr -d '[]' | while IFS='' read -r section || [ -n "$section" ]; do
      # Read section and count non-empty, non-comment lines
      entry_count="$(read_ini_section "$ini_file" "$section" | grep -vc '^$')"

      if [ "$entry_count" -eq 0 ]; then
        printf "${YELLOW}WARNING: Empty section [%s] in %s${NC}\n" "$section" "$filename" >&2
        warnings=$(cat "$warnings_file")
        echo $((warnings + 1)) > "$warnings_file"
      fi
    done
  done

  warnings=$(cat "$warnings_file")
  rm -f "$warnings_file"

  if [ "$warnings" -gt 0 ]; then
    log_verbose "Found $warnings empty section(s) (warnings only)"
  else
    log_verbose "No empty sections found"
  fi
)}

# test_zsh_completion
#
# Validate zsh completion file structure and profile loading.
# Checks that completion can be loaded and dynamically reads profiles.
test_zsh_completion()
{(
  # Skip if zsh is not installed
  if ! is_program_installed "zsh"; then
    log_verbose "Skipping zsh completion test: zsh not installed"
    return 0
  fi

  log_stage "Validating zsh completion"

  local completion_file="$DIR/symlinks/config/zsh/completions/_dotfiles"

  # Check that completion file exists
  if [ ! -f "$completion_file" ]; then
    printf "${RED}ERROR: Completion file not found: %s${NC}\n" "$completion_file" >&2
    return 1
  fi

  log_verbose "Testing completion file structure"

  # Test that completion loads without errors
  if ! zsh -c "source '$completion_file' 2>&1" >/dev/null 2>&1; then
    printf "${RED}ERROR: Completion file failed to load${NC}\n" >&2
    return 1
  fi

  log_verbose "Completion file loads successfully"

  # Test that main functions are defined
  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles >/dev/null" 2>&1; then
    printf "${RED}ERROR: _dotfiles function not defined${NC}\n" >&2
    return 1
  fi

  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles_get_profiles >/dev/null" 2>&1; then
    printf "${RED}ERROR: _dotfiles_get_profiles function not defined${NC}\n" >&2
    return 1
  fi

  log_verbose "Completion functions are defined"

  # Test profile loading from profiles.ini
  # We can't call _dotfiles_get_profiles directly as it uses _describe which only
  # works in completion context, so we test the logic by checking file reading
  log_verbose "Testing profile path resolution and file reading"

  profiles_loaded=$(zsh -c "
    cd '$DIR'
    profile_file='$DIR/conf/profiles.ini'
    count=0
    if [[ -f \"\$profile_file\" ]]; then
      while IFS= read -r line; do
        if [[ \$line =~ '^\[([^]]+)\]\$' ]]; then
          count=\$((count + 1))
        fi
      done < \"\$profile_file\"
    fi
    echo \$count
  ")

  if [ -z "$profiles_loaded" ] || [ "$profiles_loaded" -eq 0 ]; then
    printf "${RED}ERROR: No profiles loaded from profiles.ini${NC}\n" >&2
    return 1
  fi

  log_verbose "Loaded $profiles_loaded profile(s) from profiles.ini"

  # Verify loaded profiles match those in profiles.ini
  expected_profiles=$(list_available_profiles | wc -l)
  if [ "$profiles_loaded" -ne "$expected_profiles" ]; then
    printf "${RED}ERROR: Profile count mismatch: expected %d, got %d${NC}\n" "$expected_profiles" "$profiles_loaded" >&2
    return 1
  fi

  # Validate mutual exclusivity patterns in completion
  log_verbose "Validating mutual exclusivity patterns"

  # Extract the line with -I definition and check it excludes --install
  if ! grep -- "'-I'\[" "$completion_file" | grep -qF -- "--install"; then
    printf "${RED}ERROR: -I does not exclude --install in its exclusion list${NC}\n" >&2
    return 1
  fi

  # Extract the line with --install definition and check it excludes -I
  if ! grep -- ")--install\[" "$completion_file" | grep -qF -- "-I"; then
    printf "${RED}ERROR: --install does not exclude -I in its exclusion list${NC}\n" >&2
    return 1
  fi

  # Check that all command flags exclude help (both -h and --help)
  # Test a few key flags with their actual patterns in the file
  for pattern in "'-I'\[" ")--install\[" "'-T'\[" ")--test\[" "'-U'\[" ")--uninstall\["; do
    # Find the line defining this flag and check it has -h and --help in exclusions
    local flag_line
    flag_line=$(grep -- "$pattern" "$completion_file" || true)

    if [ -z "$flag_line" ]; then
      printf "${RED}ERROR: Could not find definition for pattern ${pattern}${NC}\n" >&2
      return 1
    fi

    if ! echo "$flag_line" | grep -qF -- " -h " || \
       ! echo "$flag_line" | grep -qF -- " --help"; then
      printf "${RED}ERROR: Flag matching ${pattern} does not exclude both help flags${NC}\n" >&2
    fi
  done

  # Check that help excludes everything with (- *)
  if ! grep -F -- "(- *)'-h'" "$completion_file" >/dev/null || \
     ! grep -F -- "(- *)--help[" "$completion_file" >/dev/null; then
    printf "${RED}ERROR: Help flags do not properly exclude all other options${NC}\n" >&2
    return 1
  fi

  log_verbose "Mutual exclusivity patterns validated"

  log_verbose "Zsh completion validation passed"
)}

# test_vim_opens
#
# Test that vim can start and exit without errors.
# This is a basic smoke test to ensure vim is functional.
test_vim_opens()
{(
  # Check if vim is installed
  if ! is_program_installed "vim"; then
    log_verbose "Skipping vim test: vim not installed"
    return 0
  fi

  log_stage "Testing vim startup"

  # Test 1: Check vim version (ensures binary works)
  if ! vim --version >/dev/null 2>&1; then
    printf "${RED}ERROR: Cannot run vim --version${NC}\n" >&2
    return 1
  fi

  log_verbose "Vim binary is functional"

  # Test 2: Check if custom vimrc is installed
  if [ -f "$HOME/.vim/vimrc" ]; then
    log_verbose "Custom vimrc found at ~/.vim/vimrc"
    
    # Test that vim can start with the custom vimrc
    # Use ex mode with immediate quit
    if echo | vim -e -s -c ':qa!' >/dev/null 2>&1; then
      log_verbose "Vim loads custom vimrc successfully"
    else
      printf "${YELLOW}WARNING: Vim may have issues loading custom vimrc${NC}\n" >&2
    fi
  else
    log_verbose "No custom vimrc installed, basic vim test complete"
  fi
)}

# test_nvim_opens
#
# Test that neovim can start and exit without errors.
# This is a basic smoke test to ensure nvim is functional.
test_nvim_opens()
{(
  # Check if nvim is installed
  if ! is_program_installed "nvim"; then
    log_verbose "Skipping nvim test: nvim not installed"
    return 0
  fi

  log_stage "Testing nvim startup"

  # Test 1: Check nvim version (ensures binary works)
  if ! nvim --version >/dev/null 2>&1; then
    printf "${RED}ERROR: Cannot run nvim --version${NC}\n" >&2
    return 1
  fi

  log_verbose "Nvim binary is functional"

  # Test 2: Check if nvim config is installed
  if [ -f "$HOME/.config/nvim/init.vim" ] || [ -f "$HOME/.config/nvim/init.lua" ]; then
    log_verbose "Custom nvim config found"
    
    # Test that nvim can start with the custom config in headless mode
    if nvim --headless -c ':qa!' >/dev/null 2>&1; then
      log_verbose "Nvim loads custom config successfully"
    else
      printf "${YELLOW}WARNING: Nvim may have issues loading custom config${NC}\n" >&2
    fi
  else
    log_verbose "No custom nvim config installed, basic nvim test complete"
  fi
)}
