#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-config.sh — Configuration validation tests for TOML files.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
# When sourced with `.`, use BASH_SOURCE if available (bash); otherwise use pwd
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  # Fallback: assume we're already in the scripts directory or use relative path
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Validate that files listed in manifest.toml exist in symlinks/.
test_config_validation()
{(
  log_stage "Validating configuration consistency"
  [ -f "$DIR/conf/manifest.toml" ] || { log_verbose "No manifest.toml, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/manifest.toml" | tr -d '[]')"
  for section in $sections; do
    entries="$(read_toml_section_array "$DIR/conf/manifest.toml" "$section" "paths")" || true
    echo "$entries" | while IFS='' read -r file || [ -n "$file" ]; do
      [ -n "$file" ] || continue
      if [ ! -e "$DIR/symlinks/$file" ] && [ ! -L "$DIR/symlinks/${file%/}" ]; then
        printf "%sERROR: manifest.toml [%s] references missing: symlinks/%s%s\n" "${RED}" "$section" "$file" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  done

  # Validate profiles
  if [ -f "$DIR/conf/profiles.toml" ]; then
    for profile in $(list_available_profiles); do
      if ! parse_profile "$profile"; then
        printf "%sERROR: profile '%s' could not be parsed%s\n" "${RED}" "$profile" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  fi

  result="$(cat "$err_file")"; rm -f "$err_file"
  [ "$result" -eq 0 ] || return 1
)}

# Validate that files in symlinks.toml exist in symlinks/.
test_symlinks_validation()
{(
  log_stage "Validating symlinks.toml references"
  [ -f "$DIR/conf/symlinks.toml" ] || { log_verbose "No symlinks.toml, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/symlinks.toml" | tr -d '[]')"
  for section in $sections; do
    entries="$(read_toml_section_array "$DIR/conf/symlinks.toml" "$section" "symlinks")" || true
    echo "$entries" | while IFS='' read -r src || [ -n "$src" ]; do
      [ -n "$src" ] || continue
      if [ ! -e "$DIR/symlinks/$src" ] && [ ! -L "$DIR/symlinks/$src" ]; then
        printf "%sERROR: symlinks.toml [%s] source missing: symlinks/%s%s\n" "${RED}" "$section" "$src" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  done
  result="$(cat "$err_file")"; rm -f "$err_file"
  [ "$result" -eq 0 ] || return 1
)}

# Validate that files in chmod.toml exist in symlinks/.
test_chmod_validation()
{(
  log_stage "Validating chmod.toml references"
  [ -f "$DIR/conf/chmod.toml" ] || { log_verbose "No chmod.toml, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  # chmod.toml uses structured objects: { mode = "600", path = "ssh/config" }
  # Extract path values from permissions arrays
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/chmod.toml" | tr -d '[]')"
  for section in $sections; do
    read_toml_section_array "$DIR/conf/chmod.toml" "$section" "permissions" | while IFS='' read -r file || [ -n "$file" ]; do
      [ -n "$file" ] || continue
      # read_toml_section_array extracts the first quoted string, which is the mode;
      # for chmod we need the path. Extract path from the raw TOML line instead.
      true
    done
  done
  # Use a direct grep approach for chmod.toml structured entries
  grep -oP 'path\s*=\s*"\K[^"]+' "$DIR/conf/chmod.toml" | while IFS='' read -r file || [ -n "$file" ]; do
    [ -n "$file" ] || continue
    if [ ! -e "$DIR/symlinks/$file" ] && [ ! -L "$DIR/symlinks/$file" ]; then
      printf "%sERROR: chmod.toml references missing: symlinks/%s%s\n" "${RED}" "$file" "${NC}" >&2
      echo 1 > "$err_file"
    fi
  done
  result="$(cat "$err_file")"; rm -f "$err_file"
  [ "$result" -eq 0 ] || return 1
)}

# Validate TOML file syntax (no trailing whitespace).
test_toml_syntax()
{(
  log_stage "Validating TOML file syntax"
  errors=0
  for toml in "$DIR"/conf/*.toml; do
    [ -f "$toml" ] || continue
    name="$(basename "$toml")"
    lineno=0
    while IFS='' read -r line || [ -n "$line" ]; do
      lineno=$((lineno + 1))
      # Trailing whitespace
      case "$line" in
        *[[:space:]])
          printf "%sERROR: %s:%d trailing whitespace%s\n" "${RED}" "$name" "$lineno" "${NC}" >&2
          errors=$((errors + 1))
          ;;
      esac
    done < "$toml"
    log_verbose "$name: syntax OK"
  done
  [ "$errors" -eq 0 ] || return 1
)}

# Check that categories used in TOML files are consistent across config files.
test_category_consistency()
{(
  log_stage "Validating category consistency"
  [ -f "$DIR/conf/manifest.toml" ] || { log_verbose "No manifest.toml, skipping"; return 0; }

  manifest_cats="$(grep -E '^\[.+\]$' "$DIR/conf/manifest.toml" | tr -d '[]' | sort -u)"

  errors=0
  for toml in "$DIR"/conf/symlinks.toml "$DIR"/conf/chmod.toml "$DIR"/conf/packages.toml; do
    [ -f "$toml" ] || continue
    name="$(basename "$toml")"
    cats="$(grep -E '^\[.+\]$' "$toml" | tr -d '[]' | sort -u)"
    for cat in $cats; do
      # Categories with hyphens (e.g. "arch-desktop") — check each part
      for part in $(echo "$cat" | tr '-' ' '); do
        if ! echo "$manifest_cats" | grep -qxF "$part"; then
          # Check if it's a known profile category
          if [ -f "$DIR/conf/profiles.toml" ] && grep -qE "^\[$part\]$" "$DIR/conf/profiles.toml" 2>/dev/null; then
            continue
          fi
          printf "%sWARNING: %s uses category '%s' not in manifest.toml%s\n" "${YELLOW}" "$name" "$part" "${NC}" >&2
        fi
      done
    done
  done
  [ "$errors" -eq 0 ] || return 1
)}

# Check for empty sections in TOML files.
test_empty_sections()
{(
  log_stage "Checking for empty sections"
  errors=0
  for toml in "$DIR"/conf/*.toml; do
    [ -f "$toml" ] || continue
    name="$(basename "$toml")"
    prev_section=""
    while IFS='' read -r line || [ -n "$line" ]; do
      case "$line" in ''|\#*) continue ;; esac
      if echo "$line" | grep -qE '^\[.+\]$'; then
        section="${line#\[}"; section="${section%\]}"
        if [ -n "$prev_section" ]; then
          printf "%sERROR: %s section [%s] is empty%s\n" "${RED}" "$name" "$prev_section" "${NC}" >&2
          errors=$((errors + 1))
        fi
        prev_section="$section"
      else
        prev_section=""
      fi
    done < "$toml"
    # Check last section
    if [ -n "$prev_section" ]; then
      printf "%sERROR: %s section [%s] is empty%s\n" "${RED}" "$name" "$prev_section" "${NC}" >&2
      errors=$((errors + 1))
    fi
  done
  [ "$errors" -eq 0 ] || return 1
)}

# Execute a specific test when run directly: sh test-config.sh <test_name>
case "$0" in
  *test-config.sh)
    [ $# -ge 1 ] && "test_$1"
    ;;
esac
