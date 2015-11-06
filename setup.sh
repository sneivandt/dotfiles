#!/bin/bash

# Print usage instructions.
usage()
{
  echo "Usage: $(basename $0) <command> [-r | --allow-root]"
  echo
  echo "These are the avaliable commands:"
  echo
  echo "    help       Print this usage message"
  echo "    install    Update git submodules, create symlinks, update vim plugins and install atom packages"
  echo "    uninstall  Remove symlinks"
}

# Print a formatted message.
message()
{
  echo -e "\033[1;34m::\033[0m\033[1m "$1"...\033[0m"
}

# Print an aborting warning.
aborting()
{
  echo -e "\033[1;31maborting:\033[0m "$1
}

# Print invalid command message.
invalid()
{
  echo "$(basename $0): '$1' is not a valid command. See '$(basename $0) help'."
}

# Install and update git submodules.
install_git_submodules()
{
  message "Installing git submodules"
  git -C $DIR submodule update --init
}

# Create symlinks listed in "files-list". Any symlinks listed in ".filesignore" will
# not be affected and if the symlink already exists it will be skipped.
install_symlinks()
{
  message "Creating symlinks"
  for link in `cat $DIR/files-list`; do
    if [[ (-z `cat $DIR/.filesignore 2>/dev/null | grep -Fx $link`) && (`readlink -f $DIR/files/$link` != `readlink -f ~/.$link`) ]]; then
      if [[ $link == *"/"* ]]; then
        mkdir -pv ~/.`echo $link | rev | cut -d/ -f2- | rev`
      fi
      ln -snvf $DIR/files/$link ~/.$link
    fi
  done
  chmod -c 600 ~/.ssh/config 2>/dev/null
}

# Install vim plugins managed by vim-plug.
install_vim_plugins()
{
  if [[ -n `which vim` ]]; then
    message "Installing vim plugins"
    vim +PlugUpdate +qall
  fi
}

# Install atom packages listed in "files/atom/packages-list" if the package is
# not already installed.
install_atom_packages()
{
  if [[ -n `which apm` ]]; then
    message "Installing atom packages"
    local PACKAGES
    PACKAGES=$(apm list -b | cut -d@ -f1)
    for package in `cat $DIR/files/atom/packages-list`; do
      if [[ -z `echo $PACKAGES | grep -sw $package` ]]; then
        apm install $package
      fi
    done
  fi
}

# Remove symlinks listed in "files-list". Any symlinks listed in ".filesignore"
# will not be affected.
uninstall_symlinks()
{
  message "Removing symlinks"
  for link in `cat $DIR/files-list`; do
    if [[ (-z `cat $DIR/.filesignore 2>/dev/null | grep -Fx $link`) && (`readlink -f $DIR/files/$link` == `readlink -f ~/.$link`) ]]; then
      rm -vf ~/.$link
    fi
  done
}

# Exit with an error code 1 if this script is being run by root and the command
# line flags "-r" or "--allow-root" are not set.
check_root()
{
  if [[ $EUID -eq 0 && ($OPTS != *--allow-root* && $OPTS != *-r*) ]];then
    aborting "Do not run this script as root. To skip this check pass the command line flag '--allow-root'."
    exit 1
  fi
}

# Perform a full install.
install()
{
  install_git_submodules
  install_symlinks
  install_vim_plugins
  install_atom_packages
}

# Perform a full uninstall.
uninstall()
{
  uninstall_symlinks
}

# Trigger an action based on an a command line argument.
alias()
{
  case $1 in
    help)
      usage
      ;;
    *)
      eval $1
      ;;
  esac
}

# Get absolute path to the dofiles project folder.
DIR=$(cd $(dirname "$(readlink -f "$0")") && pwd)

# Get command line options.
OPTS=$(getopt -o r -l allow-root -n "$(basename $0)" -- "$@")

# Perform root check before triggering any actions.
check_root

# Trigger actions based on command line arguments.
for i in $@; do
  case $i in
    -*)
      ;;
    install | uninstall | help)
      alias $i
      exit
      ;;
    *)
      invalid $i
      exit 1
      ;;
  esac
done

# Print usage instructions if no arguments were provided.
usage
