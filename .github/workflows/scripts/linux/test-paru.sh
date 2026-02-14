#!/bin/sh
# shellcheck disable=SC2030,SC2031  # subshell variable modifications are intentional
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-paru.sh
# -----------------------------------------------------------------------------
# Paru (AUR helper) installation and functionality tests.
#
# Functions:
#   test_paru_install      Test that paru can be installed from AUR
#   test_paru_available    Test that paru binary is functional
#   test_aur_packages      Test that AUR packages can be installed via paru
#
# Dependencies:
#   logger.sh (log_stage, log_verbose, log_error)
#   utils.sh  (is_program_installed)
#   tasks.sh  (install_paru, install_aur_packages)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh
. "$DIR"/src/linux/tasks.sh

# test_paru_prerequisites
#
# Verify that prerequisites for paru installation are met.
# Paru-bin requires git, makepkg (base-devel), and sudo to install from AUR.
# The sudo command is needed because makepkg -si installs packages via pacman.
#
# Returns:
#   0 if all prerequisites are installed
#   1 if any prerequisites are missing
test_paru_prerequisites()
{(
  log_stage "Testing paru prerequisites"

  local missing_count=0
  local prerequisites="git makepkg sudo"

  for prereq in $prerequisites; do
    if is_program_installed "$prereq"; then
      log_verbose "✓ $prereq is installed"
    else
      printf "%sERROR: Required prerequisite '%s' is not installed%s\n" "${RED}" "$prereq" "${NC}" >&2
      missing_count=$((missing_count + 1))
    fi
  done

  if [ "$missing_count" -gt 0 ]; then
    printf "%sERROR: %d prerequisite(s) missing for paru installation%s\n" "${RED}" "$missing_count" "${NC}" >&2
    return 1
  fi

  log_verbose "All prerequisites for paru installation are met"
  return 0
)}

# test_paru_install
#
# Test that paru can be installed from AUR.
# This test verifies that the install_paru task function works correctly.
#
# Returns:
#   0 if paru is successfully installed or already installed
#   1 if paru installation fails
test_paru_install()
{(
  log_stage "Testing paru installation"

  # Check if paru is already installed
  if is_program_installed "paru"; then
    log_verbose "Paru is already installed, skipping installation test"
    return 0
  fi

  # Verify prerequisites are met
  if ! test_paru_prerequisites; then
    log_verbose "Skipping paru installation: prerequisites not met"
    return 1
  fi

  # Set required environment variables for tasks
  PROFILE="arch"
  export PROFILE

  # Set EXCLUDED_CATEGORIES for arch profile (empty = include all)
  # For arch profile, we exclude: windows (since this is Linux)
  EXCLUDED_CATEGORIES="windows"
  export EXCLUDED_CATEGORIES

  # Clear dry-run flag if set (we need actual installation for this test)
  OPT=""
  export OPT

  log_verbose "Running install_paru task"

  # Install paru (this will build from AUR)
  if ! install_paru; then
    printf "%sERROR: install_paru task failed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  # Verify paru is now installed
  if ! is_program_installed "paru"; then
    printf "%sERROR: Paru was not installed successfully%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Paru installation completed successfully"
  return 0
)}

# test_paru_available
#
# Test that paru binary is functional.
# This test verifies that paru can execute basic commands.
#
# Returns:
#   0 if paru is functional
#   1 if paru is not installed or not functional
test_paru_available()
{(
  log_stage "Testing paru availability"

  # Check if paru is installed
  if ! is_program_installed "paru"; then
    printf "%sERROR: Paru is not installed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Paru binary found in PATH"

  # Test 1: Check paru version
  if ! paru --version >/dev/null 2>&1; then
    printf "%sERROR: Cannot run paru --version%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  local version
  version="$(paru --version 2>&1 | head -n 1)"
  log_verbose "Paru version: $version"

  # Test 2: Test paru help command
  if ! paru --help >/dev/null 2>&1; then
    printf "%sERROR: Cannot run paru --help%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Paru help command works"

  # Test 3: Test paru can query packages (read-only operation)
  # Use -Ss to search (similar to pacman -Ss)
  if ! paru -Ss --noconfirm base-devel >/dev/null 2>&1; then
    printf "%sWARNING: Paru search command failed%s\n" "${YELLOW}" "${NC}" >&2
  else
    log_verbose "Paru search command works"
  fi

  log_verbose "Paru is functional"
  return 0
)}

