#!/bin/bash

# Globals ----------------------------------------------------------------- {{{
#
# Constant values which exist in the global scope of this script.

# Get absolute path to the dotfiles project directory. This value will be
# correct even if this script is executed from a symlink or while your working
# directory is not the root of this project.
DIR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)

# Read command line options. While reading this input the 'getopt' call will
# report invalid options that were given.
OPTS=$(getopt -o rg -l root,gui -n "$(basename "$0")" -- "$@")

# }}}
# Helpers ----------------------------------------------------------------- {{{
#
# Helper functions.

# trigger_action
#
# Trigger an action based on a command line argument.
#
# Args:
#     $1 - The command line argument to map to an action.
trigger_action()
{
  case $1 in
    help)
      action_usage
      ;;
    *)
      eval "action_$1"
      ;;
  esac
}

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
    echo 0
  else
    echo 1
  fi
}

# does_symlink_exist
#
# Check if a given symlink exists in $HOME.
#
# Args:
#     $1 - The symlink to be checked.
#
# return:
#     bool - True of the symlink exists.
does_symlink_exist()
{
  if [[ $(readlink -f "$DIR"/files/"$1") == $(readlink -f ~/."$1") ]]
  then
    echo 0
  else
    echo 1
  fi
}

# is_file_ignored
#
# Check if a file is ignored. Files listed in ".symlinks" with the "g" option
# will be ignored unless the flags "-g" or "--gui" are set. Ignored files will
# also be listed in '.symlinksignore'.
#
# Args:
#     $1 - The file to check.
#
# return:
#     bool - True of the file is ignored.
is_file_ignored()
{
  if grep -qxi "$1" "$DIR"/.symlinksignore 2>/dev/null
  then
    echo 0
  elif [[ $(is_flag_set "--gui") == "1" && $(is_flag_set "-g") == "1" ]]
  then
    if grep -xi "$1" "$DIR"/.symlinksgui
    then
      echo 0
    else
      echo 1
    fi
  else
    echo 1
  fi
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
    echo 0
  else
    echo 1
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
  echo "    install    Create symlinks, install plugins and install dotfiles CLI"
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
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic of this script. Workers are called by
# action functions in series.

# worker_install_dotfiles_cli
#
# Put "dotfiles.sh" on the $PATH
worker_install_dotfiles_cli()
{
  if [[ ! -e ~/bin/dotfiles ]]
  then
    message_worker "Installing dotfiles cli"
    mkdir -pv ~/bin
    ln -snvf "$DIR"/dotfiles.sh ~/bin/dotfiles
  fi
}

# worker_install_symlinks
#
# Create symlinks excluding any symlinks that are ignored. Symlinks that are in
# child directories of $HOME will trigger creation of those directories.
worker_install_symlinks()
{
  message_worker "Installing symlinks"
  for file in "$DIR"/.symlinks "$DIR"/.symlinksgui
  do
    local link
    while read -r link
    do
      if [[ $(is_file_ignored "$link") == "1" && $(does_symlink_exist "$link") == "1" ]]
      then
        if [[ $link == *"/"* ]]
        then
          mkdir -pv ~/."$(echo "$link" | rev | cut -d/ -f2- | rev)"
        fi
        ln -snvf "$DIR"/files/"$link" ~/."$link"
      fi
    done < "$file"
  done
  chmod -c 600 ~/.ssh/config 2>/dev/null
}

# worker_install_vim_plugins
#
# Install vim plugins managed by vim-plug as long as the "vim" symlink exists.
worker_install_vim_plugins()
{
  if [[ $(is_program_installed "vim") == "0" && $(does_symlink_exist "vim") == "0" ]]
  then
    message_worker "Installing vim plugins"
    if [[ ! -e $DIR/files/vim/autoload/plug.vim ]]
    then
      curl -fLo "$DIR"/files/vim/autoload/plug.vim --create-dirs https://raw.githubusercontent.com/junegunn/vim-plug/master/plug.vim
    fi
    if [[ -f /.dockerenv ]]
    then
      vim +PlugUpdate +qall >/dev/null 2>&1
    else
      vim +PlugUpdate +qall
    fi
  fi
}

# worker_uninstall_symlinks
#
# Remove all symlinks that are not ignored.
worker_uninstall_symlinks()
{
  message_worker "Removing symlinks"
  for file in "$DIR"/.symlinks "$DIR"/.symlinksgui
  do
    while read -r link
    do
      if [[ $(is_file_ignored "$link") == "1" && $(does_symlink_exist "$link") == "0" ]]
      then
        rm -vf ~/."$link"
      fi
    done < "$file"
  done
}

# }}}
# Assertions -------------------------------------------------------------- {{{
#
# Assertions about the machines state that exit with an error if they are not
# meet.

# assert_user_permissions
#
# Verify that if this script is being run by root, that the command line flags
# "-r" or "--root" are set.
assert_user_permissions()
{
  if [[ $EUID -eq 0 && ($(is_flag_set "--root") == "1" && $(is_flag_set "-r") == "1") ]]
  then
    message_error "Do not run this script as root. To skip this check pass the command line flag '--root'."
    exit 1
  fi
}

# }}}
# Actions ----------------------------------------------------------------- {{{
#
# Functions which are triggered based on command line input.

# action_install
#
# Perform a full install.
action_install()
{
  worker_install_symlinks
  worker_install_vim_plugins
  worker_install_dotfiles_cli
}

# action_uninstall
#
# Perform a full uninstall.
action_uninstall()
{
  worker_uninstall_symlinks
}

# action_usage
#
# Print usage instructions.
action_usage()
{
  message_usage
}

# }}}
# Main -------------------------------------------------------------------- {{{
#
# The entry point to this script. If the user is allowed to trigger actions,
# they will be triggered based on the command line arguments.

# Abort if the root user is running this without permission.
assert_user_permissions

# Iterate through the command line input.
for i in "$@"
do
  case $i in

    # Skip any flags. They should already have been processed when the $OPTS
    # was initialized.
    -*)
      ;;

    # Call the action function for any of the valid action keywords. Only the
    # first one that is found will be processed and immediately after this
    # script will exit.
    help | install | uninstall)
      trigger_action "$i"
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
action_usage
exit 1

# }}}