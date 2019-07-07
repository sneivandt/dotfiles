#!/bin/sh
set -o errexit
set -o nounset

# Helpers ----------------------------------------------------------------- {{{
#
# Helper functions.

# is_flag_set
#
# Check if a flag is set.
#
# Args:
#     $1 - The flag to check.
#
# return:
#     bool - True of the flag is set.
is_flag_set()
{
  case " $opts " in
    *" -$1 "*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# is_symlink_installed
#
# Check if a symlink is installed.
#
# Args:
#     $1 - The environment to be checked.
#     $2 - The symlink to be checked.
#
# return:
#     bool - True of the symlink is installed.
is_symlink_installed()
{
  if [ "$(readlink -f "$dir"/env/"$1"/symlinks/"$2")" = "$(readlink -f ~/."$2")" ]
  then
    return 0
  else
    return 1
  fi
}

# is_program_installed
#
# Check if a program is installed.
#
# Args:
#     $1 - The program to check.
#
# return:
#     bool - True of the program is installed.
is_program_installed()
{
  if [ -n "$(command -vp "$1")" ]
  then
    return 0
  else
    return 1
  fi
}

# is_env_ignored
#
# Check if an environment is ignored.
#
# Args:
#     $1 - The environment to check.
#
# return:
#     bool - True of the environment is ignored.
is_env_ignored()
{
  case $1 in
    arch)
      if cat /etc/*-release | grep -xP "ID=.*|ID_LIKE=.*" | cut -d= -f2 | grep -qvxP "arch|archlinux"
      then
        return 0
      fi
      ;;
    arch-gui)
      if is_env_ignored "base-gui" || is_env_ignored "arch"
      then
        return 0
      fi
      ;;
    base-gui)
      if ! is_flag_set "g"
      then
        return 0
      fi
      ;;
  esac
  return 1
}

# }}}
# Messages ---------------------------------------------------------------- {{{
#
# Functions that write to stdout.

# message_error
#
# Print an error message and quit.
#
# Args:
#     $1 - The reason for exiting.
message_error()
{
  echo "ERROR: $1"
  exit 1
}

# message_usage
#
# Print usage information.
message_usage()
{
  echo "Usage: $(basename "$0") <command> [<options>]"
  echo
  echo "Commands:"
  echo
  echo "  -I, --install    : Install"
  echo "  -U, --uninstall  : Uninstall"
  echo "  -h, --help       : Display usage"
  echo
  echo "Options:"
  echo
  echo "  -g               : Configure GUI programs"
  echo "  -s               : Use sudo"
  echo
  echo "Examples:"
  echo
  echo "  $(basename "$0") --install      # Install"
  echo "  $(basename "$0") --uninstall    # Uninstall"
  exit
}

# message_worker
#
# Print a message if a worker did work.
#
# Args:
#     $1 - The message.
message_worker()
{
  if [ "${_work-unset}" = "unset" ] || ! $_work
  then
    _work=true
    echo ":: $1..."
  fi
}

# }}}
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic.

# configure_cron
#
# Configure cron.
configure_cron()
{(
  if ! is_env_ignored "arch" && is_program_installed "crontab"
  then
    if [ "$(crontab -l 2> /dev/null)" != "$(cat "$dir"/env/arch/crontab)" ]
    then
      message_worker "Updating crontab"
      crontab "$dir"/env/arch/crontab
    fi
    if is_flag_set "s" \
      && is_program_installed "sudo" \
      && [ "$(sudo crontab -l 2> /dev/null)" != "$(cat "$dir"/env/arch/crontab-root)" ]
    then
      message_worker "Updating crontab"
      sudo crontab "$dir"/env/arch/crontab-root
    fi
  fi
)}

# configure_file_mode_bits
#
# Configure file mode bits.
configure_file_mode_bits()
{(
  for env in "$dir"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" && [ -e "$env"/chmod.conf ]
    then
      while IFS='' read -r line || [ -n "$line" ]
      do
        chmod -c -R "$(echo "$line" | cut -d " " -f1)" ~/."$(echo "$line" | cut -d " " -f2)"
      done < "$env"/chmod.conf
    fi
  done
)}

# configure_fonts
#
# Configure fonts.
configure_fonts()
{(
  if ! is_env_ignored "arch-gui" \
    && is_program_installed "fc-list" \
    && is_program_installed "fc-cache" \
    && [ "$(fc-list : family | grep -f "$dir"/env/arch-gui/fonts.conf -cx)" != "$(grep -c "" "$dir"/env/arch-gui/fonts.conf | cut -f1 -d" ")" ]
  then
    message_worker "Updating fonts"
    fc-cache
  fi
)}

# configure_shell
#
# Set the user shell.
configure_shell()
{(
  if is_program_installed "zsh" \
    && [ "$SHELL" != "$(zsh -c "command -vp zsh")" ] \
    && [ ! -f /.dockerenv ] \
    && [ "$(passwd --status "$USER" | cut -d" " -f2)" = "P" ]
  then
    message_worker "Configuring user shell"
    chsh -s "$(zsh -c "command -vp zsh")"
  fi
)}

# install_dotfiles_cli
#
# Install dotfiles cli.
install_dotfiles_cli()
{(
  if [ "$(readlink -f "$dir"/dotfiles.sh)" != "$(readlink -f ~/bin/dotfiles)" ]
  then
    message_worker "Installing dotfiles cli"
    mkdir -pv ~/bin
    ln -snvf "$dir"/dotfiles.sh ~/bin/dotfiles
  fi
)}

# install_git_submodules
#
# Install git submodules.
install_git_submodules()
{(
  if [ -d "$dir"/.git ] && is_program_installed "git"
  then
    modules="$(cat "$dir"/env/base/submodules.conf)"
    for env in "$dir"/env/*
    do
      if [ "$(basename "$env")" != "base" ] && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules "env/$(basename "$env")
      fi
    done
    if eval "git -C $dir submodule status $modules" | cut -c-1 | grep -q "+\\|-"
    then
      message_worker "Installing git submodules"
      eval "git -C $dir submodule update --init --recursive $modules"
    fi
  fi
)}

# install_packages
#
# Install packages.
install_packages()
{(
  if is_flag_set "s" && is_program_installed "sudo"
  then
    packages=""
    for env in "$dir"/env/*
    do
      if ! is_env_ignored "$(basename "$env")" \
        && [ -e "$env"/packages.conf ]
      then
        while IFS='' read -r package
        do
          if ! pacman -Qq "$package" >/dev/null 2>&1
          then
            packages="$packages $package"
          fi
        done < "$env"/packages.conf
      fi
    done
    if [ -n "$packages" ]
    then
      message_worker "Installing packages"
      eval "sudo pacman -S --quiet --needed $packages"
    fi
  fi
)}

# install_symlinks
#
# Install symlinks.
install_symlinks()
{(
  for env in "$dir"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if ! is_symlink_installed "$(basename "$env")" "$symlink"
        then
          message_worker "Installing symlinks"
          case "$symlink" in
            *"/"*) mkdir -pv ~/."$(echo "$symlink" | rev | cut -d/ -f2- | rev)"
          esac
          if [ -e ~/."$symlink" ]
          then
            rm -rvf ~/."$symlink"
          fi
          ln -snvf "$env"/symlinks/"$symlink" ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
)}

# install_vscode_extensions
#
# Install vscode extensions.
install_vscode_extensions()
{(
  for code in code code-insiders
  do
    if ! is_env_ignored "base-gui" && is_program_installed "$code"
    then
      extensions=$($code --list-extensions)
      while IFS='' read -r extension || [ -n "$extension" ]
      do
        if ! echo "$extensions" | grep -qw "$extension"
        then
          message_worker "Installing $code extensions"
          $code --install-extension "$extension"
        fi
      done < "$dir/env/base-gui/vscode-extensions.conf"
    fi
  done
)}

# uninstall_symlinks
#
# Uninstall symlinks.
uninstall_symlinks()
{(
  for env in "$dir"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$dir"/env/"$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if is_symlink_installed "$env" "$symlink"
        then
          message_worker "Uninstalling symlinks"
          rm -vf ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
)}

# update_dotfiles
#
# Update dotfiles.
update_dotfiles()
{(
  if [ -d "$dir"/.git ] \
    && is_program_installed "git" \
    && git -C "$dir" diff-index --quiet HEAD -- \
    && [ "$(git -C "$dir" remote show origin | sed -n -e "s/.*HEAD branch: //p")" = "$(git -C "$dir" rev-parse --abbrev-ref HEAD)" ] \
    && [ "$(git -C "$dir" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$dir" log --format=format:%H -n 1 HEAD)" ]
  then
    message_worker "Updating dotfiles"
    git -C "$dir" pull
  fi
)}

# update_git_submodules
#
# Update git submodules.
update_git_submodules()
{(
  if [ -d "$dir"/.git ] && is_program_installed "git"
  then
    modules=""
    for env in "$dir"/env/*
    do
      if [ "$(basename "$env")" != "base" ] && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules env/"$(basename "$env")
      fi
    done
    # shellcheck disable=SC2086
    if [ -z "$(git -C "$dir" submodule status $modules | cut -c1)" ]
    then
      message_worker "Updating git submodules"
      # shellcheck disable=SC2086
      git -C "$dir" submodule update --init --recursive --remote $modules
    fi
  fi
)}

# }}}
# Commands ---------------------------------------------------------------- {{{
#
# Functions that implement the core logic.

# install
#
# Perform a full install.
install()
{
  update_dotfiles
  install_git_submodules
  update_git_submodules
  install_packages
  install_symlinks
  install_dotfiles_cli
  install_vscode_extensions
  configure_cron
  configure_file_mode_bits
  configure_fonts
  configure_shell
}

# uninstall
#
# Perform a full uninstall.
uninstall()
{
  uninstall_symlinks
}

# }}}
# Main -------------------------------------------------------------------- {{{
#
# Main.

if [ "$(id -u)" = 0 ]
then
  message_error "$(basename "$0") can not be run as root."
fi

readonly dir=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

case ${1:-} in
  -I* | --install)
    opts=$(getopt -o Isg -l install -n "$(basename "$0")" -- "$@") || exit 1
    install
    ;;
  -U* | --uninstall)
    opts=$(getopt -o Usg -l uninstall -n "$(basename "$0")" -- "$@") || exit 1
    uninstall
    ;;
  -h | --help)
    opts=$(getopt -o h -l help -n "$(basename "$0")" -- "$@") || exit 1
    message_usage
    ;;
  *)
    opts=$(getopt -o -l -n "$(basename "$0")" -- "$@") || exit 1
    message_usage
    ;;
esac

# }}}