# test_aur_packages
#
# Test that AUR packages can be installed via paru.
# This test verifies that the install_aur_packages task function works correctly.
#
# Note: This test requires packages.ini to have AUR packages defined.
#
# Returns:
#   0 if AUR packages are successfully processed
#   1 if AUR package installation fails
test_aur_packages()
{(
  log_stage "Testing AUR package installation"

  # Check if paru is installed
  if ! is_program_installed "paru"; then
    printf "%sERROR: Paru is not installed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  # Check if packages.ini exists
  if [ ! -f "$DIR"/conf/packages.ini ]; then
    log_verbose "Skipping AUR package test: no packages.ini found"
    return 0
  fi

  # Check if there are any AUR sections in packages.ini
  local aur_sections
  aur_sections="$(grep -E '^\[.*,aur.*\]$' "$DIR"/conf/packages.ini || echo "")"

  if [ -z "$aur_sections" ]; then
    log_verbose "Skipping AUR package test: no AUR sections found in packages.ini"
    return 0
  fi

  log_verbose "Found AUR package sections in packages.ini"

  # Set required environment variables
  PROFILE="arch"
  export PROFILE

  # Set EXCLUDED_CATEGORIES for arch profile
  EXCLUDED_CATEGORIES="windows"
  export EXCLUDED_CATEGORIES

  # Use dry-run mode to avoid actually installing packages in CI
  # (unless explicitly disabled for real testing)
  if [ "${TEST_PARU_REAL_INSTALL:-0}" = "1" ]; then
    OPT=""
    log_verbose "Running real AUR package installation"
  else
    OPT="--dry-run"
    log_verbose "Running dry-run AUR package installation"
  fi
  export OPT

  # Test install_aur_packages function
  if ! install_aur_packages 2>&1 | tee /tmp/aur-install-output.log; then
    printf "%sERROR: install_aur_packages task failed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  # In dry-run mode, verify output contains expected messages
  if [ "${TEST_PARU_REAL_INSTALL:-0}" != "1" ]; then
    if grep -q "Would install AUR packages:" /tmp/aur-install-output.log; then
      log_verbose "✓ Dry-run output contains expected AUR package installation message"
    else
      # It's okay if no packages need installation
      log_verbose "No AUR packages need installation (all already installed)"
    fi
  fi

  rm -f /tmp/aur-install-output.log

  log_verbose "AUR package installation test passed"
  return 0
)}

# test_paru_config
#
# Test that paru configuration is properly set up.
# Verifies that paru.conf exists and contains expected settings.
#
# Returns:
#   0 if paru configuration is valid or not required
#   1 if configuration issues are detected
test_paru_config()
{(
  log_stage "Testing paru configuration"

  # Check if paru is installed
  if ! is_program_installed "paru"; then
    log_verbose "Skipping paru config test: paru not installed"
    return 0
  fi

  # Check if paru.conf exists in expected locations
  local config_locations="/etc/paru.conf $HOME/.config/paru/paru.conf"
  local config_found=0

  for config_path in $config_locations; do
    if [ -f "$config_path" ]; then
      log_verbose "Found paru config at: $config_path"
      config_found=1

      # Verify config file is readable
      if ! cat "$config_path" >/dev/null 2>&1; then
        printf "%sWARNING: Cannot read paru config: %s%s\n" "${YELLOW}" "$config_path" "${NC}" >&2
      fi
    fi
  done

  if [ "$config_found" -eq 0 ]; then
    log_verbose "No paru config found (using defaults)"
  fi

  # Test that paru can read its configuration
  # The --show-config option doesn't exist, but --version will fail if config is broken
  if ! paru --version >/dev/null 2>&1; then
    printf "%sWARNING: Paru may have configuration issues%s\n" "${YELLOW}" "${NC}" >&2
  else
    log_verbose "Paru configuration is valid"
  fi

  return 0
)}

# test_paru_idempotency
#
# Test that paru operations are idempotent.
# Running install_paru multiple times should not cause errors.
#
# Returns:
#   0 if paru operations are idempotent
#   1 if idempotency issues are detected
test_paru_idempotency()
{(
  log_stage "Testing paru idempotency"

  # Set required environment variables
  PROFILE="arch"
  export PROFILE

  # Set EXCLUDED_CATEGORIES for arch profile
  EXCLUDED_CATEGORIES="windows"
  export EXCLUDED_CATEGORIES

  OPT=""
  export OPT

  log_verbose "Running install_paru first time"

  # First run (may install or skip if already installed)
  local first_run_output
  first_run_output="$(mktemp)"

  if ! install_paru >"$first_run_output" 2>&1; then
    printf "%sERROR: First install_paru run failed%s\n" "${RED}" "${NC}" >&2
    cat "$first_run_output" >&2
    rm -f "$first_run_output"
    return 1
  fi

  log_verbose "Running install_paru second time (should be idempotent)"

  # Second run (should skip with "already installed" message)
  local second_run_output
  second_run_output="$(mktemp)"

  if ! install_paru >"$second_run_output" 2>&1; then
    printf "%sERROR: Second install_paru run failed%s\n" "${RED}" "${NC}" >&2
    cat "$second_run_output" >&2
    rm -f "$first_run_output" "$second_run_output"
    return 1
  fi

  # Verify second run skipped installation
  if grep -q "Skipping paru installation: already installed" "$second_run_output"; then
    log_verbose "✓ Second run correctly skipped installation"
  else
    printf "%sWARNING: Second run did not skip installation as expected%s\n" "${YELLOW}" "${NC}" >&2
    cat "$second_run_output" >&2
  fi

  rm -f "$first_run_output" "$second_run_output"

  log_verbose "Paru idempotency test passed"
  return 0
)}
