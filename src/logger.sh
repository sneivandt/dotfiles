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
# -----------------------------------------------------------------------------

# log_error
#
# Print an error message (stderr semantics not required for current usage)
# then terminate with non‑zero exit to propagate failure to orchestrator.
#
# Args:
#   $1 human readable error description
log_error()
{
  echo "ERROR: $1"
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
  echo "FAIL $FILE $TEST $1 : $2"
}

# log_usage
#
# Display CLI help text. Called on explicit -h/--help or invalid invocation.
log_usage()
{
  echo "Usage:"
  echo "  $(basename "$0")"
  echo "  $(basename "$0") {-I --install}   [-g] [-p] [-s]"
  echo "  $(basename "$0") {-U --uninstall} [-g]"
  echo "  $(basename "$0") {-T --test}"
  echo "  $(basename "$0") {-h --help}"
  echo
  echo "Options:"
  echo "  -g  Configure GUI environment"
  echo "  -p  Install system packages"
  echo "  -s  Install systemd units"
  exit
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
    || ! $_work
  then
    _work=true
    echo ":: $1..."
  fi
}