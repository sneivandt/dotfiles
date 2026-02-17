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

is_shell_script() {
  [ -f "$1" ] || return 1
  case "$(head -n 1 "$1")" in
    '#!/bin/sh'*|'#!/bin/bash'*|'#!/usr/bin/env sh'*|'#!/usr/bin/env bash'*) return 0 ;;
  esac
  return 1
}

# Read values from an INI section.  Prints one line per entry.
read_ini_section() {
  local file="$1" section="$2" in_section=0
  [ -f "$file" ] || return 1
  while IFS='' read -r line || [ -n "$line" ]; do
    case "$line" in ''|\#*) continue ;; esac
    if [ "${line#\[}" != "$line" ]; then
      [ "$in_section" -eq 1 ] && return 0
      local name="${line#\[}"; name="${name%\]}"
      [ "$name" = "$section" ] && in_section=1
      continue
    fi
    [ "$in_section" -eq 1 ] && echo "$line"
  done < "$file"
}

# List profile names (section headers) from profiles.ini.
list_available_profiles() {
  [ -f "$DIR/conf/profiles.ini" ] || return 1
  grep -E '^\[.+\]$' "$DIR/conf/profiles.ini" | tr -d '[]'
}

# Parse a profile section into PROFILE_INCLUDE / PROFILE_EXCLUDE.
parse_profile() {
  local profile="$1" in_profile=0 found=0
  PROFILE_INCLUDE=""; PROFILE_EXCLUDE=""
  [ -f "$DIR/conf/profiles.ini" ] || return 1
  while IFS='' read -r line || [ -n "$line" ]; do
    case "$line" in ''|\#*) continue ;; esac
    if [ "${line#\[}" != "$line" ]; then
      local name="${line#\[}"; name="${name%\]}"
      if [ "$name" = "$profile" ]; then in_profile=1; found=1; else in_profile=0; fi
      continue
    fi
    if [ "$in_profile" -eq 1 ]; then
      local key="${line%%=*}" value="${line#*=}"
      key="$(echo "$key" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      value="$(echo "$value" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      case "$key" in
        include) PROFILE_INCLUDE="$value" ;; exclude) PROFILE_EXCLUDE="$value" ;;
      esac
    fi
  done < "$DIR/conf/profiles.ini"
  [ "$found" -eq 1 ]
}
