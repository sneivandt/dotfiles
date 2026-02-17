#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-config.sh — Configuration validation tests for INI files.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

# shellcheck disable=SC2154
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Validate that files listed in manifest.ini exist in symlinks/.
test_config_validation()
{(
  log_stage "Validating configuration consistency"
  [ -f "$DIR/conf/manifest.ini" ] || { log_verbose "No manifest.ini, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/manifest.ini" | tr -d '[]')"
  for section in $sections; do
    entries="$(read_ini_section "$DIR/conf/manifest.ini" "$section")" || true
    echo "$entries" | while IFS='' read -r file || [ -n "$file" ]; do
      [ -n "$file" ] || continue
      if [ ! -e "$DIR/symlinks/$file" ] && [ ! -L "$DIR/symlinks/${file%/}" ]; then
        printf "%sERROR: manifest.ini [%s] references missing: symlinks/%s%s\n" "${RED}" "$section" "$file" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  done

  # Validate profiles
  if [ -f "$DIR/conf/profiles.ini" ]; then
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

# Validate that files in symlinks.ini exist in symlinks/.
test_symlinks_validation()
{(
  log_stage "Validating symlinks.ini references"
  [ -f "$DIR/conf/symlinks.ini" ] || { log_verbose "No symlinks.ini, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/symlinks.ini" | tr -d '[]')"
  for section in $sections; do
    entries="$(read_ini_section "$DIR/conf/symlinks.ini" "$section")" || true
    echo "$entries" | while IFS='' read -r entry || [ -n "$entry" ]; do
      [ -n "$entry" ] || continue
      src="${entry%%=*}"
      src="$(echo "$src" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      if [ ! -e "$DIR/symlinks/$src" ] && [ ! -L "$DIR/symlinks/$src" ]; then
        printf "%sERROR: symlinks.ini [%s] source missing: symlinks/%s%s\n" "${RED}" "$section" "$src" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  done
  result="$(cat "$err_file")"; rm -f "$err_file"
  [ "$result" -eq 0 ] || return 1
)}

# Validate that files in chmod.ini exist in symlinks/.
test_chmod_validation()
{(
  log_stage "Validating chmod.ini references"
  [ -f "$DIR/conf/chmod.ini" ] || { log_verbose "No chmod.ini, skipping"; return 0; }

  err_file="$(mktemp)"; echo 0 > "$err_file"
  sections="$(grep -E '^\[.+\]$' "$DIR/conf/chmod.ini" | tr -d '[]')"
  for section in $sections; do
    entries="$(read_ini_section "$DIR/conf/chmod.ini" "$section")" || true
    echo "$entries" | while IFS='' read -r entry || [ -n "$entry" ]; do
      [ -n "$entry" ] || continue
      file="${entry%%=*}"
      file="$(echo "$file" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      if [ ! -e "$DIR/symlinks/$file" ] && [ ! -L "$DIR/symlinks/$file" ]; then
        printf "%sERROR: chmod.ini [%s] references missing: symlinks/%s%s\n" "${RED}" "$section" "$file" "${NC}" >&2
        echo 1 > "$err_file"
      fi
    done
  done
  result="$(cat "$err_file")"; rm -f "$err_file"
  [ "$result" -eq 0 ] || return 1
)}

# Validate INI file syntax (sections, no trailing whitespace, no duplicates).
test_ini_syntax()
{(
  log_stage "Validating INI file syntax"
  errors=0
  for ini in "$DIR"/conf/*.ini; do
    [ -f "$ini" ] || continue
    name="$(basename "$ini")"
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
      # Malformed section header
      case "$line" in
        \[*) case "$line" in *\]) ;; *)
          printf "%sERROR: %s:%d malformed section header%s\n" "${RED}" "$name" "$lineno" "${NC}" >&2
          errors=$((errors + 1))
          ;; esac ;;
      esac
    done < "$ini"
    log_verbose "$name: syntax OK"
  done
  [ "$errors" -eq 0 ] || return 1
)}

# Check that categories used in INI files are consistent across config files.
test_category_consistency()
{(
  log_stage "Validating category consistency"
  [ -f "$DIR/conf/manifest.ini" ] || { log_verbose "No manifest.ini, skipping"; return 0; }

  manifest_cats="$(grep -E '^\[.+\]$' "$DIR/conf/manifest.ini" | tr -d '[]' | sort -u)"

  errors=0
  for ini in "$DIR"/conf/symlinks.ini "$DIR"/conf/chmod.ini "$DIR"/conf/packages.ini; do
    [ -f "$ini" ] || continue
    name="$(basename "$ini")"
    cats="$(grep -E '^\[.+\]$' "$ini" | tr -d '[]' | sort -u)"
    for cat in $cats; do
      # Categories with commas (e.g. "arch,aur") — check each part
      for part in $(echo "$cat" | tr ',' ' '); do
        case "$part" in aur) continue ;; esac  # aur is packages-only
        if ! echo "$manifest_cats" | grep -qxF "$part"; then
          # Check if it's a known profile category
          if [ -f "$DIR/conf/profiles.ini" ] && grep -qE "^\[$part\]$" "$DIR/conf/profiles.ini" 2>/dev/null; then
            continue
          fi
          printf "%sWARNING: %s uses category '%s' not in manifest.ini%s\n" "${YELLOW}" "$name" "$part" "${NC}" >&2
        fi
      done
    done
  done
  [ "$errors" -eq 0 ] || return 1
)}

# Check for empty sections in INI files.
test_empty_sections()
{(
  log_stage "Checking for empty sections"
  errors=0
  for ini in "$DIR"/conf/*.ini; do
    [ -f "$ini" ] || continue
    name="$(basename "$ini")"
    prev_section=""
    while IFS='' read -r line || [ -n "$line" ]; do
      case "$line" in ''|\#*) continue ;; esac
      if [ "${line#\[}" != "$line" ]; then
        section="${line#\[}"; section="${section%\]}"
        if [ -n "$prev_section" ]; then
          printf "%sERROR: %s section [%s] is empty%s\n" "${RED}" "$name" "$prev_section" "${NC}" >&2
          errors=$((errors + 1))
        fi
        prev_section="$section"
      else
        prev_section=""
      fi
    done < "$ini"
    # Check last section
    if [ -n "$prev_section" ]; then
      printf "%sERROR: %s section [%s] is empty%s\n" "${RED}" "$name" "$prev_section" "${NC}" >&2
      errors=$((errors + 1))
    fi
  done
  [ "$errors" -eq 0 ] || return 1
)}
