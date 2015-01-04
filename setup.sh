#!/usr/bin/env bash

# Absolute path to this script
ABS_PATH=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)

# Update submodules
echo -e "\033[1;34m::\033[0m\033[1m Updating git submodules ...\033[0m"
git -C $ABS_PATH submodule init | sed "s/^/ /"
git -C $ABS_PATH submodule update | sed "s/^/ /"

# Create symlinks
echo -e "\033[1;34m::\033[0m\033[1m Creating symlinks ...\033[0m"
echo " This may overrite files in your home directory"
echo -e -n "\033[1;34m::\033[0m\033[1m Proceed with setup? [y/N] \033[0m"
read -p "" &&  [[ ! $REPLY =~ ^[yY]$ ]] && exit

echo " Git"
ln -snf $ABS_PATH/gitconfig ~/.gitconfig

echo " Vim"
ln -snf $ABS_PATH/vim ~/.vim

echo " Tmux"
ln -snf $ABS_PATH/tmux.conf ~/.tmux.conf

echo " Terminfo"
ln -snf $ABS_PATH/terminfo ~/.terminfo

# Non root users only
if [[ $EUID -ne 0 ]]
then
  echo " Bash"
  ln -snf $ABS_PATH/bash_profile ~/.bash_profile
  ln -snf $ABS_PATH/bashrc ~/.bashrc

  echo " X"
  ln -snf $ABS_PATH/xinitrc ~/.xinitrc
  ln -snf $ABS_PATH/Xresources ~/.Xresources

  echo " GTK+"
  ln -snf $ABS_PATH/gtkrc-2.0 ~/.gtkrc-2.0

  echo " i3"
  ln -snf $ABS_PATH/i3 ~/.i3
fi
