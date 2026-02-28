#!/bin/sh
# shellcheck disable=SC2034
# Minimal helpers for CI test scripts.
set -o errexit
set -o nounset

RED="" GREEN="" YELLOW="" NC=""
if [ -t 1 ]; then
  RED="\033[0;31m"; GREEN="\033[0;32m"; YELLOW="\033[1;33m"; NC="\033[0m"
fi

log_stage()  { printf ":: %s\n" "$1"; }
log_verbose(){ printf "   %s\n" "$1"; }
log_error()  { printf "%sERROR: %s%s\n" "${RED}" "$1" "${NC}" >&2; exit 1; }

is_program_installed() { command -v "$1" >/dev/null 2>&1; }

# Retry a command up to N times with D seconds between attempts.
retry_cmd() {
  _retries="${1:-3}"; _delay="${2:-10}"; shift 2
  _i=1
  while true; do
    "$@" && return 0
    [ "$_i" -ge "$_retries" ] && return 1
    printf "   Attempt %d/%d failed, retrying in %ds...\n" "$_i" "$_retries" "$_delay"
    sleep "$_delay"
    _i=$((_i + 1))
  done
}

is_shell_script() {
  [ -f "$1" ] || return 1
  case "$(head -n 1 "$1")" in
    '#!/bin/sh'*|'#!/bin/bash'*|'#!/usr/bin/env sh'*|'#!/usr/bin/env bash'*) return 0 ;;
  esac
  return 1
}

# Read array values from a TOML section.  Prints one value per line.
# Supports both inline arrays and multi-line arrays.
# Handles simple strings ("value") and structured objects ({ name = "x", ... })
# by extracting the first quoted string from each element.
read_toml_section_array() {
  local file="$1" section="$2" key="$3" in_section=0 in_array=0
  [ -f "$file" ] || return 1
  while IFS='' read -r line || [ -n "$line" ]; do
    case "$line" in ''|\#*) continue ;; esac
    # Detect section header [name] but not [name.values] subtables
    if echo "$line" | grep -qE '^\['; then
      if [ "$in_array" -eq 1 ]; then return 0; fi
      local name
      name="$(echo "$line" | sed 's/^\[//;s/\]$//')"
      if [ "$name" = "$section" ]; then in_section=1; else in_section=0; fi
      continue
    fi
    if [ "$in_section" -eq 1 ]; then
      # Match "key = [" to start array
      case "$line" in
        *"$key"*=*)
          in_array=1
          # Check for inline array on same line: key = ["a", "b"]
          local rhs="${line#*=}"
          rhs="$(echo "$rhs" | sed 's/^[[:space:]]*//')"
          case "$rhs" in
            '['*']')
              # Inline array — extract quoted strings
              echo "$rhs" | sed 's/[][{}]//g' | tr ',' '\n' | while IFS='' read -r item; do
                item="$(echo "$item" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
                # Extract first quoted string
                val="$(echo "$item" | sed -n 's/.*"\([^"]*\)".*/\1/p')"
                [ -n "$val" ] && echo "$val"
              done
              return 0
              ;;
            '['*)
              # Multi-line array starts here (items on subsequent lines)
              ;;
          esac
          continue
          ;;
      esac
      if [ "$in_array" -eq 1 ]; then
        case "$line" in
          *']'*) return 0 ;;
        esac
        # Extract first quoted string from array element
        val="$(echo "$line" | sed -n 's/.*"\([^"]*\)".*/\1/p')"
        [ -n "$val" ] && echo "$val"
      fi
    fi
  done < "$file"
}

# List profile names (top-level section headers) from profiles.toml.
list_available_profiles() {
  [ -f "$DIR/conf/profiles.toml" ] || return 1
  grep -E '^\[.+\]$' "$DIR/conf/profiles.toml" | tr -d '[]'
}

# Parse a profile section into PROFILE_INCLUDE / PROFILE_EXCLUDE.
parse_profile() {
  local profile="$1"
  PROFILE_INCLUDE=""; PROFILE_EXCLUDE=""
  [ -f "$DIR/conf/profiles.toml" ] || return 1
  local include_vals exclude_vals
  include_vals="$(read_toml_section_array "$DIR/conf/profiles.toml" "$profile" "include")" || true
  exclude_vals="$(read_toml_section_array "$DIR/conf/profiles.toml" "$profile" "exclude")" || true
  PROFILE_INCLUDE="$(echo "$include_vals" | paste -sd, -)"
  PROFILE_EXCLUDE="$(echo "$exclude_vals" | paste -sd, -)"
  # Check profile exists by looking for its section header
  grep -qE "^\[$profile\]$" "$DIR/conf/profiles.toml" 2>/dev/null
}
