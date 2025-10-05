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
#   * git submodules (install/update) precede operations relying on their
#     presence (e.g., symlink creation referencing submodule content).
#   * install_* steps proceed before configure_* to guarantee prerequisites.
#
# Idempotency: Underlying tasks short‑circuit when no work is necessary; this
# file intentionally does not add additional guards to keep flow readable.
# -----------------------------------------------------------------------------

# Task primitives (install_packages, configure_systemd, etc.).
. "$DIR"/src/tasks.sh

# do_install
#
# Perform a full install of the selected layers.
# Relies on global $OPT flags parsed by dotfiles.sh:
#   -g include GUI layer (base-gui, arch-gui envs)
#   -p install packages via pacman
#   -s configure user systemd units
#
# Side Effects:
#   * May modify user shell (chsh) if zsh available.
#   * Creates ~/.bin/dotfiles symlink for convenience.
#   * Installs pacman packages, VS Code extensions, PowerShell modules.
do_install()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

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
# Run static analysis / linting without applying configuration changes beyond
# ensuring required submodules are present. Intended for CI / validation.
do_test()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

  test_psscriptanalyzer
  test_shellcheck
}

# do_uninstall
#
# Remove managed symlinks (and only those) for enabled environments. Leaves
# packages, shells, fonts, and submodules untouched to avoid data loss.
do_uninstall()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

  uninstall_symlinks
}
