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
is_flag_set()
{
  if [[ " $OPTS " == *\ $1\ * ]]
  then
    echo 1
  else
    echo 0
  fi
}

# does_symlink_exist
#
# Check if a given symlink for a group exists.
#
# Args:
#     $1 - The group to be checked
#     $2 - The symlink to be checked.
#
# return:
#     bool - True of the symlink exists.
does_symlink_exist()
{
  if [[ $(readlink -f "$DIR"/files/"$1"/"$2") == $(readlink -f ~/."$2") ]]
  then
    echo 1
  else
    echo 0
  fi
}

# is_group_ignored
#
# Check if a group is ignored.
#
# Args:
#     $1 - The group to check.
#
# return:
#     bool - True of the group is ignored.
is_group_ignored()
{
  case $1 in
    gui)
      if [[ $(is_flag_set "--gui") == "0" && $(is_flag_set "-g") == "0" ]]
      then
        echo 1
      else
        echo 0
      fi
      ;;
    *)
      echo 0
      ;;
  esac
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
  if [[ -n $(which "$1" 2>/dev/null) ]]
  then
    echo 1
  else
    echo 0
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
  echo "Usage: $(basename "$0") <command> [-g | --gui] [-r | --root]"
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
# Print an exit message.
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
  if [[ $EUID -eq 0 && ($(is_flag_set "--root") == "0" && $(is_flag_set "-r") == "0") ]]
  then
    message_error "Do not run this script as root. To skip this check pass the command line flag '--root'."
    exit 1
  fi
}

# worker_install_git_submodules
#
# Install git submodules.
worker_install_git_submodules()
{
  if [[ -d "$DIR"/.git && $(is_program_installed "git") == "1" ]]
  then
    if git submodule status | cut -c-1 | grep -q "+\\|-"
    then
      message_worker "Installing git submodules"
      git -C "$DIR" submodule update --init --recursive
    fi
  fi
}

# }}}
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic of this script. Workers are called by
# action functions in series.

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

# worker_chmod
#
# Change file mode bits.
worker_chmod()
{
  if [[ -e ~/.ssh/config && "$(stat -c "%a" "$(readlink -f ~/.ssh/config)")" != "600" ]]
  then
    message_worker "Changing file mode bits"
    chmod -c 600 ~/.ssh/config
  fi
}

# worker_install_symlinks
#
# Create symlinks excluding any symlinks that are ignored. Symlinks that are in
# child directories of $HOME will trigger creation of those directories.
worker_install_symlinks()
{
  local act="0"
  local groups
  groups=$(ls "$DIR"/files)
  for group in $groups
  do
    if [[ $(is_group_ignored "$group") == "0" ]]
    then
      local link
      while read -r link
      do
        if [[ $(does_symlink_exist "$group" "$link") == "0" ]]
        then
          if [[ $act == "0" ]]
          then
            act="1"
            message_worker "Installing symlinks"
          fi
          if [[ $link == *"/"* ]]
          then
            mkdir -pv ~/."$(echo "$link" | rev | cut -d/ -f2- | rev)"
          fi
          if [[ -e ~/."$link" ]]
          then
            rm -rvf ~/."$link"
          fi
          ln -snvf "$DIR"/files/"$group"/"$link" ~/."$link"
        fi
      done < "$DIR/files/$group/.symlinks"
    fi
  done
}

# worker_install_vscode_extensions
#
# Install vscode extensions.
worker_install_vscode_extensions()
{
  if [[ $(is_group_ignored "gui") == "0" && $(is_program_installed "code") == "1" ]]
  then
    local act="0"
    local extension
    local extensionsInstalled
    mapfile -t extensionsInstalled < <(code --list-extensions)
    while read -r extension
    do
      if ! echo "${extensionsInstalled[@]}" | grep -qw "$extension"
      then
        if [[ $act == "0" ]]
        then
          act="1"
          message_worker "Installing vscode extensions"
        fi
        code --install-extension "$extension"
      fi
    done < "$DIR/files/gui/vscode/extensions"
  fi
}

# worker_uninstall_symlinks
#
# Remove all symlinks that are not in igned groups.
worker_uninstall_symlinks()
{
  local act="0"
  local groups
  groups=$(ls "$DIR"/files)
  for group in $groups
  do
    if [[ $(is_group_ignored "$group") == "0" ]]
    then
      for file in "$DIR"/files/"$group"/.symlinks
      do
        local link
        while read -r link
        do
          if [[ $(does_symlink_exist "$group" "$link") == "1" ]]
          then
            if [[ $act == "0" ]]
            then
              act="1"
              message_worker "Removing symlinks"
            fi
            rm -vf ~/."$link"
          fi
        done < "$file"
      done
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
  worker_install_git_submodules
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

# Get absolute path to the dotfiles project directory. This value will be
# correct even if this script is executed from a symlink or while your working
# directory is not the root of this project.
DIR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

# Read command line options. While reading this input the 'getopt' call will
# report invalid options that were given.
OPTS=$(getopt -o rg -l root,gui -n "$(basename "$0")" -- "$@")

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
