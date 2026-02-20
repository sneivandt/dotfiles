#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-uninstall.sh — Tests for the install/uninstall command round-trip.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root), BINARY_PATH (path to pre-built binary)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Verify that a path is a symlink pointing into the dotfiles repo.
assert_symlink() {
  if [ ! -L "$1" ]; then
    printf "%sERROR: expected symlink: %s%s\n" "${RED}" "$1" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ symlink exists: $1"
}

# Verify that a path exists as a regular file or directory (not a symlink).
assert_materialized() {
  if [ ! -e "$1" ]; then
    printf "%sERROR: expected file/dir after uninstall: %s%s\n" "${RED}" "$1" "${NC}" >&2
    return 1
  fi
  if [ -L "$1" ]; then
    printf "%sERROR: expected materialized file, still a symlink: %s%s\n" "${RED}" "$1" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ materialized: $1"
}

# Test the full install → uninstall round-trip for the base profile.
test_install_uninstall_base_profile()
{(
  log_stage "Testing install/uninstall round-trip (base profile)"

  [ -n "${BINARY_PATH:-}" ] || log_error "BINARY_PATH is not set"
  [ -f "$BINARY_PATH" ] || log_error "Binary not found: $BINARY_PATH"

  # Run install
  log_verbose "Running install..."
  "$BINARY_PATH" --root "$DIR" -p base install
  log_verbose "Install complete"

  # Verify representative symlinks were created
  assert_symlink "$HOME/.bashrc"
  assert_symlink "$HOME/.zshrc"
  assert_symlink "$HOME/.config/git/config"

  # Run uninstall
  log_verbose "Running uninstall..."
  "$BINARY_PATH" --root "$DIR" -p base uninstall
  log_verbose "Uninstall complete"

  # After uninstall symlinks should be materialized as real files
  assert_materialized "$HOME/.bashrc"
  assert_materialized "$HOME/.zshrc"
  assert_materialized "$HOME/.config/git/config"
)}

# Run all tests when executed directly
case "$0" in
  *test-uninstall.sh)
    test_install_uninstall_base_profile
    echo "All uninstall tests passed"
    ;;
esac
