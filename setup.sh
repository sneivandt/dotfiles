#!/bin/bash

<<GLOBALS

  Values which exist in the global scope of this script.

GLOBALS

# Get absolute path to the dofiles project folder.
DIR=$(cd $(dirname "$(readlink -f "$0")") && pwd)

# Read command line options.
OPTS=$(getopt -o r -l allow-root -n "$(basename $0)" -- "$@")

<<HELPERS

  Helper functions.

HELPERS

# helper_alias
#
# Trigger an action based on a command line argument.
#
# $1 - The command line argument to map to an action.
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

<<MESSAGES

  Functions which write to stdout.

MESSAGES

# message_usage
#
# Print usage instructions
message_usage()
{
  echo "Usage: $(basename $0) <command> [-r | --allow-root]"
  echo
  echo "These are the available commands:"
  echo
  echo "    help       Print this usage message"
  echo "    install    Update git submodules, create symlinks, update vim plugins and install atom packages"
  echo "    uninstall  Remove symlinks"
}

# message_worker
#
# Print a worker starting message.
#
# $1 - The work is being performed.
message_worker()
{
  echo -e "\033[1;34m::\033[0m\033[1m "$1"...\033[0m"
}

# message_exit
#
# Print an exit message.
#
# $1 - The reason for exiting.
message_exit()
{
  echo -e "\033[1;31maborting:\033[0m "$1
}

# message_invalid
#
# Print an invalid command message.
#
# $1 - The invalid command.
message_invalid()
{
  echo "$(basename $0): '$1' is not a valid command. See '$(basename $0) help'."
}

<<EXIT_CHECKS

  Functions that will stop this script from executing and exit with a non zero
  exit status under some condition.

EXIT_CHECKS

# exit_check_root
#
# Exit with an error if this script is being run by root and the command line
# flags "-r" or "--allow-root" are not set.
exit_check_root()
{
  if [[ $EUID -eq 0 && ($OPTS != *--allow-root* && $OPTS != *-r*) ]];then
    message_exit "Do not run this script as root. To skip this check pass the command line flag '--allow-root'."
    exit 1
  fi
}

<<WORKERS

  Functions that perform the core logic of this script.

WORKERS

# worker_install_git_submodules
#
# Install and update git submodules.
worker_install_git_submodules()
{
  message_worker "Installing git submodules"
  if [[ ! $(git -C $DIR submodule update --init 2>/dev/null) ]]; then
    cd $DIR && git submodule update --init
  fi
}

# worker_install_symlinks
#
# Create symlinks listed in "files-list" excluding any symlinks listed in
# ".filesignore".
worker_install_symlinks()
{
  message_worker "Creating symlinks"
  for link in $(cat $DIR/files-list); do
    if [[ (-z $(cat $DIR/.filesignore 2>/dev/null | grep -Fx $link)) && ($(readlink -f $DIR/files/$link) != $(readlink -f ~/.$link)) ]]; then
      if [[ $link == *"/"* ]]; then
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
  if [[ (-n $(which vim 2>/dev/null)) && ($(readlink -f $DIR/files/vim) == $(readlink -f ~/.vim)) ]]; then
    message_worker "Installing vim plugins"
    vim +PlugUpdate +qall
  fi
}

# worker_install_atom_packages
#
# Install atom packages listed in "files/atom/packages-list" if the package is
# not already installed.
worker_install_atom_packages()
{
  if [[ -n $(which apm 2>/dev/null) ]]; then
    message_worker "Installing atom packages"
    local PACKAGES
    PACKAGES=$(apm list -b | cut -d@ -f1)
    for package in $(cat $DIR/files/atom/packages-list); do
      if [[ -z $(echo $PACKAGES | grep -sw $package) ]]; then
        apm install $package
      fi
    done
  fi
}

# worker_uninstall_symlinks
#
# Remove all symlinks that are not listed in ".filesignore".
worker_uninstall_symlinks()
{
  message_worker "Removing symlinks"
  for link in $(cat $DIR/files-list); do
    if [[ (-z $(cat $DIR/.filesignore 2>/dev/null | grep -Fx $link)) && ($(readlink -f $DIR/files/$link) == $(readlink -f ~/.$link)) ]]; then
      rm -vf ~/.$link
    fi
  done
}

<<ACTIONS

  Functions which are triggered based on command line input. Each action will
  trigger work to be performed by calling a series of worker functions.

ACTIONS

# action_install
#
# Perform a full install.
action_install()
{
  worker_install_git_submodules
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

<<MAIN

  The entry point to this script. If the user is allowed to trigger actions,
  they will be triggered based on the command line arguments.

MAIN

exit_check_root
for i in $@; do
  case $i in
    -*)
      ;;
    install | uninstall | help)
      helper_alias $i && exit
      ;;
    *)
      message_invalid $i && exit 1
      ;;
  esac
done
action_usage
