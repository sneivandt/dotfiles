#!/usr/bin/env bash

# Absolute path to this script
ABS_PATH=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)

# Update repo
echo -e "\033[1;34m::\033[0m\033[1m Updating git ...\033[0m"
git -C $ABS_PATH pull | sed "s/^/ /"

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
ln -sf $ABS_PATH/gitconfig ~/.gitconfig

echo " Terminfo"
rm -rf ~/.terminfo
ln -sf $ABS_PATH/terminfo ~/.terminfo

echo " Vim"
rm -rf ~/.vim
ln -sf $ABS_PATH/vim ~/.vim
ln -sf $ABS_PATH/vim/autoload/vim-pathogen/autoload/pathogen.vim ~/.dotfiles/vim/autoload/pathogen.vim
ln -sf $ABS_PATH/vim/colors/jellybeans/colors/jellybeans.vim ~/.dotfiles/vim/colors/jellybeans.vim

echo " Tmux"
ln -sf $ABS_PATH/tmux.conf ~/.tmux.conf

# Exit if running as root
[[ $EUID -eq 0 ]] && exit

echo " Bash"
ln -sf $ABS_PATH/bash_profile ~/.bash_profile
ln -sf $ABS_PATH/bashrc ~/.bashrc

echo " X"
ln -sf $ABS_PATH/xinitrc ~/.xinitrc
ln -sf $ABS_PATH/Xresources ~/.Xresources

echo " GTK+"
ln -sf $ABS_PATH/gtkrc-2.0 ~/.gtkrc-2.0

echo " i3"
rm -rf ~/.i3
ln -sf $ABS_PATH/i3 ~/.i3
