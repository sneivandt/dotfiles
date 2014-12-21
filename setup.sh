#!/bin/bash

# Absolute path to this script
ABS_PATH=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)

# Update repo
echo -e "\033[1;34m--\033[0m\033[1m Updating git ...\033[0m"
git -C $ABS_PATH pull | sed "s/^/ /"

# Update submodules
echo -e "\033[1;34m--\033[0m\033[1m Updating git submodules ...\033[0m"
git -C $ABS_PATH submodule init | sed "s/^/ /"
git -C $ABS_PATH submodule update | sed "s/^/ /"

# Create symlinks
echo -e "\033[1;34m--\033[0m\033[1m Creating symlinks ...\033[0m"
echo " This may overrite files in your home directory"
echo -e -n "\033[1;34m--\033[0m\033[1m Proceed with setup? [y/N] \033[0m"
read -p "" &&  [[ ! $REPLY =~ ^[yY]$ ]] && exit

echo " Git"
ln -sf $ABS_PATH/gitconfig ~/.gitconfig

echo " Vim"
rm -rf ~/.vim
ln -sf $ABS_PATH/vim ~/.vim
ln -sf $ABS_PATH/vim/autoload/vim-pathogen/autoload/pathogen.vim ~/.dotfiles/vim/autoload/pathogen.vim
ln -sf $ABS_PATH/vim/colors/jellybeans/colors/jellybeans.vim ~/.dotfiles/vim/colors/jellybeans.vim

echo " Tmux"
ln -sf $ABS_PATH/tmux.conf ~/.tmux.conf

if [[ $EUID -ne 0 ]]
then
  echo " Bash"
  ln -sf $ABS_PATH/bash_profile ~/.bash_profile
  ln -sf $ABS_PATH/bashrc ~/.bashrc

  if [[ -n $DISPLAY ]]
  then
    echo " X"
    ln -sf $ABS_PATH/xinitrc ~/.xinitrc
    ln -sf $ABS_PATH/Xresources ~/.Xresources

    echo " GTK+"
    ln -sf $ABS_PATH/gtkrc-2.0 ~/.gtkrc-2.0

    if [[ -x $(which i3 2>/dev/null) ]]
    then
      echo " i3"
      rm -rf ~/.i3
      ln -sf $ABS_PATH/i3 ~/.i3
    fi
  fi
fi

unset ABS_PATH
