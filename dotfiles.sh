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
#   -g  Include GUI layer setup (fonts, desktop related dotfiles, VS Code).
#   -p  Install system packages (Arch pacman based).
#   -s  Install and enable user‑level systemd units.
#   -v  Enable verbose logging.
#   -q  Quiet mode (minimal output, useful for scripting).
#
# Usage Examples:
#   ./dotfiles.sh --install -gp    # Install including GUI + packages.
#   ./dotfiles.sh -U -g            # Uninstall GUI related symlinks.
#   ./dotfiles.sh --test -v        # Run analyzers / linters with verbose output.
#   ./dotfiles.sh --install -q     # Silent install for automation.
#
# Implementation Notes:
#   * getopt is used to provide consistent long/short option handling while
#     preserving a single aggregated $OPT evaluated by helper predicates.
#   * No work is performed directly in this file beyond dispatching.
# -----------------------------------------------------------------------------

DIR="$(dirname "$(readlink -f "$0")")"
export DIR

# Logging helpers (log_error, log_usage, log_stage, etc.).
. "$DIR"/src/logger.sh

# Guard: refuse to run as root to avoid polluting /root with user config and
# accidental privilege escalations inside tasks that assume normal user perms.
if [ "$(id -u)" = 0 ]; then
  log_error "$(basename "$0") can not be run as root."
fi

# High‑level orchestration functions (do_install, do_uninstall, do_test).
. "$DIR"/src/commands.sh

# Start timing if not in quiet mode
START_TIME=$(date +%s)

case ${1:-} in
  -I* | --install)
    # Full install path (optionally gated by -g -p -s sub‑flags).
    OPT="$(getopt -o Ipgqsv -l install,quiet -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_install
    ;;
  -T* | --test)
    # Static analysis / lint checks (shellcheck, PSScriptAnalyzer).
    OPT="$(getopt -o Tqv -l test,quiet -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_test
    ;;
  -U* | --uninstall)
    # Remove installed symlinks (respecting -g to include GUI layer paths).
    OPT="$(getopt -o Ugqv -l uninstall,quiet -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
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

# Print execution time if not in quiet mode
if ! is_flag_set "q"
then
  END_TIME=$(date +%s)
  ELAPSED=$((END_TIME - START_TIME))
  if [ "$ELAPSED" -ge 1 ]
  then
    printf "${GREEN}Completed in %d seconds${NC}\n" "$ELAPSED"
  fi
fi
