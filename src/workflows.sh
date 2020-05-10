#!/bin/sh
set -o errexit
set -o nounset

. "$DIR"/src/stages.sh

# do_install
#
# Perform a full install.
do_install()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

  install_packages
  install_symlinks
  install_dotfiles_cli
  install_vscode_extensions
  configure_file_mode_bits
  configure_shell
  configure_fonts
  configure_systemd
}

# do_test
#
# Run tests.
do_test()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

  test_shellcheck
  test_unit
}

# do_uninstall
#
# Perform a full uninstall.
do_uninstall()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules

  uninstall_symlinks
}