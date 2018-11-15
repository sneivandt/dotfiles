#!/bin/bash

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
  if [[ $(readlink -f "$DIR"/env/"$1"/symlinks/"$2") == $(readlink -f ~/."$2") ]]
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
  if [[ -z $release ]]
  then
    release=$(cat /etc/*-release | grep -xP 'ID=.*' | cut -d= -f2)
  fi
  case $1 in
    arch)
      if [[ $release != "archlinux" ]]
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
    wsl)
      if ! (is_program_installed "wsl.exe" && [[ $release == "debian" ]])
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
  if [[ -n $(command -v "$1") ]]
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
  echo "Usage: $(basename "$0") <command> [-g | --gui] [-p | --pack] [-r | --root]"
  echo
  echo "These are the available commands:"
  echo
  echo "    help       Show usage instructions"
  echo "    install    Install symlinks, packages and dotfiles CLI"
  echo "    uninstall  Remove symlinks"
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

# assert_user_permissions
#
# Verify that if this script is being run by root, that the command line flags
# "-r" or "--root" are set.
assert_user_permissions()
{
  if (! (is_flag_set "--root" || is_flag_set "-r")) && [ $EUID -eq 0 ]
  then
    message_error "Do not run this script as root. To skip this check pass the command line flag '--root'."
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
  if [ -d "$DIR"/.git ] && is_program_installed "git" && git -C "$DIR" diff-index --quiet HEAD -- && [ "$(git -C "$DIR" remote show origin | sed -n -e 's/.*HEAD branch: //p')" == "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ] && [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
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
    if ! git -C "$DIR" submodule status "${modules[@]}" | rev | cut -d" " -f1 | rev | grep -q "(heads/master)"
    then
      message_worker "Updating git submodules"
      git -C "$DIR" submodule update --init --recursive --remote "${modules[@]}"
    fi
  fi
}

# worker_install_packages
#
# Install system packages.
worker_install_packages()
{
  if (is_flag_set "--pack" || is_flag_set "-p") && is_program_installed "sudo"
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
          wsl)
            installed=$(dpkg-query -f '${binary:Package}\n' -W)
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
              echo "${notinstalled[@]}" | sudo pacman -S --quiet --needed -
              ;;
            wsl)
              sudo apt install --quiet --no-install-recommends --no-install-suggests "${notinstalled[@]}"
              ;;
          esac
        fi
      fi
    done
  fi
}

# worker_configure_shell
#
# Set the user shell except when running in a docker container or WSL.
worker_configure_shell()
{
  if is_program_installed "zsh" && [ "$SHELL" != "$(command -v zsh)" ] && [ ! -f /.dockerenv ] && [ "$(passwd --status "$USER" | cut -d' ' -f2)" == "P" ]
  then
    message_worker "Configuring user login shell"
    chsh -s "$(command -v zsh)"
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
          if [[ $symlink == *"/"* ]]
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
# Put "dotfiles.sh" on the $PATH.
worker_install_dotfiles_cli()
{
  if [[ $(readlink -f "$DIR"/dotfiles.sh) != $(readlink -f ~/bin/dotfiles) ]]
  then
    message_worker "Installing dotfiles cli"
    mkdir -pv ~/bin
    ln -snvf "$DIR"/dotfiles.sh ~/bin/dotfiles
  fi
}

# worker_uninstall_symlinks
#
# Remove all symlinks that are not in ignored environments.
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
  worker_configure_shell
  worker_configure_fonts
  worker_install_symlinks
  worker_chmod
  worker_install_vscode_extensions
  worker_install_dotfiles_cli
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

# Get absolute path to the dotfiles project directory. This value will be
# correct even if this script is executed from a symlink or while your working
# directory is not the root of this project.
DIR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

# Read command line options. While reading this input the 'getopt' call will
# report invalid options that were given.
OPTS=$(getopt -o rgp -l root,gui,pack -n "$(basename "$0")" -- "$@")

# Abort if the root user is running this without permission.
assert_user_permissions

# Iterate through the command line input.
for i in "$@"
do
  case $i in

    # Skip any flags. They should already have been processed when $OPTS was
    # initialized.
    -*)
      ;;

    # Call the action function for any of the valid action keywords. Only the
    # first one that is found will be processed and immediately after this
    # script will exit.
    help)
      action_help
      exit
      ;;
    install)
      action_install
      exit
      ;;
    uninstall)
      action_uninstall
      exit
      ;;

    # If an argument is found that is not valid exit with an error.
    *)
      message_invalid "$i"
      exit 1
      ;;
  esac
done

# If no actions triggered when processing the command line input, print the
# usage instructions and exit with error.
action_help
exit 1

# }}}
