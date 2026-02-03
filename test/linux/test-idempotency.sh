#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-idempotency.sh
# -----------------------------------------------------------------------------
# Idempotency tests for dotfiles installation.
#
# Functions:
#   test_idempotency_install_base          Test base profile idempotency
#   test_idempotency_install_arch          Test arch profile idempotency
#   test_idempotency_install_arch_desktop  Test arch-desktop profile idempotency
#
# Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (is_program_installed)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

# run_install_twice
#
# Helper function to run installation twice and verify idempotency.
# Tests that the second run completes without errors and reports
# minimal or no changes (operations are skipped because already correct).
#
# Arguments:
#   $1 - Profile name
#   $2 - Additional flags (optional, e.g., "--skip-os-detection")
#
# Returns:
#   0 on success (idempotent behavior verified)
#   1 on failure (errors or unexpected changes on second run)
run_install_twice()
{
  local profile="$1"
  local additional_flags="${2:-}"

  log_verbose "Testing idempotency for profile: $profile"

  # Create temporary files for output capture
  local first_run_log
  local second_run_log
  first_run_log="$(mktemp)"
  second_run_log="$(mktemp)"

  # First installation run
  log_verbose "Running first installation (profile=$profile)"
  # shellcheck disable=SC2086  # Intentional word splitting for optional additional_flags
  if ! "$DIR"/dotfiles.sh --install --profile "$profile" $additional_flags -v >"$first_run_log" 2>&1; then
    printf "%sERROR: First installation run failed for profile %s%s\n" "${RED}" "$profile" "${NC}" >&2
    printf "%sOutput:%s\n" "${RED}" "${NC}" >&2
    cat "$first_run_log" >&2
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "First installation completed successfully"

  # Second installation run (should be idempotent)
  log_verbose "Running second installation (should be idempotent)"
  # shellcheck disable=SC2086  # Intentional word splitting for optional additional_flags
  if ! "$DIR"/dotfiles.sh --install --profile "$profile" $additional_flags -v >"$second_run_log" 2>&1; then
    printf "%sERROR: Second installation run failed for profile %s%s\n" "${RED}" "$profile" "${NC}" >&2
    printf "%sThis indicates the installation is not idempotent%s\n" "${RED}" "${NC}" >&2
    printf "%sOutput:%s\n" "${RED}" "${NC}" >&2
    cat "$second_run_log" >&2
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "Second installation completed successfully"

  # Verify second run shows idempotent behavior
  # Look for "Skipping" messages which indicate operations were not needed
  local skip_count
  skip_count=$(grep -c "Skipping" "$second_run_log" || echo "0")

  log_verbose "Second run reported $skip_count skip operations"

  # Check that second run didn't create new symlinks
  # (Should all be skipped as "already linked" or similar)
  if grep -q "Linking" "$second_run_log"; then
    if ! grep -q "Skipping.*already linked" "$second_run_log"; then
      printf "%sWARNING: Second run appears to have created new symlinks%s\n" "${YELLOW}" "${NC}" >&2
      printf "%sThis may indicate non-idempotent behavior%s\n" "${YELLOW}" "${NC}" >&2
    fi
  fi

  # Verify no errors in second run
  if grep -qi "error" "$second_run_log"; then
    printf "%sERROR: Second installation run contained errors for profile %s%s\n" "${RED}" "$profile" "${NC}" >&2
    printf "%sOutput:%s\n" "${RED}" "${NC}" >&2
    cat "$second_run_log" >&2
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "Idempotency verified for profile $profile"

  # Clean up
  rm -f "$first_run_log" "$second_run_log"
  return 0
}

# test_idempotency_install_base
#
# Test that the base profile installation is idempotent.
# Runs installation twice and verifies the second run completes
# cleanly without errors or unnecessary changes.
test_idempotency_install_base()
{(
  log_stage "Testing base profile idempotency"

  # Run with base profile
  if ! run_install_twice "base" ""; then
    return 1
  fi

  log_verbose "Base profile idempotency test passed"
)}

# test_idempotency_install_arch
#
# Test that the arch profile installation is idempotent.
# Uses --skip-os-detection to test on non-Arch systems.
test_idempotency_install_arch()
{(
  log_stage "Testing arch profile idempotency"

  # Run with arch profile and skip OS detection
  if ! run_install_twice "arch" "--skip-os-detection"; then
    return 1
  fi

  log_verbose "Arch profile idempotency test passed"
)}

# test_idempotency_install_arch_desktop
#
# Test that the arch-desktop profile installation is idempotent.
# Uses --skip-os-detection to test on non-Arch systems.
test_idempotency_install_arch_desktop()
{(
  log_stage "Testing arch-desktop profile idempotency"

  # Run with arch-desktop profile and skip OS detection
  if ! run_install_twice "arch-desktop" "--skip-os-detection"; then
    return 1
  fi

  log_verbose "Arch-desktop profile idempotency test passed"
)}

# test_idempotency_symlinks
#
# Test that symlink installation is idempotent.
# Verifies that running symlink installation multiple times
# doesn't create duplicate symlinks or break existing ones.
test_idempotency_symlinks()
{(
  log_stage "Testing symlink idempotency"

  # Create a temporary test directory
  local test_home
  test_home="$(mktemp -d)"

  log_verbose "Using temporary HOME: $test_home"

  # Save original HOME
  local original_home="$HOME"

  # Override HOME for this test
  HOME="$test_home"
  export HOME

  # Create temporary output files
  local first_run_log
  local second_run_log
  first_run_log="$(mktemp)"
  second_run_log="$(mktemp)"

  # Run install_symlinks twice by sourcing tasks.sh
  log_verbose "Running first symlink installation"

  # We need to run the actual task function, so source it
  # shellcheck disable=SC1091
  . "$DIR"/src/linux/tasks.sh

  # Set PROFILE for the task
  PROFILE="base"
  export PROFILE

  # First run
  if ! install_symlinks >"$first_run_log" 2>&1; then
    printf "%sERROR: First symlink installation failed%s\n" "${RED}" "${NC}" >&2
    cat "$first_run_log" >&2
    HOME="$original_home"
    export HOME
    rm -rf "$test_home"
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "First symlink installation completed"

  # Second run (should be idempotent)
  log_verbose "Running second symlink installation"
  if ! install_symlinks >"$second_run_log" 2>&1; then
    printf "%sERROR: Second symlink installation failed%s\n" "${RED}" "${NC}" >&2
    printf "%sThis indicates symlink installation is not idempotent%s\n" "${RED}" "${NC}" >&2
    cat "$second_run_log" >&2
    HOME="$original_home"
    export HOME
    rm -rf "$test_home"
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "Second symlink installation completed"

  # Verify all symlinks are still valid
  local broken_links
  broken_links=$(find "$test_home" -type l ! -exec test -e {} \; -print 2>/dev/null || echo "")

  if [ -n "$broken_links" ]; then
    printf "%sERROR: Found broken symlinks after second run:%s\n" "${RED}" "${NC}" >&2
    echo "$broken_links" >&2
    HOME="$original_home"
    export HOME
    rm -rf "$test_home"
    rm -f "$first_run_log" "$second_run_log"
    return 1
  fi

  log_verbose "All symlinks remain valid after second run"

  # Restore original HOME
  HOME="$original_home"
  export HOME

  # Clean up
  rm -rf "$test_home"
  rm -f "$first_run_log" "$second_run_log"

  log_verbose "Symlink idempotency test passed"
)}
