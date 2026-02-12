#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-config.sh
# -----------------------------------------------------------------------------
# Configuration validation tests for INI files and manifest consistency.
#
# Functions:
#   test_config_validation     Validate configuration file consistency
#   test_symlinks_validation   Validate symlinks.ini references
#   test_chmod_validation      Validate chmod.ini references
#   test_ini_syntax            Validate INI file syntax
#   test_category_consistency  Validate category consistency across files
#   test_empty_sections        Check for empty sections in INI files
#
# Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (read_ini_section, list_available_profiles, parse_profile)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

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
        # Strip trailing slash for symlink check (trailing slash prevents -L from detecting symlinks)
        local file_no_slash="${file%/}"
        # Check if it's a symlink (even if broken, it might be valid in other profiles)
        if [ -L "$DIR"/symlinks/"$file_no_slash" ]; then
          log_verbose "File $file in manifest.ini is a symlink (target may be excluded by sparse checkout)"
        # Check if it's tracked in git (might be excluded by sparse checkout)
        elif [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$file" 2>/dev/null | grep -q .; then
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
        # Check if the path itself is a symlink (even if broken)
        if [ -L "$DIR"/symlinks/"$symlink" ]; then
          log_verbose "File $symlink in symlinks.ini [$section] is a symlink (target may be excluded by sparse checkout)"
        else
          # Check if any parent directory is a symlink (which might be broken due to sparse checkout)
          local path_to_check="$symlink"
          local is_under_symlink=false
          while [ "$path_to_check" != "." ] && [ "$path_to_check" != "/" ]; do
            path_to_check="$(dirname "$path_to_check")"
            if [ -L "$DIR"/symlinks/"$path_to_check" ]; then
              is_under_symlink=true
              log_verbose "File $symlink in symlinks.ini [$section] is under symlink directory $path_to_check (target may be excluded by sparse checkout)"
              break
            fi
          done

          # If not under a symlink, check if tracked in git
          if [ "$is_under_symlink" = false ]; then
            if [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$symlink" 2>/dev/null | grep -q .; then
              log_verbose "File $symlink in symlinks.ini [$section] is tracked but excluded by sparse checkout"
            else
              printf "${RED}ERROR: File listed in symlinks.ini [$section] does not exist: symlinks/%s${NC}\n" "$symlink" >&2
              errors=$(cat "$errors_file")
              echo $((errors + 1)) > "$errors_file"
            fi
          fi
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
        # shellcheck disable=SC2034  # in_section used for validation context
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
    echo "aur"
  } > "$valid_categories"

  # Remove duplicates and sort
  sort -u "$valid_categories" -o "$valid_categories"

  log_verbose "Valid categories/sections: $(tr '\n' ',' < "$valid_categories" | sed 's/,$//')"

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
      # Use || true to prevent grep from failing when section is empty
      entry_count="$(read_ini_section "$ini_file" "$section" | grep -vc '^$' || true)"

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
