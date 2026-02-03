#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# logger.sh
# -----------------------------------------------------------------------------
# Lightweight logging utilities used across shell tasks. Designed to keep
# output minimal yet informative for both interactive and CI contexts.
#
# Conventions:
#   * Stage messages prefixed with '::' to visually group related actions.
#   * log_error exits immediately (caller usually aborts entire run).
#   * log_stage prints only once per conceptual stage even if called multiple
#     times (suppresses redundant noise) by leveraging a private _work flag.
#
# Expected Environment Variables:
#   FILE  Test file name (used by log_fail, set by test harness)
#   TEST  Test name (used by log_fail, set by test harness)
# -----------------------------------------------------------------------------

# Colors
RED=""
GREEN=""
BLUE=""
YELLOW=""
NC=""

if [ -t 1 ]; then
  RED="\033[0;31m"
  GREEN="\033[0;32m"
  BLUE="\033[0;34m"
  YELLOW="\033[1;33m"
  NC="\033[0m" # No Color
fi

export RED GREEN BLUE YELLOW NC

# log_error
#
# Print an error message (stderr semantics not required for current usage)
# then terminate with non‑zero exit to propagate failure to orchestrator.
#
# Args:
#   $1 human readable error description
log_error()
{
  # shellcheck disable=SC2059  # RED and NC are controlled color codes
  printf "${RED}ERROR: %s${NC}\n" "$1"
  exit 1
}

# log_fail
#
# Emit a standardized test failure line consumed by any higher-level test
# harness. Relies on externally set $FILE and $TEST identifiers.
#
# Args:
#   $1 line number
#   $2 failure description
log_fail()
{
  # FILE and TEST are set by test harness
  # shellcheck disable=SC2154
  # shellcheck disable=SC2059  # RED and NC are controlled color codes
  printf "${RED}FAIL %s %s %s : %s${NC}\n" "$FILE" "$TEST" "$1" "$2"
}

# log_usage
#
# Display CLI help text. Called on explicit -h/--help or invalid invocation.
log_usage()
{
  echo "Usage:"
  echo "  $(basename "$0")"
  echo "  $(basename "$0") {-I | --install}   [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]"
  echo "  $(basename "$0") {-U | --uninstall} [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]"
  echo "  $(basename "$0") {-T | --test}      [-v]"
  echo "  $(basename "$0") {-h | --help}"
  echo
  echo "Options:"
  echo "  --profile PROFILE     Use predefined profile for sparse checkout"
  echo "                        Available: base, arch, arch-desktop, windows"
  echo "                        If not specified:"
  echo "                          1. Uses previously persisted profile (if exists)"
  echo "                          2. Prompts interactively to select a profile"
  echo "                        Selected profile is persisted for future runs."
  echo "  -v                    Enable verbose logging"
  echo "  --dry-run             Perform a dry run without making system modifications."
  echo "                        Logs all actions that would be taken. Verbose logging"
  echo "                        is automatically enabled in dry-run mode for detailed"
  echo "                        output."
  echo "  --skip-os-detection   Skip automatic OS detection overrides. Allows testing"
  echo "                        arch profile on non-Arch systems. Primarily for CI"
  echo "                        testing to ensure profile differentiation."
  exit
}

# log_verbose
#
# Print a verbose message if the -v flag is set.
#
# Args:
#   $1 message
log_verbose()
{
  if is_flag_set "v"; then
    # shellcheck disable=SC2059  # YELLOW and NC are controlled color codes
    printf "${YELLOW}VERBOSE: %s${NC}\n" "$*"
  fi
}

# log_dry_run
#
# Print a dry run message indicating what would happen. Always prints in dry run
# mode regardless of verbose flag setting to provide visibility into intended actions.
#
# Args:
#   $1 action description (e.g., "Would install package: foo")
log_dry_run()
{
  if is_dry_run; then
    # shellcheck disable=SC2059  # GREEN and NC are controlled color codes
    printf "${GREEN}DRY-RUN: %s${NC}\n" "$*"
  fi
}

# log_stage
#
# Print a stage heading exactly once for a multi-step logical unit. Subsequent
# calls within the same subshell no-op until _work resets (new subshell or
# script invocation). This keeps logs concise when a task loops.
#
# Args:
#   $1 stage description (imperative present tense preferred)
log_stage()
{
  if [ "${_work-unset}" = "unset" ] \
    || ! $_work; then
    _work=true
    # shellcheck disable=SC2059  # BLUE and NC are controlled color codes
    printf "${BLUE}:: %s${NC}\n" "$1"
  fi
}

# log_profile
#
# Print the currently selected profile name.
#
# Args:
#   $1 profile name
log_profile()
{
  # shellcheck disable=SC2059  # BLUE and NC are controlled color codes
  printf "${BLUE}:: Using profile: %s${NC}\n" "$1"
}
