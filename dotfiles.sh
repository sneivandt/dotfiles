#!/bin/sh
set -e
# set -u

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
  case "$OPTS" in
    *"$1"*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# is_symlink_installed
#
# Check if a symlink for a environment exists.
#
# Args:
#     $1 - The environment to be checked.
#     $2 - The symlink to be checked.
#
# return:
#     bool - True of the symlink exists.
is_symlink_installed()
{
  if [ "$(readlink -f "$DIR"/env/"$1"/symlinks/"$2")" = "$(readlink -f ~/."$2")" ]
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
  release=$(cat /etc/*-release | grep -xP "ID_LIKE=.*" | cut -d= -f2)
  if [ -z "$release" ]
  then
    release=$(cat /etc/*-release | grep -xP "ID=.*" | cut -d= -f2)
  fi
  case $1 in
    arch)
      if [ "$release" != "arch" ] && [ "$release" != "archlinux" ]
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

# }}}
# Messages ---------------------------------------------------------------- {{{
#
# Functions which write to stdout.

# message_usage
#
# Print usage instructions.
message_usage()
{
  basename "$0"
  echo
  echo "Usage:"
  echo "    $(basename "$0") install   [-s] [-g]"
  echo "    $(basename "$0") uninstall [-s] [-g]"
  echo "    $(basename "$0") -h"
  echo
  echo "Options:"
  echo "    -g  Configure GUI programs"
  echo "    -s  Use sudo"
}

# message_worker
#
# Print a worker starting message.
#
# Args:
#     $1 - The work that is being performed.
message_worker()
{
  echo ":: $1..."
}

# message_error
#
# Print an error message.
#
# Args:
#     $1 - The reason for exiting.
message_error()
{
  echo "error: $1"
}

# }}}
# Assertions -------------------------------------------------------------- {{{
#
# Assertions about the state that exit with an error if they are not meet.

# assert_not_root
#
# Verify that the current user is not root.
assert_not_root()
{
  if [ "$(id -u)" = 0 ]
  then
    message_error "$(basename "$0") can not be run as root."
    exit 1
  fi
}

# }}}
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic. Workers are called by action
# functions in series.

# worker_update_dotfiles
#
# Update dotfiles.
worker_update_dotfiles()
{
  if [ -d "$DIR"/.git ] \
    && is_program_installed "git" \
    && git -C "$DIR" diff-index --quiet HEAD -- \
    && [ "$(git -C "$DIR" remote show origin | sed -n -e "s/.*HEAD branch: //p")" = "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ] \
    && [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
  then
    message_worker "Updating dotfiles"
    git -C "$DIR" pull
  fi
}

# worker_install_git_submodules
#
# Install git submodules.
worker_install_git_submodules()
{
  if [ -d "$DIR"/.git ] && is_program_installed "git"
  then
    modules="$(cat "$DIR"/env/base/submodules.conf)"
    for env in "$DIR"/env/*
    do
      if [ "$(basename "$env")" != "base" ] && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules "env/$(basename "$env")
      fi
    done
    # shellcheck disable=SC2086
    if git -C "$DIR" submodule status $modules | cut -c-1 | grep -q "+\\|-"
    then
      message_worker "Installing git submodules"
      # shellcheck disable=SC2086
      git -C "$DIR" submodule update --init --recursive $modules
    fi
  fi
}

# worker_update_git_submodules
#
# Update git submodules.
worker_update_git_submodules()
{
  if [ -d "$DIR"/.git ] && is_program_installed "git"
  then
    for env in "$DIR"/env/*
    do
      if [ "$(basename "$env")" != "base" ] && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules env/"$(basename "$env")
      fi
    done
    # shellcheck disable=SC2086
    if [ -z "$(git -C "$DIR" submodule status $modules | cut -c1)" ]
    then
      message_worker "Updating git submodules"
      # shellcheck disable=SC2086
      git -C "$DIR" submodule update --init --recursive --remote $modules
    fi
  fi
}

# worker_install_packages
#
# Install packages.
worker_install_packages()
{
  if is_flag_set "s" && is_program_installed "sudo"
  then
    for env in "$DIR"/env/*
    do
      if [ "$(basename "$env")" != "base" ] \
        && ! is_env_ignored "$(basename "$env")" \
        && [ -e "$env"/packages.conf ]
      then
        case $env in
          arch | arch-gui)
            installed=$(pacman -Q | cut -f 1 -d" ")
            ;;
        esac
        while IFS='' read -r package
        do
          if ! echo "$installed" | grep -qw "$package"
          then
            notinstalled="$notinstalled $package"
          fi
        done < "$env"/packages.conf
        if [ -z "$notinstalled" ]
        then
          message_worker "Installing packages"
          case $env in
            arch | "arch-gui")
              # shellcheck disable=SC2086
              sudo pacman -S --quiet --needed $notinstalled
              ;;
          esac
        fi
      fi
    done
  fi
}

# worker_configure_shell
#
# Set the user shell.
worker_configure_shell()
{
  if is_program_installed "zsh" \
    && [ "$SHELL" != "$(zsh -c "command -vp zsh")" ] \
    && [ ! -f /.dockerenv ] \
    && [ "$(passwd --status "$USER" | cut -d" " -f2)" = "P" ]
  then
    message_worker "Configuring user login shell"
    chsh -s "$(zsh -c "command -vp zsh")"
  fi
}

# worker_configure_fonts
#
# Update font cache.
worker_configure_fonts()
{
  if ! is_env_ignored "arch-gui" \
    && is_program_installed "fc-list" \
    && is_program_installed "fc-cache" \
    && [ "$(fc-list : family | grep -f "$DIR"/env/arch-gui/fonts.conf -cx)" != "$(grep -c "" "$DIR"/env/arch-gui/fonts.conf | cut -f1 -d" ")" ]
  then
    message_worker "Updating fontconfig font cache"
    fc-cache
  fi
}

# worker_configure_cron
#
# Configure cron.
worker_configure_cron()
{
  work=false
  if ! is_env_ignored "arch" && is_program_installed "crontab"
  then
    if [ "$(crontab -l 2> /dev/null)" != "$(cat "$DIR"/env/arch/crontab)" ]
    then
      if ! $work
      then
        work=true
        message_worker "Updating crontab"
      fi
      crontab "$DIR"/env/arch/crontab
    fi
    if is_flag_set "s" \
      && is_program_installed "sudo" \
      && [ "$(sudo crontab -l 2> /dev/null)" != "$(cat "$DIR"/env/arch/crontab-root)" ]
    then
      if ! $work
      then
        work=true
        message_worker "Updating crontab"
      fi
      sudo crontab "$DIR"/env/arch/crontab-root
    fi
  fi
}

# worker_install_symlinks
#
# Create symlinks.
worker_install_symlinks()
{
  work=false
  for env in "$DIR"/env/*
  do
    if [ "$(basename "$env")" != "base" ] \
      && ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if ! is_symlink_installed "$(basename "$env")" "$symlink"
        then
          if ! $work
          then
            work=true
            message_worker "Installing symlinks"
          fi
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
}

# worker_chmod
#
# Change file mode bits.
worker_chmod()
{
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" && [ -e "$env"/chmod.conf ]
    then
      while IFS='' read -r line || [ -n "$line" ]
      do
        file="$(echo "$line" | cut -d" " -f1)"
        perm="$(echo "$line" | cut -d" " -f2)"
        if [ -d "$file" ]
        then
          chmod -c -R "$perm" "$file"
        elif [ -e "$file" ]
        then
          chmod -c "$perm" "$file"
        fi
      done < "$env"/chmod.conf
    fi
  done
}

# worker_install_vscode_extensions
#
# Install vscode extensions.
worker_install_vscode_extensions()
{
  for code in code code-insiders
  do
    if ! is_env_ignored "base-gui" && is_program_installed "$code"
    then
      work=false
      extensionsInstalled=$($code --list-extensions)
      while IFS='' read -r extension || [ -n "$extension" ]
      do
        if ! echo "$extensionsInstalled" | grep -qw "$extension"
        then
          if ! $work
          then
            work=true
            message_worker "Installing $code extensions"
          fi
          $code --install-extension "$extension"
        fi
      done < "$DIR/env/base-gui/vscode-extensions.conf"
    fi
  done
}

# worker_install_dotfiles_cli
#
# Add "dotfiles.sh" to $PATH.
worker_install_dotfiles_cli()
{
  if [ "$(readlink -f "$DIR"/dotfiles.sh)" != "$(readlink -f ~/bin/dotfiles)" ]
  then
    message_worker "Installing dotfiles cli"
    mkdir -pv ~/bin
    ln -snvf "$DIR"/dotfiles.sh ~/bin/dotfiles
  fi
}

# worker_uninstall_symlinks
#
# Uninstall symlinks.
worker_uninstall_symlinks()
{
  work=false
  for env in "$DIR"/env/*
  do
    if [ "$(basename "$env")" != "base" ] \
      && ! is_env_ignored "$(basename "$env")" \
      && [ -e "$DIR"/env/"$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if is_symlink_installed "$env" "$symlink"
        then
          if ! $work
          then
            work=true
            message_worker "Removing symlinks"
          fi
          rm -vf ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
}

# }}}
# Actions ----------------------------------------------------------------- {{{
#
# Controllers for the core logic.

# action_install
#
# Perform a full install.
action_install()
{
  worker_update_dotfiles
  worker_install_git_submodules
  worker_update_git_submodules
  worker_install_packages
  worker_configure_cron
  worker_configure_shell
  worker_configure_fonts
  worker_install_symlinks
  worker_install_vscode_extensions
  worker_install_dotfiles_cli
  worker_chmod
}

# action_uninstall
#
# Perform a full uninstall.
action_uninstall()
{
  worker_uninstall_symlinks
}

# }}}
# Main -------------------------------------------------------------------- {{{
#
# The entry point.

# Get absolute path to the dotfiles directory.
DIR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

assert_not_root

case $1 in
  install)
    OPTS=$(getopt -o sg -n "$(basename "$0")" -- "$@") || exit 1
    action_install
    ;;
  uninstall)
    OPTS=$(getopt -o sg -n "$(basename "$0")" -- "$@") || exit 1
    action_uninstall
    ;;
  *)
    OPTS=$(getopt -o h -n "$(basename "$0")" -- "$@") || exit 1
    message_usage
    ;;
esac

message_usage && exit

# }}}
