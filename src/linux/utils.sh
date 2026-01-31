#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# utils.sh
# -----------------------------------------------------------------------------
# Small predicate / helper functions consumed by task + command layers.
# Each returns 0 for "true" (POSIX convention) and 1 for "false".
# Keep logic minimal—complex workflows belong in tasks.sh.
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
#   OPT  CLI options string (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# Detect host OS once to avoid repeated expensive checks.
# IS_ARCH: 1 (processed) if Arch Linux / Arch-based, 0 (ignored) otherwise.
# Logic: Check /etc/*-release. If ANY match for "arch" or "archlinux" is found
# in ID fields, we consider it Arch.
IS_ARCH=0
if grep -E "^(ID|ID_LIKE)=.*" /etc/*-release 2>/dev/null | cut -d= -f2 | tr -d '"' | grep -qxE "arch|archlinux"; then
  IS_ARCH=1
fi

# get_persisted_profile
#
# Reads the previously used profile from git config.
#
# Output:
#   Profile name if set, empty otherwise
#
# Result:
#   0 if profile found, 1 if not set
get_persisted_profile()
{
  # DIR is exported by dotfiles.sh
  # shellcheck disable=SC2154
  if [ ! -d "$DIR"/.git ]; then
    return 1
  fi

  local profile
  profile="$(git -C "$DIR" config --local --get dotfiles.profile 2>/dev/null || true)"

  if [ -n "$profile" ]; then
    echo "$profile"
    return 0
  fi
  return 1
}

# persist_profile
#
# Persists the profile to git config for future use.
#
# Args:
#   $1  profile name to persist
#
# Result:
#   0 success, 1 failure
persist_profile()
{
  local profile="$1"

  if [ ! -d "$DIR"/.git ]; then
    log_verbose "Skipping profile persistence: not a git repository"
    return 0
  fi

  git -C "$DIR" config --local dotfiles.profile "$profile"
  log_verbose "Persisted profile: $profile"
  return 0
}

# list_available_profiles
#
# Lists all available profile names from profiles.ini.
#
# Output:
#   One profile name per line
#
# Result:
#   0 success
list_available_profiles()
{
  if [ ! -f "$DIR"/conf/profiles.ini ]; then
    return 1
  fi

  while IFS='' read -r line || [ -n "$line" ]; do
    # Skip empty lines and comments
    if [ -z "$line" ] || [ "${line#\#}" != "$line" ]; then
      continue
    fi

    # Extract section headers
    if [ "${line#\[}" != "$line" ]; then
      local profile="${line#\[}"
      profile="${profile%\]}"
      echo "$profile"
    fi
  done < "$DIR"/conf/profiles.ini
}

# prompt_profile_selection
#
# Interactively prompts the user to select a profile from available options.
#
# Output:
#   Selected profile name
#
# Result:
#   0 success, 1 if no selection made or error
prompt_profile_selection()
{
  local profiles
  local count=0
  local selection

  # Read available profiles into array-like structure
  profiles="$(list_available_profiles)"

  if [ -z "$profiles" ]; then
    log_error "No profiles found in conf/profiles.ini"
  fi

  echo "Available profiles:" >&2
  echo "" >&2

  # Display profiles with numbers - use a temporary file to avoid subshell issues
  count=0
  while IFS='' read -r profile; do
    count=$((count + 1))
    # Read profile description from profiles.ini
    desc=""
    case "$profile" in
      base)
        desc="Minimal core shell configuration (no OS-specific files)"
        ;;
      arch)
        desc="Arch Linux headless (CLI only)"
        ;;
      arch-desktop)
        desc="Arch Linux desktop (GUI, window manager, fonts)"
        ;;
      windows)
        desc="Windows environment (PowerShell, registry)"
        ;;
      *)
        desc="(No description available)"
        ;;
    esac
    printf "  %d) %-15s - %s\n" "$count" "$profile" "$desc" >&2
  done <<EOF
$profiles
EOF

  # Count total profiles for prompt
  profile_count=$count

  echo "" >&2
  printf "Select profile (1-%d): " "$profile_count" >&2

  read -r selection

  # Validate selection
  if ! echo "$selection" | grep -qE '^[0-9]+$'; then
    log_error "Invalid selection: $selection"
  fi

  # Get profile by number
  local selected_profile
  selected_profile="$(echo "$profiles" | sed -n "${selection}p")"

  if [ -z "$selected_profile" ]; then
    log_error "Invalid selection: $selection"
  fi

  echo "$selected_profile"
  return 0
}

# read_ini_section
#
# Generic INI file section reader. Outputs all non-empty, non-comment lines
# within the specified section.
#
# Args:
#   $1  path to INI file
#   $2  section name to read
#
# Output:
#   Each line in the section (one per line)
#
# Result:
#   0 success (section found), 1 if file or section not found
read_ini_section()
{
  local file="$1"
  local section="$2"
  local in_section=0
  local found=0

  if [ ! -f "$file" ]; then
    return 1
  fi

  while IFS='' read -r line || [ -n "$line" ]; do
    # Skip empty lines and comments
    if [ -z "$line" ] || [ "${line#\#}" != "$line" ]; then
      continue
    fi

    # Check for section header
    if [ "${line#\[}" != "$line" ]; then
      local section_name="${line#\[}"
      section_name="${section_name%\]}"

      # If we were in target section, we're done
      if [ "$in_section" -eq 1 ]; then
        return 0
      fi

      # Check if this is our target section
      if [ "$section_name" = "$section" ]; then
        in_section=1
        found=1
      fi
      continue
    fi

    # Output lines within target section
    if [ "$in_section" -eq 1 ]; then
      echo "$line"
    fi
  done < "$file"

  if [ "$found" -eq 1 ]; then
    return 0
  fi
  return 1
}

# parse_profile
#
# Reads profiles.ini and resolves a profile name to categories to include/exclude.
# Returns two space-separated lists assigned to PROFILE_INCLUDE and PROFILE_EXCLUDE.
#
# Args:
#   $1  profile name (e.g., "arch-desktop")
#
# Globals set:
#   PROFILE_INCLUDE  comma-separated categories to include
#   PROFILE_EXCLUDE  comma-separated categories to exclude
#
# Result:
#   0 success, 1 profile not found
parse_profile()
{
  local profile="$1"
  local in_profile=0

  PROFILE_INCLUDE=""
  PROFILE_EXCLUDE=""

  if [ ! -f "$DIR"/conf/profiles.ini ]; then
    return 1
  fi

  while IFS='' read -r line || [ -n "$line" ]; do
    # Skip empty lines and comments
    if [ -z "$line" ] || [ "${line#\#}" != "$line" ]; then
      continue
    fi

    # Check for section header
    if [ "${line#\[}" != "$line" ]; then
      local section_name="${line#\[}"
      section_name="${section_name%\]}"
      if [ "$section_name" = "$profile" ]; then
        in_profile=1
      else
        in_profile=0
      fi
      continue
    fi

    # Parse key=value within target profile
    if [ "$in_profile" -eq 1 ]; then
      local key="${line%%=*}"
      local value="${line#*=}"
      # Trim whitespace
      key="$(echo "$key" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      value="$(echo "$value" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      case "$key" in
        include)
          PROFILE_INCLUDE="$value"
          ;;
        exclude)
          PROFILE_EXCLUDE="$value"
          ;;
      esac
    fi
  done < "$DIR"/conf/profiles.ini

  # If we found the profile, return success
  if [ -n "$PROFILE_INCLUDE" ] || [ -n "$PROFILE_EXCLUDE" ]; then
    return 0
  fi
  return 1
}

# get_excluded_files
#
# Reads manifest.ini and returns a list of files for excluded categories.
# Each file path is printed on a separate line.
#
# Handles both single-category sections (e.g., [arch]) and multi-category
# sections (e.g., [arch,desktop]). A section is processed if ANY of its
# required categories are in the excluded list.
#
# Args:
#   $1  comma-separated list of categories to exclude (e.g., "windows,gui")
#
# Output:
#   One file path per line
get_excluded_files()
{
  local exclude_list="$1"

  if [ ! -f "$DIR"/conf/manifest.ini ]; then
    return 0
  fi

  # Helper function to check if section matches any excluded category
  section_matches_excluded()
  {
    local section="$1"
    local excludes="$2"

    # Split section name by comma to get required categories
    local section_cat
    for section_cat in $(echo "$section" | tr ',' ' '); do
      # Check if this category is in the exclude list
      case ",$excludes," in
        *,"$section_cat",*)
          # This category is excluded, so include this section
          return 0
          ;;
      esac
    done
    return 1
  }

  # Read manifest.ini and output files from matching sections
  local in_section=0
  local section_name=""

  while IFS='' read -r line || [ -n "$line" ]; do
    # Skip empty lines and comments
    if [ -z "$line" ] || [ "${line#\#}" != "$line" ]; then
      continue
    fi

    # Check for section header
    if [ "${line#\[}" != "$line" ]; then
      section_name="${line#\[}"
      section_name="${section_name%\]}"

      # Check if this section should be processed
      if section_matches_excluded "$section_name" "$exclude_list"; then
        in_section=1
      else
        in_section=0
      fi
      continue
    fi

    # Output file paths within matching sections
    if [ "$in_section" -eq 1 ]; then
      echo "$line"
    fi
  done < "$DIR"/conf/manifest.ini | sort -u
}

# configure_sparse_checkout
#
# Configures git sparse checkout based on selected profile or explicit exclusions.
# Reads profile configuration, applies dependency logic, and configures git to
# exclude files in excluded categories.
#
# Auto-detection overrides (applied after profile parsing):
#   * Non-Arch systems: Always exclude 'arch' category regardless of profile
#   * Linux systems: Always exclude 'windows' category regardless of profile
#
# This ensures system compatibility even if an incompatible profile is selected.
#
# Args:
#   $1  profile name (e.g., "arch-desktop") or empty to use explicit excludes
#
# Globals read:
#   EXPLICIT_EXCLUDE  comma-separated categories to exclude (if no profile)
#   IS_ARCH          1 if Arch Linux detected
#
# Globals set:
#   EXCLUDED_CATEGORIES  comma-separated categories that are excluded
#
# Result:
#   0 success, 1 failure
configure_sparse_checkout()
{
  local profile="$1"
  local exclude_categories=""

  # Skip if not a git repository
  if [ ! -d "$DIR"/.git ]; then
    log_verbose "Skipping sparse checkout: not a git repository"
    EXCLUDED_CATEGORIES=""
    return 0
  fi

  # Resolve profile to exclude categories
  if [ -n "$profile" ]; then
    if ! parse_profile "$profile"; then
      log_error "Profile '$profile' not found in profiles.ini"
    fi
    exclude_categories="$PROFILE_EXCLUDE"

    # Apply auto-detection overrides
    if [ "$IS_ARCH" -eq 0 ]; then
      # Not on Arch - always exclude arch
      case ",$exclude_categories," in
        *,arch,*) ;;
        *)
          if [ -z "$exclude_categories" ]; then
            exclude_categories="arch"
          else
            exclude_categories="$exclude_categories,arch"
          fi
          ;;
      esac
    fi

    # Windows always excluded on Linux
    case ",$exclude_categories," in
      *,windows,*) ;;
      *)
        if [ -z "$exclude_categories" ]; then
          exclude_categories="windows"
        else
          exclude_categories="$exclude_categories,windows"
        fi
        ;;
    esac
  else
    # Use explicit exclusions or default to full checkout
    exclude_categories="${EXPLICIT_EXCLUDE:-}"
  fi

  # Store excluded categories for use by should_include_profile_tag
  EXCLUDED_CATEGORIES="$exclude_categories"
  export EXCLUDED_CATEGORIES

  log_stage "Configuring sparse checkout"
  log_verbose "Profile: ${profile:-<none>}"
  log_verbose "Excluding categories: ${exclude_categories:-<none>}"

  # Build sparse checkout patterns
  local tmpfile
  tmpfile="$(mktemp)"

  # Start with all top-level files
  echo "/*" > "$tmpfile"

  # Add exclusions for specific files from manifest
  if [ -n "$exclude_categories" ]; then
    get_excluded_files "$exclude_categories" | while IFS='' read -r file; do
      if [ -n "$file" ]; then
        echo "!/symlinks/$file"
      fi
    done >> "$tmpfile"
  fi

  # Apply sparse checkout configuration
  git -C "$DIR" sparse-checkout init --no-cone 2>/dev/null || true
  git -C "$DIR" sparse-checkout set --no-cone --stdin < "$tmpfile" 2>/dev/null || true

  rm -f "$tmpfile"

  return 0
}

# should_include_profile_tag
#
# Checks if a configuration section should be included based on current exclusions.
# Section names are comma-separated category requirements (e.g., "arch,desktop").
#
# Logic: Include section only when ALL required categories are available.
# In other words: Exclude section if ANY required category is in EXCLUDED_CATEGORIES.
#
# Examples:
#   - Section [base]: requires category "base" (never excluded, always included)
#   - Section [arch]: requires category "arch" (excluded on non-Arch systems)
#   - Section [arch,desktop]: requires BOTH "arch" AND "desktop" categories
#   - Section [desktop]: requires category "desktop" (excluded in headless profiles)
#
# Note: This is different from profile names in profiles.ini which use hyphens.
#       Profile "arch-desktop" excludes categories: "windows"
#       Section [arch,desktop] requires categories: arch AND desktop (comma-separated)
#
# Args:
#   $1  section name as comma-separated category list (e.g., "base", "arch", "arch,desktop")
#
# Globals read:
#   EXCLUDED_CATEGORIES  comma-separated categories that are excluded
#
# Result:
#   0 include this section, 1 exclude it
should_include_profile_tag()
{
  local profile_tag="$1"

  # Empty profile tag means always include
  if [ -z "$profile_tag" ]; then
    return 0
  fi

  # If no categories are excluded, include everything
  if [ -z "${EXCLUDED_CATEGORIES:-}" ]; then
    return 0
  fi

  # Check each required category in the profile tag
  # If ANY required category is excluded, we must exclude this line
  # Use IFS to split without creating a subshell (avoids pipe-to-while issues)
  local old_ifs="$IFS"
  IFS=','
  for category in $profile_tag; do
    IFS="$old_ifs"
    if [ -z "$category" ]; then
      continue
    fi

    # Check if this category is in the excluded list
    case ",$EXCLUDED_CATEGORIES," in
      *,"$category",*)
        # This category is excluded, so exclude the line
        return 1
        ;;
    esac
  done
  IFS="$old_ifs"

  # All required categories are available, include the line
  return 0
}

# is_flag_set
#
# Check whether a short flag (single character) was present in the original
# CLI invocation as normalized by getopt and stored in $OPT.
#
# Special behavior: verbose flag (-v) is automatically enabled when dry-run
# mode is active to provide visibility into what actions would be taken.
#
# Args:
#   $1  single-letter flag (without leading dash)
#
# Result:
#   0 flag present, 1 absent.
is_flag_set()
{
  # Automatically enable verbose flag when in dry-run mode
  if [ "$1" = "v" ] && is_dry_run; then
    return 0
  fi

  # OPT is exported by dotfiles.sh
  # shellcheck disable=SC2154
  case " $OPT " in
    *" -$1 "*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# is_dry_run
#
# Check if dry run mode is active. Dry run prevents all system modifications.
#
# Result:
#   0 dry run active, 1 normal operation.
is_dry_run()
{
  case " $OPT " in
    *" --dry-run "*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# is_program_installed
#
# Predicate for presence of an executable in PATH. Use `command -v` to avoid
# subshells and output capture overhead.
#
# Args:
#   $1 program name
#
# Result:
#   0 found, 1 missing.
is_program_installed()
{
  if command -v "$1" >/dev/null 2>&1; then
    return 0
  else
    return 1
  fi
}

# is_shell_script
#
# Heuristic: file exists and first line shebang references a POSIX / bash shell.
# Avoids false positives on plain text without execution semantics.
#
# Args:
#   $1 path to file
#
# Result:
#   0 matches known shell shebang, 1 otherwise.
is_shell_script()
{
  if [ -f "$1" ]; then
    case "$(head -n 1 "$1")" in
      '#!/bin/sh'* | '#!/bin/bash'* | '#!/usr/bin/env sh'* | '#!/usr/bin/env bash'*)
        return 0
        ;;
    esac
  fi
  return 1
}

# is_symlink_installed
#
# Compare resolved target of managed symlink against existing entry in $HOME.
# Ensures we only re-link when drift occurred. Uses readlink -f for canonical
# path resolution (following any intermediate symlinks).
#
# Args:
#   $1 relative symlink path as listed in symlinks.conf
#
# Result:
#   0 installed & matches, 1 absent or different.
is_symlink_installed()
{
  if [ "$(readlink -f "$DIR"/symlinks/"$1")" = "$(readlink -f ~/".$1")" ]; then
    return 0
  else
    return 1
  fi
}
