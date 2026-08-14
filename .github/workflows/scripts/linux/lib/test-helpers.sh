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

# List profile names (top-level section headers) from profiles.toml.
list_available_profiles() {
  [ -f "$DIR/conf/profiles.toml" ] || return 1
  grep -E '^\[.+\]$' "$DIR/conf/profiles.toml" | tr -d '[]'
}
