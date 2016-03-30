#!/bin/bash

# Globals ----------------------------------------------------------------- {{{
#
# Constant values which exist in the global scope of this script.

# Get absolute path to the dotfiles project directory. This value will be
# correct even if this script is executed from a symlink or while your working
# directory is not the root of this project.
DIR=$(cd $(dirname "$(readlink -f "$0")") && pwd)

# Read command line options. While reading this input the 'getopt' call will
# report invalid options that were given.
OPTS=$(getopt -o rg -l root,gui -n "$(basename $0)" -- "$@")

# }}}
# Helpers ----------------------------------------------------------------- {{{
#
# Helper functions. Helper functions which return a boolean value will echo a
# zero or one to stdout which the calling function can process.

# helper_alias
#
# Trigger an action based on a command line argument.
#
# Args:
#     $1 - The command line argument to map to an action.
helper_alias()
{
  case $1 in
    help)
      action_usage
      ;;
    *)
      eval "action_"$1
      ;;
  esac
}

# helper_flag_set
#
# Check if a command line flag is set.
#
# Args:
#     $1 - The flag to check.
helper_flag_set()
{
  if [[ " "$OPTS" " == *\ $1\ * ]]
  then
    echo 0
  else
    echo 1
  fi
}

# helper_symlink_exists
#
# Check if a given symlink exists in $HOME.
#
# Args:
#     $1 - The symlink to be checked.
#
# return:
#     bool - True of the symlink exists.
helper_symlink_exists()
{
  if [[ $(readlink -f $DIR/files/$1) == $(readlink -f ~/.$1) ]]
  then
    echo 0
  else
    echo 1
  fi
}

# helper_file_ignored
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
helper_file_ignored()
{
  if [[ -n $(cat $DIR/.symlinksignore 2>/dev/null | grep -xi $1) ]]
  then
    echo 0
  elif [[ $(helper_flag_set "--gui") == "1" && $(helper_flag_set "-g") == "1" && $(cat $DIR/.symlinks | grep -w $1 | cut -d " " -s -f 2) == *g* ]]
  then
    echo 0
  else
    echo 1
  fi
}

# helper_program_installed
#
# Check if a program is installed on your $PATH.
#
# Args:
#     $1 - The program to be checked.
#
# return:
#     bool - True of the program is installed.
helper_program_installed()
{
  if [[ -n $(which $1 2>/dev/null) ]]
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
  echo "Usage: $(basename $0) <command> [-r | --root] [-g | --gui]"
  echo
  echo "These are the available commands:"
  echo
  echo "    help       Print this usage message"
  echo "    install    Update git submodules, create symlinks and install editor plugins"
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
  echo -e "\033[1;34m::\033[0m\033[1m "$1"...\033[0m"
}

# message_exit
#
# Print an exit message.
#
# Args:
#     $1 - The reason for exiting.
message_exit()
{
  echo -e "\033[1;31maborting:\033[0m "$1
}

# message_invalid
#
# Print an invalid command message.
#
# Args:
#     $1 - The invalid command.
message_invalid()
{
  echo "$(basename $0): '$1' is not a valid command. See '$(basename $0) help'."
}

# }}}
# Exit Checks ------------------------------------------------------------- {{{
#
# Functions that will stop this script from executing and exit with a non zero
# exit status under some condition.

# exit_check_root
#
# Exit with an error if this script is being run by root and the command line
# flags "-r" or "--root" are not set.
exit_check_root()
{
  if [[ $EUID -eq 0 && ($(helper_flag_set "--root") == "1" && $(helper_flag_set "-r") == "1") ]]
  then
    message_exit "Do not run this script as root. To skip this check pass the command line flag '--root'."
    exit 1
  fi
}

# }}}
# Workers ----------------------------------------------------------------- {{{
#
# Functions that perform the core logic of this script. Workers are called by
# action functions in series.

# worker_install_symlinks
#
# Create symlinks excluding any symlinks that are ignored. Symlinks that are in
# child directories of $HOME will trigger creation of those directories.
worker_install_symlinks()
{
  message_worker "Creating symlinks"
  for link in $(cat $DIR/.symlinks | cut -d " " -f 1)
  do
    if [[ $(helper_file_ignored "$link") == "1" && $(helper_symlink_exists "$link") == "1" ]]
    then
      if [[ $link == *"/"* ]]
      then
        mkdir -pv ~/.$(echo $link | rev | cut -d/ -f2- | rev)
      fi
      ln -snvf $DIR/files/$link ~/.$link
    fi
  done
  chmod -c 600 ~/.ssh/config 2>/dev/null
}

# worker_install_vim_plugins
#
# Install vim plugins managed by vim-plug as long as the "vim" symlink exists.
worker_install_vim_plugins()
{
  if [[ $(helper_program_installed "vim") == "0" && $(helper_symlink_exists "vim") == "0" ]]
  then
    message_worker "Installing vim plugins"
    if [[ ! -e $DIR/files/vim/autoload/plug.vim ]]
    then
      curl -fLo $DIR/files/vim/autoload/plug.vim --create-dirs https://raw.githubusercontent.com/junegunn/vim-plug/master/plug.vim
    fi
    vim +PlugUpdate +qall
  fi
}

# worker_install_atom_packages
#
# Install atom packages listed in "files/atom/.package-list" if the package is
# not already installed.
worker_install_atom_packages()
{
  if [[ $(helper_file_ignored "atom/config.cson") == "1" && $(helper_program_installed "apm") == "0" ]]
  then
    message_worker "Installing atom packages"
    local PACKAGES
    PACKAGES=$(apm list -b | cut -d@ -f1)
    for package in $(cat $DIR/files/atom/.package-list)
    do
      if [[ -z $(echo $PACKAGES | grep -sw $package) ]]
      then
        apm install $package
      fi
    done
  fi
}

# worker_uninstall_symlinks
#
# Remove all symlinks that are not ignored.
worker_uninstall_symlinks()
{
  message_worker "Removing symlinks"
  for link in $(cat $DIR/.symlinks | cut -d " " -f 1)
  do
    if [[ $(helper_file_ignored "$link") == "1" && $(helper_symlink_exists "$link") == "0" ]]
    then
      rm -vf ~/.$link
    fi
  done
}

# }}}
# Actions ----------------------------------------------------------------- {{{
#
# Functions which are triggered based on command line input. Each action will
# trigger work to be performed by calling a series of worker functions. Some
# functions may return immediately if some preconditions are not satisfied for
# the work to be done.

# action_install
#
# Perform a full install.
action_install()
{
  worker_install_symlinks
  worker_install_vim_plugins
  worker_install_atom_packages
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
exit_check_root

# Iterate through the command line input.
for i in $@
do
  case $i in

    # Skip any flags. They should already have been processed when the $OPTS
    # was initialized.
    -*)
      ;;

    # Call the action function for any of the valid action keywords. Only the
    # first one that is found will be processed and immediately after this
    # script will exit.
    install | uninstall | help)
      helper_alias $i
      exit
      ;;

    # If an argument is found that is not valid exit with an error.
    *)
      message_invalid $i
      exit 1
      ;;
  esac
done

# If no actions triggered when processing the command line input, print the
# usage instructions and exit with error.
action_usage
exit 1

# }}}
