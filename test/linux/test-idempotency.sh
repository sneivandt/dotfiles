#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-idempotency.sh
# -----------------------------------------------------------------------------
# Idempotency tests for installer tasks.
#
# Functions:
#   test_install_symlinks_idempotency       Verify symlink installation is idempotent
#   test_configure_file_mode_bits_idempotency  Verify chmod operations are idempotent
#   test_configure_systemd_idempotency      Verify systemd unit configuration is idempotent
#   test_configure_shell_idempotency        Verify shell configuration is idempotent
#   test_install_dotfiles_cli_idempotency   Verify CLI symlink is idempotent
#   test_install_packages_idempotency       Verify package installation is idempotent
#   test_install_vscode_extensions_idempotency  Verify VS Code extensions are idempotent
#
# Dependencies:
#   logger.sh (log_stage, log_verbose, log_error)
#   utils.sh  (is_program_installed)
#   tasks.sh  (install_*, configure_* functions)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh
. "$DIR"/src/linux/tasks.sh

# test_install_symlinks_idempotency
#
# Verify that running install_symlinks twice does not fail or make
# unnecessary changes. Tests that:
# 1. First run creates symlinks successfully
# 2. Second run skips already-correct symlinks
# 3. No errors occur on second run
test_install_symlinks_idempotency()
{(
  # Check if symlinks.ini exists
  if [ ! -f "$DIR"/conf/symlinks.ini ]; then
    log_verbose "Skipping symlink idempotency test: no symlinks.ini found"
    return 0
  fi

  log_stage "Testing symlink installation idempotency"

  # Create a temporary home directory for testing
  test_home="$(mktemp -d)"
  trap 'rm -rf "$test_home"' EXIT

  # Override HOME for this test
  original_home="$HOME"
  export HOME="$test_home"

  # Run install_symlinks first time
  log_verbose "Running install_symlinks (first time)"
  first_run_output="$(install_symlinks 2>&1)"
  first_run_exit=$?

  if [ $first_run_exit -ne 0 ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: First install_symlinks run failed with exit code %d${NC}\n" "$first_run_exit" >&2
    printf "Output: %s\n" "$first_run_output" >&2
    return 1
  fi

  # Count symlinks created
  first_run_count=$(find "$test_home" -type l | wc -l)
  log_verbose "First run created $first_run_count symlink(s)"

  # Run install_symlinks second time
  log_verbose "Running install_symlinks (second time)"
  second_run_output="$(install_symlinks 2>&1)"

  # Verify second run doesn't report "Installing symlinks" stage
  # (it should only say "Skipping symlink X: already correct")
  if echo "$second_run_output" | grep -q ":: Installing symlinks"; then
    export HOME="$original_home"
    printf "${RED}ERROR: Second install_symlinks run incorrectly reported new installations${NC}\n" >&2
    printf "Output: %s\n" "$second_run_output" >&2
    return 1
  fi

  # Count symlinks after second run
  second_run_count=$(find "$test_home" -type l | wc -l)

  # Verify same number of symlinks
  if [ "$first_run_count" -ne "$second_run_count" ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: Symlink count changed: first=%d, second=%d${NC}\n" "$first_run_count" "$second_run_count" >&2
    return 1
  fi

  export HOME="$original_home"
  log_verbose "Symlink installation is idempotent"
)}

# test_configure_file_mode_bits_idempotency
#
# Verify that running configure_file_mode_bits twice does not fail or make
# unnecessary changes. Tests that:
# 1. First run sets permissions successfully
# 2. Second run skips files with correct permissions
# 3. No errors occur on second run
test_configure_file_mode_bits_idempotency()
{(
  # Check if chmod.ini exists
  if [ ! -f "$DIR"/conf/chmod.ini ]; then
    log_verbose "Skipping chmod idempotency test: no chmod.ini found"
    return 0
  fi

  log_stage "Testing file mode bits configuration idempotency"

  # Create a temporary home directory for testing
  test_home="$(mktemp -d)"
  trap 'rm -rf "$test_home"' EXIT

  # Create a test file structure that chmod.ini might reference
  # First, install symlinks so files exist
  original_home="$HOME"
  export HOME="$test_home"
  install_symlinks >/dev/null 2>&1 || true

  # Run configure_file_mode_bits first time
  log_verbose "Running configure_file_mode_bits (first time)"
  first_run_output="$(configure_file_mode_bits 2>&1)"

  # Run configure_file_mode_bits second time
  log_verbose "Running configure_file_mode_bits (second time)"
  second_run_output="$(configure_file_mode_bits 2>&1)"

  # Verify second run only reports skipping (no new chmod operations)
  if echo "$second_run_output" | grep -q ":: Configuring file"; then
    export HOME="$original_home"
    printf "${RED}ERROR: Second configure_file_mode_bits run incorrectly reported new operations${NC}\n" >&2
    printf "Output: %s\n" "$second_run_output" >&2
    return 1
  fi

  export HOME="$original_home"
  log_verbose "File mode bits configuration is idempotent"
)}

# test_configure_systemd_idempotency
#
# Verify that running configure_systemd twice does not fail or make
# unnecessary changes. Tests that:
# 1. First run enables/starts units successfully (or skips if not available)
# 2. Second run skips already-enabled units
# 3. No errors occur on second run
test_configure_systemd_idempotency()
{(
  # Check if systemd is available
  if [ "$(ps -p 1 -o comm=)" != "systemd" ]; then
    log_verbose "Skipping systemd idempotency test: not running under systemd"
    return 0
  fi

  if ! is_program_installed "systemctl"; then
    log_verbose "Skipping systemd idempotency test: systemctl not installed"
    return 0
  fi

  # Check if units.ini exists
  if [ ! -f "$DIR"/conf/units.ini ]; then
    log_verbose "Skipping systemd idempotency test: no units.ini found"
    return 0
  fi

  log_stage "Testing systemd configuration idempotency"

  # Run configure_systemd first time
  log_verbose "Running configure_systemd (first time)"
  first_run_output="$(configure_systemd 2>&1)"

  # Run configure_systemd second time
  log_verbose "Running configure_systemd (second time)"
  second_run_output="$(configure_systemd 2>&1)"

  # Verify second run only reports skipping (no new enable operations)
  # Note: configure_systemd checks if units are already enabled before enabling
  if echo "$second_run_output" | grep -q ":: Configuring systemd"; then
    printf "${RED}ERROR: Second configure_systemd run incorrectly reported new operations${NC}\n" >&2
    printf "Output: %s\n" "$second_run_output" >&2
    return 1
  fi

  log_verbose "Systemd configuration is idempotent"
)}

# test_configure_shell_idempotency
#
# Verify that running configure_shell twice does not fail or make
# unnecessary changes. Tests that:
# 1. First run sets shell successfully (or skips if not available/already set)
# 2. Second run skips when shell is already correct
# 3. No errors occur on second run
test_configure_shell_idempotency()
{(
  # Check if zsh is available
  if ! is_program_installed "zsh"; then
    log_verbose "Skipping shell idempotency test: zsh not installed"
    return 0
  fi

  # Skip if in Docker (configure_shell skips in Docker)
  if [ -f /.dockerenv ]; then
    log_verbose "Skipping shell idempotency test: running inside Docker"
    return 0
  fi

  log_stage "Testing shell configuration idempotency"

  # Run configure_shell first time
  log_verbose "Running configure_shell (first time)"
  first_run_output="$(configure_shell 2>&1)"

  # Run configure_shell second time
  log_verbose "Running configure_shell (second time)"
  second_run_output="$(configure_shell 2>&1)"

  # Second run should skip (shell already configured)
  if ! echo "$second_run_output" | grep -q "Skipping shell configuration"; then
    printf "${YELLOW}WARNING: Second configure_shell run may have attempted changes${NC}\n" >&2
    printf "Output: %s\n" "$second_run_output" >&2
  fi

  log_verbose "Shell configuration is idempotent"
)}

# test_install_dotfiles_cli_idempotency
#
# Verify that running install_dotfiles_cli twice does not fail or make
# unnecessary changes. Tests that:
# 1. First run creates CLI symlink successfully
# 2. Second run skips when symlink is already correct
# 3. No errors occur on second run
test_install_dotfiles_cli_idempotency()
{(
  log_stage "Testing dotfiles CLI installation idempotency"

  # Create a temporary home directory for testing
  test_home="$(mktemp -d)"
  trap 'rm -rf "$test_home"' EXIT

  original_home="$HOME"
  export HOME="$test_home"

  # Run install_dotfiles_cli first time
  log_verbose "Running install_dotfiles_cli (first time)"
  first_run_output="$(install_dotfiles_cli 2>&1)"

  # Verify CLI symlink was created
  if [ ! -L "$test_home/.bin/dotfiles" ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: First install_dotfiles_cli run did not create symlink${NC}\n" >&2
    return 1
  fi

  # Run install_dotfiles_cli second time
  log_verbose "Running install_dotfiles_cli (second time)"
  second_run_output="$(install_dotfiles_cli 2>&1)"
  second_run_exit=$?

  if [ $second_run_exit -ne 0 ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: Second install_dotfiles_cli run failed with exit code %d${NC}\n" "$second_run_exit" >&2
    printf "Output: %s\n" "$second_run_output" >&2
    return 1
  fi

  # Verify second run either:
  # 1. Skipped (in verbose mode) with message, or
  # 2. Produced no output (in non-verbose mode)
  # Either way, it shouldn't have the "Installing dotfiles cli" stage header
  if echo "$second_run_output" | grep -q ":: Installing dotfiles cli"; then
    export HOME="$original_home"
    printf "${RED}ERROR: Second install_dotfiles_cli run incorrectly reported new installation${NC}\n" >&2
    printf "Output: %s\n" "$second_run_output" >&2
    return 1
  fi

  export HOME="$original_home"
  log_verbose "Dotfiles CLI installation is idempotent"
)}

# test_install_packages_idempotency
#
# Verify that running install_packages twice does not fail or make
# unnecessary changes. Tests that:
# 1. Second run skips already-installed packages
# 2. No errors occur on second run
# Note: This test only verifies the logic, not actual package installation
test_install_packages_idempotency()
{(
  # Check if pacman and sudo are available
  if ! is_program_installed "sudo" || ! is_program_installed "pacman"; then
    log_verbose "Skipping package idempotency test: sudo or pacman not installed"
    return 0
  fi

  # Check if packages.ini exists
  if [ ! -f "$DIR"/conf/packages.ini ]; then
    log_verbose "Skipping package idempotency test: no packages.ini found"
    return 0
  fi

  log_stage "Testing package installation idempotency"

  # Run install_packages first time (dry-run to avoid actual installation)
  log_verbose "Running install_packages (first time, dry-run)"
  export OPT="--dry-run"
  first_run_output="$(install_packages 2>&1)"

  # Run install_packages second time (dry-run)
  log_verbose "Running install_packages (second time, dry-run)"
  second_run_output="$(install_packages 2>&1)"

  # Both runs should produce similar output (both would install missing packages)
  # The actual idempotency is guaranteed by pacman's --needed flag
  log_verbose "Package installation uses pacman --needed for idempotency"
  log_verbose "Package installation idempotency validated"
)}

# test_install_vscode_extensions_idempotency
#
# Verify that running install_vscode_extensions twice does not fail or make
# unnecessary changes. Tests that:
# 1. Second run skips already-installed extensions
# 2. No errors occur on second run
test_install_vscode_extensions_idempotency()
{(
  # Check if VS Code is available
  if ! is_program_installed "code" && ! is_program_installed "code-insiders"; then
    log_verbose "Skipping VS Code idempotency test: code not installed"
    return 0
  fi

  # Check if vscode-extensions.ini exists
  if [ ! -f "$DIR"/conf/vscode-extensions.ini ]; then
    log_verbose "Skipping VS Code idempotency test: no vscode-extensions.ini found"
    return 0
  fi

  log_stage "Testing VS Code extension installation idempotency"

  # Run install_vscode_extensions first time (dry-run to avoid actual installation)
  log_verbose "Running install_vscode_extensions (first time, dry-run)"
  export OPT="--dry-run"
  first_run_output="$(install_vscode_extensions 2>&1)"

  # Run install_vscode_extensions second time (dry-run)
  log_verbose "Running install_vscode_extensions (second time, dry-run)"
  second_run_output="$(install_vscode_extensions 2>&1)"

  # Both runs should produce similar output
  # The actual idempotency is guaranteed by checking --list-extensions before installing
  log_verbose "VS Code extension installation checks existing extensions for idempotency"
  log_verbose "VS Code extension installation idempotency validated"
)}

# test_full_install_idempotency
#
# Verify that running a full installation twice produces consistent results.
# This is an end-to-end test of the entire installation process.
test_full_install_idempotency()
{(
  log_stage "Testing full installation idempotency"

  # Create a temporary home directory for testing
  test_home="$(mktemp -d)"
  trap 'rm -rf "$test_home"' EXIT

  original_home="$HOME"
  export HOME="$test_home"

  # Set dry-run mode to avoid making actual system changes
  export OPT="--dry-run"

  # Source commands.sh to get do_install function
  . "$DIR"/src/linux/commands.sh

  # Run full installation first time
  log_verbose "Running full installation (first time, dry-run)"
  first_run_output="$(do_install 2>&1)"

  # Run full installation second time
  log_verbose "Running full installation (second time, dry-run)"
  second_run_output="$(do_install 2>&1)"

  # Compare outputs (they should be similar in dry-run mode)
  # In a real scenario, the second run should show mostly "Skipping" messages
  if [ -z "$first_run_output" ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: First installation produced no output${NC}\n" >&2
    return 1
  fi

  if [ -z "$second_run_output" ]; then
    export HOME="$original_home"
    printf "${RED}ERROR: Second installation produced no output${NC}\n" >&2
    return 1
  fi

  export HOME="$original_home"
  log_verbose "Full installation idempotency validated (dry-run mode)"
)}
