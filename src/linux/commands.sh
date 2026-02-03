#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# commands.sh
# -----------------------------------------------------------------------------
# High‑level orchestration layer invoked by the CLI front‑end (dotfiles.sh).
# Each public function here sequences lower‑level task primitives from
# tasks.sh. No direct state mutation outside calling those helpers.
#
# Functions:
#   do_install    Perform idempotent full environment provisioning.
#   do_test       Run static analysis / linting checks only.
#   do_uninstall  Remove previously installed symlinks (non‑destructive to
#                 user data besides the managed links themselves).
#
# Ordering Rationale:
#   * update_dotfiles occurs first to ensure latest task definitions.
#   * install_* steps proceed before configure_* to guarantee prerequisites.
#
# Idempotency: Underlying tasks short‑circuit when no work is necessary; this
# file intentionally does not add additional guards to keep flow readable.
#
# Expected Environment Variables:
#   DIR      Repository root directory (exported by dotfiles.sh)
#   PROFILE  Selected profile name (exported by dotfiles.sh)
#   OPT      CLI options string (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# Task primitives (install_packages, configure_systemd, etc.).
# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154
. "$DIR"/src/linux/tasks.sh

# do_install
#
# Perform a full install of the selected profile.
# Relies on global $PROFILE parsed by dotfiles.sh:
#   --profile <name> use predefined profile for sparse checkout
#
# All components defined in the profile (packages, symlinks, units, fonts)
# are automatically installed based on the configuration files.
#
# Side Effects:
#   * Configures git sparse checkout based on profile or flags.
#   * May modify user shell (chsh) if zsh available.
#   * Creates ~/.bin/dotfiles symlink for convenience.
#   * Installs pacman packages, VS Code extensions, PowerShell modules.
do_install()
{
  # PROFILE is exported from dotfiles.sh (uppercase is intentional)
  # shellcheck disable=SC2153,SC2154
  configure_sparse_checkout "$PROFILE"
  update_dotfiles

  install_packages
  install_symlinks
  install_dotfiles_cli
  install_vscode_extensions
  install_powershell_modules

  configure_file_mode_bits
  configure_shell
  configure_fonts
  configure_systemd
}

# do_test
#
# Run static analysis / linting without applying configuration changes.
# Intended for CI / validation.
do_test()
{
  update_dotfiles

  # Source test functions only when needed
  . "$DIR"/test/linux/test-config.sh
  . "$DIR"/test/linux/test-static-analysis.sh
  . "$DIR"/test/linux/test-applications.sh
  . "$DIR"/test/linux/test-idempotency.sh

  # Configuration validation tests
  test_config_validation
  test_symlinks_validation
  test_chmod_validation
  test_ini_syntax
  test_category_consistency
  test_empty_sections
  test_zsh_completion

  # Static analysis tests
  test_psscriptanalyzer
  test_shellcheck

  # Application tests
  test_vim_opens
  test_nvim_opens
  test_nvim_plugins

  # Idempotency tests
  test_idempotency_symlinks
}

# do_uninstall
#
# Remove managed symlinks (and only those) for enabled environments. Leaves
# packages, shells, and fonts untouched to avoid data loss.
do_uninstall()
{
  configure_sparse_checkout "$PROFILE"
  update_dotfiles

  uninstall_symlinks
}
