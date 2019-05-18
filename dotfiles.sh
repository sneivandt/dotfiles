#!/usr/bin/env bash

# Helpers ----------------------------------------------------------------- {{{
#
# Helper functions.

# is_flag_set
#
# Check if a command line flag is set.
#
# Args:
#     $1 - The flag to check.
#
# return:
#     bool - True of the flag is set.
is_flag_set()
{
  if [[ " $OPTS " == *\ $1\ * ]]
  then
    return 0
  else
    return 1
  fi
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
  release=$(cat /etc/*-release | grep -xP 'ID_LIKE=.*' | cut -d= -f2)
  if [ -z "$release" ]
  then
    release=$(cat /etc/*-release | grep -xP 'ID=.*' | cut -d= -f2)
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
      if ! (is_flag_set "--gui" || is_flag_set "-g")
      then
        return 0
      fi
      ;;
  esac
  return 1
}

# is_program_installed
#
# Check if a program is installed on your $PATH.
#
# Args:
#     $1 - The program to be checked.
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
  echo "    $(basename "$0") help"
  echo "    $(basename "$0") install   [-s|--sudo] [-g|--gui]"
  echo "    $(basename "$0") uninstall [-s|--sudo] [-g|--gui]"
  echo
  echo "Options:"
  echo "    -g --gui   Include GUI config"
  echo "    -s --sudo  Include privileged config"
}

# message_worker
#
# Print a worker starting message.
#
# Args:
#     $1 - The work that is being performed.
message_worker()
{
  echo -e ":: $1..."
}

# message_error
#
# Print an error message.
#
# Args:
#     $1 - The reason for exiting.
message_error()
{
  echo -e "error: $1"
}

# message_invalid
#
# Print an invalid command message.
#
# Args:
#     $1 - The invalid command.
message_invalid()
{
  echo "$(basename "$0"): '$1' is not a valid command. See '$(basename "$0") help'."
}

# }}}
# Assertions -------------------------------------------------------------- {{{
#
# Assertions about the state that exit with an error if they are not meet.

# assert_not_root
#
# Verify that if this script is not being run as root.
assert_not_root()
{
  if [ "$EUID" -eq 0 ]
  then
    message_error "$(basename "$0") can not be run as root."
    exit 1
  fi
}

# }}}
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic of this script. Workers are called by
# action functions in series.

# worker_update_dotfiles
#
# Update dotfiles.
worker_update_dotfiles()
{
  if [ -d "$DIR"/.git ] && is_program_installed "git" && git -C "$DIR" diff-index --quiet HEAD -- && [ "$(git -C "$DIR" remote show origin | sed -n -e 's/.*HEAD branch: //p')" = "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ] && [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
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
    local modules
    readarray modules < "$DIR"/env/base/submodules.conf
    local envs
    mapfile -t envs <<< "$(ls -1 "$DIR"/env -I base)"
    for env in "${envs[@]}"
    do
      if (! is_env_ignored "$env")
      then
        modules+=("env/$env")
      fi
    done
    if git -C "$DIR" submodule status "${modules[@]}" | cut -c-1 | grep -q "+\\|-"
    then
      message_worker "Installing git submodules"
      git -C "$DIR" submodule update --init --recursive "${modules[@]}"
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
    local modules
    local envs
    mapfile -t envs <<< "$(ls -1 "$DIR"/env -I base)"
    for env in "${envs[@]}"
    do
      if (! is_env_ignored "$env")
      then
        modules+=("env/$env")
      fi
    done
    if [ -z "$(git -C "$DIR" submodule status "${modules[@]}" | cut -c1)" ]
    then
      message_worker "Updating git submodules"
      git -C "$DIR" submodule update --init --recursive --remote "${modules[@]}"
    fi
  fi
}

# worker_install_packages
#
# Install packages.
worker_install_packages()
{
  if (is_flag_set "--sudo" || is_flag_set "-s") && is_program_installed "sudo"
  then
    local envs
    mapfile -t envs <<< "$(ls -1 "$DIR"/env)"
    for env in "${envs[@]}"
    do
      if (! is_env_ignored "$env") && [ -e "$DIR"/env/"$env"/packages.conf ]
      then
        readarray packages < "$DIR"/env/"$env"/packages.conf
        case $env in
          arch | "arch-gui")
            installed=$(pacman -Q | cut -f 1 -d ' ')
            ;;
        esac
        notinstalled=()
        for package in "${packages[@]}"
        do
          if ! echo "${installed[@]}" | grep -qw "${package%$'\n'}"
          then
            notinstalled+=("${package%$'\n'}")
          fi
        done
        if [ ${#notinstalled[@]} -ne 0 ]
        then
          message_worker "Installing packages"
          case $env in
            arch | "arch-gui")
              sudo pacman -S --quiet --needed "${notinstalled[@]}"
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
  if is_program_installed "zsh" && [ "$SHELL" != "$(command -vp zsh)" ] && [ ! -f /.dockerenv ] && [ "$(passwd --status "$USER" | cut -d' ' -f2)" = "P" ]
  then
    message_worker "Configuring user login shell"
    chsh -s "$(command -vp zsh)"
  fi
}

# worker_configure_fonts
#
# Update font cache if fonts are not currently cached.
worker_configure_fonts()
{
  if (! is_env_ignored "arch-gui") && is_program_installed "fc-list" && is_program_installed "fc-cache" && [ "$(fc-list : family | grep -f "$DIR"/env/arch-gui/fonts.conf -cx)" != "$(grep -c '' "$DIR"/env/arch-gui/fonts.conf | cut -f1 -d ' ')" ]
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
  local work=false
  if (! is_env_ignored "arch") && is_program_installed "crontab"
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
    if (is_flag_set "--sudo" || is_flag_set "-s") && is_program_installed "sudo" && [ "$(sudo crontab -l 2> /dev/null)" != "$(cat "$DIR"/env/arch/crontab-root)" ]
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
# Create symlinks excluding any symlinks that are ignored. Symlinks that are in
# child directories of $HOME will trigger creation of those directories.
worker_install_symlinks()
{
  local work=false
  local envs
  mapfile -t envs <<< "$(ls -1 "$DIR"/env)"
  for env in "${envs[@]}"
  do
    if (! is_env_ignored "$env") && [ -e "$DIR"/env/"$env"/symlinks.conf ]
    then
      local symlink
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if ! is_symlink_installed "$env" "$symlink"
        then
          if ! $work
          then
            work=true
            message_worker "Installing symlinks"
          fi
          if [[ "$symlink" == *"/"* ]]
          then
            mkdir -pv ~/."$(echo "$symlink" | rev | cut -d/ -f2- | rev)"
          fi
          if [ -e ~/."$symlink" ]
          then
            rm -rvf ~/."$symlink"
          fi
          ln -snvf "$DIR"/env/"$env"/symlinks/"$symlink" ~/."$symlink"
        fi
      done < "$DIR"/env/"$env"/symlinks.conf
    fi
  done
}

# worker_chmod
#
# Change file mode bits.
worker_chmod()
{
  local envs
  mapfile -t envs <<< "$(ls -1 "$DIR"/env)"
  for env in "${envs[@]}"
  do
    if ! is_env_ignored "$env" && [ -e "$DIR"/env/"$env"/chmod.conf ]
    then
      local line
      while IFS='' read -r line || [ -n "$line" ]
      do
        read -r -a elements <<< "$line"
        local file
        local permissions
        file=~/."${elements[0]}"
        permissions="${elements[1]}"
        if [ -d "$file" ]
        then
          chmod -c -R "$permissions" "$file"
        elif [ -e "$file" ]
        then
          chmod -c "$permissions" "$file"
        fi
      done < "$DIR"/env/"$env"/chmod.conf
    fi
  done
}

# worker_install_vscode_extensions
#
# Install vscode extensions.
worker_install_vscode_extensions()
{
  codes=("code" "code-insiders")
  for code in "${codes[@]}"
  do
    if (! is_env_ignored "base-gui") && is_program_installed "$code"
    then
      local work=false
      local extension
      local extensionsInstalled
      mapfile -t extensionsInstalled < <($code --list-extensions)
      while IFS='' read -r extension || [ -n "$extension" ]
      do
        if ! echo "${extensionsInstalled[@]}" | grep -qw "$extension"
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
  local work=false
  local envs
  mapfile -t envs <<< "$(ls -1 "$DIR"/env)"
  for env in "${envs[@]}"
  do
    if ! is_env_ignored "$env" && [ -e "$DIR"/env/"$env"/symlinks.conf ]
    then
      local symlink
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
      done < "$DIR"/env/"$env"/symlinks.conf
    fi
  done
}

# }}}
# Actions ----------------------------------------------------------------- {{{
#
# Functions that control the execution of the core logic.

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

# action_help
#
# Print usage instructions.
action_help()
{
  message_usage
}

# }}}
# Main -------------------------------------------------------------------- {{{
#
# The entry point to this script.

# Get absolute path to the dotfiles project directory.
DIR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

assert_not_root

case $1 in
  "" | -* | help)
    OPTS=$(getopt -o s -l sudo -n "$(basename "$0")" -- "$@") || exit 1
    action_help
    ;;
  install)
    OPTS=$(getopt -o sg -l sudo,gui -n "$(basename "$0")" -- "$@") || exit 1
    action_install
    ;;
  uninstall)
    OPTS=$(getopt -o sg -l sudo,gui -n "$(basename "$0")" -- "$@") || exit 1
    action_uninstall
    ;;
  *)
    message_invalid "$1"
    exit 1
    ;;
esac

# }}}
