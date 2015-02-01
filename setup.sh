#!/usr/bin/env bash

# Do not run for root users
[[ $EUID -eq 0 ]] && echo 'Error: Not runnable as root' 1>&2 && exit

# Absolute path
p=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)

# Update git submodules
echo -e "\033[1;34m::\033[0m\033[1m Updating git submodules ...\033[0m"
cd $p && git submodule init | sed "s/^/ /"
cd $p && git submodule update | sed "s/^/ /"

# Vim plugins
echo -e "\033[1;34m::\033[0m\033[1m Installing vim plugins ...\033[0m"
vim +PluginInstall +qall

# Create symlinks
echo -e "\033[1;34m::\033[0m\033[1m Creating symlinks ...\033[0m"
echo "This may overrite files in your home directory"
echo -e -n "\033[1;34m::\033[0m\033[1m Proceed with setup? [y/N] \033[0m" && read -p "" && [[ ! $REPLY =~ ^[yY]$ ]] && exit
mkdir -p ~/.ssh
ln -snfv $p/aliases ~/.aliases
ln -snfv $p/bash_profile ~/.bash_profile
ln -snfv $p/bashrc ~/.bashrc
ln -snfv $p/gitconfig ~/.gitconfig
ln -snfv $p/gitignore ~/.gitignore
ln -snfv $p/gtkrc-2.0 ~/.gtkrc-2.0
ln -snfv $p/i3 ~/.i3
ln -snfv $p/profile ~/.profile
ln -snfv $p/ssh/config ~/.ssh/config
ln -snfv $p/tmux.conf ~/.tmux.conf
ln -snfv $p/vim ~/.vim
ln -snfv $p/xinitrc ~/.xinitrc
ln -snfv $p/Xresources ~/.Xresources
ln -snfv $p/zprofile ~/.zprofile
ln -snfv $p/zsh ~/.zsh
ln -snfv $p/zshrc ~/.zshrc
