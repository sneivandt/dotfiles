#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# dotfiles.sh
# -----------------------------------------------------------------------------
# Entry point for the *nix (Arch‑focused) dotfiles workflow.
#
# Responsibilities:
#   * Parse top‑level CLI flags / modes (install, uninstall, test, help).
#   * Prevent execution as root (operations are intended for an unprivileged
#     user; elevation is performed ad‑hoc inside specific tasks where needed).
#   * Source shared logging helpers and higher‑level command orchestrators.
#
# Behavior & Idempotency:
#   Re‑running the same mode should only perform missing work. Individual
#   tasks are defensive (checking existing state before mutating it).
#
# Exit Codes:
#   0  Success / help displayed.
#   1  getopt parsing failure or explicit log_error invocation.
#
# Flags (forwarded via $OPT after getopt normalisation):
#   -v  Enable verbose logging.
#   --profile <name>  Use predefined profile for sparse checkout filtering.
#   --dry-run  Perform dry run without system modifications (install/uninstall only).
#
# Usage Examples:
#   ./dotfiles.sh --install --profile arch-desktop    # Arch with GUI.
#   ./dotfiles.sh --install --profile arch            # Arch CLI only.
#   ./dotfiles.sh -I --dry-run        # Preview install without changes (verbose auto-enabled).
#   ./dotfiles.sh -U            # Uninstall symlinks.
#   ./dotfiles.sh --test -v     # Run analyzers / linters with verbose output.
#
# Implementation Notes:
#   * getopt is used to provide consistent long/short option handling while
#     preserving a single aggregated $OPT evaluated by helper predicates.
#   * No work is performed directly in this file beyond dispatching.
# -----------------------------------------------------------------------------

DIR="$(dirname "$(readlink -f "$0")")"
export DIR

# Profile selection for sparse checkout filtering
PROFILE=""
export PROFILE

# Logging helpers (log_error, log_usage, log_stage, etc.).
. "$DIR"/src/linux/logger.sh

# Utility functions (profile management, INI parsing, etc.).
. "$DIR"/src/linux/utils.sh

# Guard: refuse to run as root to avoid polluting /root with user config and
# accidental privilege escalations inside tasks that assume normal user perms.
if [ "$(id -u)" = 0 ]; then
  log_error "$(basename "$0") can not be run as root."
fi

# High‑level orchestration functions (do_install, do_uninstall, do_test).
. "$DIR"/src/linux/commands.sh

# parse_profile_arg
#
# Parses --profile argument from getopt-normalized options.
# Must be called after 'eval set -- "$OPT"'.
#
# Globals read/set:
#   PROFILE  Set to profile value if --profile argument present
#
# Result:
#   0 success
parse_profile_arg()
{
  while true; do
    case "$1" in
      --profile)
        PROFILE="$2"
        if [ -z "$PROFILE" ]; then
          log_error "Profile name cannot be empty"
        fi
        shift 2
        ;;
      --)
        shift
        break
        ;;
      *)
        shift
        ;;
    esac
  done
}

# resolve_profile
#
# Resolves the profile to use: from CLI arg, persisted config, or interactive prompt.
# Persists the selected profile for future use.
# Validates that the specified profile exists in profiles.ini.
#
# Globals read/set:
#   PROFILE  Profile name (may be empty on input, populated on output)
#
# Result:
#   0 success, exits on error
resolve_profile()
{
  # If profile already specified via CLI, validate it exists
  if [ -n "$PROFILE" ]; then
    if ! list_available_profiles | grep -qx "$PROFILE"; then
      log_error "Profile '$PROFILE' not found in profiles.ini"
    fi
    log_verbose "Using profile from command line: $PROFILE"
    persist_profile "$PROFILE"
    return 0
  fi

  # Try to get persisted profile
  if PROFILE="$(get_persisted_profile)"; then
    log_verbose "Using persisted profile: $PROFILE"
    return 0
  fi

  # No profile specified or persisted, prompt interactively
  log_stage "No profile specified"
  echo "" >&2
  if PROFILE="$(prompt_profile_selection)"; then
    echo "" >&2
    log_verbose "Selected profile: $PROFILE"
    persist_profile "$PROFILE"
    export PROFILE
    return 0
  else
    log_error "Profile selection failed"
  fi
}

case ${1:-} in
  -I* | --install)
    # Full install path for selected profile.
    OPT="$(getopt -o Iv -l install,profile:,dry-run -n "$(basename "$0")" -- "$@")" \
      || exit 1
    eval set -- "$OPT"
    parse_profile_arg "$@"
    export OPT
    export PROFILE
    resolve_profile
    printf "${BLUE}:: Using profile: %s${NC}\n" "$PROFILE"
    if is_dry_run; then
      # shellcheck disable=SC2059  # BLUE and NC are intentional color codes
      printf "${BLUE}:: DRY-RUN MODE: No system modifications will be made${NC}\n"
    fi
    do_install
    ;;
  -T* | --test)
    # Static analysis / lint checks (shellcheck, PSScriptAnalyzer).
    OPT="$(getopt -o Tv -l test -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_test
    ;;
  -U* | --uninstall)
    # Remove installed symlinks for selected profile.
    OPT="$(getopt -o Uv -l uninstall,profile:,dry-run -n "$(basename "$0")" -- "$@")" \
      || exit 1
    eval set -- "$OPT"
    parse_profile_arg "$@"
    export OPT
    export PROFILE
    resolve_profile
    printf "${BLUE}:: Using profile: %s${NC}\n" "$PROFILE"
    if is_dry_run; then
      # shellcheck disable=SC2059  # BLUE and NC are intentional color codes
      printf "${BLUE}:: DRY-RUN MODE: No system modifications will be made${NC}\n"
    fi
    do_uninstall
    ;;
  -h | --help)
    # Show usage information only.
    OPT="$(getopt -o h -l help -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    log_usage
    ;;
  *)
    # Fallback: any other input falls through to usage.
    OPT="$(getopt -o -l -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    log_usage
    ;;
esac
