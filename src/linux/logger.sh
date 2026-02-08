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
#   * Output written through log_* helpers is mirrored to a persistent log
#     file for troubleshooting. Direct command output must be explicitly logged.
#   * Counters track operations for summary reporting (only in non-dry-run mode).
#
# Expected Environment Variables:
#   FILE  Test file name (used by log_fail, set by test harness)
#   TEST  Test name (used by log_fail, set by test harness)
#   DOTFILES_LOG_FILE  Path to persistent log file (set on init)
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

# -----------------------------------------------------------------------------
# Persistent Logging and Counters
# -----------------------------------------------------------------------------

# Initialize log file location (XDG Base Directory compatible)
DOTFILES_LOG_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles"
DOTFILES_LOG_FILE="$DOTFILES_LOG_DIR/install.log"
export DOTFILES_LOG_FILE
export DOTFILES_LOG_DIR

# Operation counters (global, persisted across subshells via files)
_COUNTER_DIR="${DOTFILES_LOG_DIR}/counters"

# init_logging
#
# Initialize logging system: create log directory and file, reset counters.
# Should be called once at the start of install/uninstall operations.
init_logging()
{
  # Create log directory if it doesn't exist
  mkdir -p "$DOTFILES_LOG_DIR"
  mkdir -p "$_COUNTER_DIR"

  # Initialize log file with timestamp
  {
    echo "=========================================="
    echo "Dotfiles $(date '+%Y-%m-%d %H:%M:%S')"
    echo "Profile: ${PROFILE:-unset}"
    echo "=========================================="
  } > "$DOTFILES_LOG_FILE"

  # Reset all counters
  rm -f "$_COUNTER_DIR"/*
}

# _log_to_file
#
# Internal: write a message to the persistent log file.
# Strips ANSI color codes for clean file output.
#
# Args:
#   $* message to log
_log_to_file()
{
  if [ -n "${DOTFILES_LOG_FILE:-}" ] && [ -f "$DOTFILES_LOG_FILE" ]; then
    # Strip ANSI color codes and write to file
    # Use \033 instead of \x1b for better POSIX portability
    echo "$*" | sed 's/\033\[[0-9;]*m//g' >> "$DOTFILES_LOG_FILE" 2>/dev/null || true
  fi
}

# increment_counter
#
# Increment a named counter for summary statistics.
#
# Args:
#   $1 counter name (e.g., "packages_installed", "symlinks_created")
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
increment_counter()
{
  local counter_name="$1"
  local counter_file="$_COUNTER_DIR/$counter_name"

  # Read current value (default to 0)
  local current=0
  if [ -f "$counter_file" ]; then
    current="$(cat "$counter_file")"
  fi

  # Increment and write back
  echo "$((current + 1))" > "$counter_file"
}

# get_counter
#
# Get the current value of a named counter.
#
# Args:
#   $1 counter name
#
# Returns:
#   Counter value (0 if counter doesn't exist)
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
get_counter()
{
  local counter_name="$1"
  local counter_file="$_COUNTER_DIR/$counter_name"

  if [ -f "$counter_file" ]; then
    cat "$counter_file"
  else
    echo "0"
  fi
}

# log_summary
#
# Print a summary of all operations performed during install/uninstall.
# Should be called at the end of install/uninstall operations.
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
# shellcheck disable=SC2155  # get_counter always succeeds and returns 0 on error
log_summary()
{
  # shellcheck disable=SC2059  # BLUE and NC are controlled color codes
  printf "${BLUE}:: Installation Summary${NC}\n"

  local packages_installed="$(get_counter "packages_installed")"
  local aur_packages_installed="$(get_counter "aur_packages_installed")"
  local symlinks_created="$(get_counter "symlinks_created")"
  local vscode_extensions_installed="$(get_counter "vscode_extensions_installed")"
  local powershell_modules_installed="$(get_counter "powershell_modules_installed")"
  local systemd_units_enabled="$(get_counter "systemd_units_enabled")"
  local fonts_cache_updated="$(get_counter "fonts_cache_updated")"
  local chmod_applied="$(get_counter "chmod_applied")"
  local symlinks_removed="$(get_counter "symlinks_removed")"

  # Build summary message
  local summary=""

  if [ "$packages_installed" -gt 0 ]; then
    summary="${summary}   Packages installed: $packages_installed\n"
  fi

  if [ "$aur_packages_installed" -gt 0 ]; then
    summary="${summary}   AUR packages installed: $aur_packages_installed\n"
  fi

  if [ "$symlinks_created" -gt 0 ]; then
    summary="${summary}   Symlinks created: $symlinks_created\n"
  fi

  if [ "$symlinks_removed" -gt 0 ]; then
    summary="${summary}   Symlinks removed: $symlinks_removed\n"
  fi

  if [ "$vscode_extensions_installed" -gt 0 ]; then
    summary="${summary}   VS Code extensions installed: $vscode_extensions_installed\n"
  fi

  if [ "$powershell_modules_installed" -gt 0 ]; then
    summary="${summary}   PowerShell modules installed: $powershell_modules_installed\n"
  fi

  if [ "$systemd_units_enabled" -gt 0 ]; then
    summary="${summary}   Systemd units enabled: $systemd_units_enabled\n"
  fi

  if [ "$fonts_cache_updated" -gt 0 ]; then
    summary="${summary}   Font cache updated: $fonts_cache_updated times\n"
  fi

  if [ "$chmod_applied" -gt 0 ]; then
    summary="${summary}   File permissions set: $chmod_applied\n"
  fi

  if [ -z "$summary" ]; then
    echo "   No changes made (all components already configured)"
  else
    # shellcheck disable=SC2059
    printf "$summary"
  fi

  # Log file location
  if [ -f "$DOTFILES_LOG_FILE" ]; then
    echo "   Log file: $DOTFILES_LOG_FILE"
  fi

  # Also write summary to log file
  _log_to_file ""
  _log_to_file "=========================================="
  _log_to_file "Installation Summary"
  _log_to_file "=========================================="
  if [ -z "$summary" ]; then
    _log_to_file "No changes made (all components already configured)"
  else
    echo "$summary" | sed 's/\\n/\n/g' >> "$DOTFILES_LOG_FILE" 2>/dev/null || true
  fi
}

# log_progress
#
# Print a progress message at the default log level (always visible).
# This provides feedback about what is being checked/processed without
# being as detailed as verbose mode.
#
# Args:
#   $1 progress description (e.g., "Checking packages", "Installing symlinks")
log_progress()
{
  printf "   %s\n" "$*"
  _log_to_file "   $*"
}

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
  _log_to_file "ERROR: $1"
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
  echo "                        Available: base, arch, arch-desktop, desktop, windows"
  echo "                        If not specified:"
  echo "                          1. Uses previously persisted profile (if exists)"
  echo "                          2. Prompts interactively to select a profile"
  echo "                        Selected profile is persisted for future runs."
  echo "  -v                    Enable verbose logging"
  echo "  --dry-run             Perform a dry run without making system modifications."
  echo "                        Logs all actions that would be taken. Use -v for"
  echo "                        detailed output."
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
  _log_to_file "VERBOSE: $*"
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
    _log_to_file "DRY-RUN: $*"
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
    _log_to_file ":: $1"
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
  _log_to_file ":: Using profile: $1"
}
