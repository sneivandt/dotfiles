#!/bin/bash

# Submodules
git submodule init
git submodule update

# Git
ln -sf ~/.dotfiles/gitconfig ~/.gitconfig

# Tmux
ln -sf ~/.dotfiles/tmux.conf ~/.tmux.conf

# Vim
rm -rf ~/.vim
ln -sf ~/.dotfiles/vim ~/.vim
ln -sf ~/.dotfiles/vim/autoload/vim-pathogen/autoload/pathogen.vim ~/.dotfiles/vim/autoload/pathogen.vim
ln -sf ~/.dotfiles/vim/colors/jellybeans/colors/jellybeans.vim ~/.dotfiles/vim/colors/jellybeans.vim
ln -sf ~/.dotfiles/vimrc ~/.vimrc

# Bash
if [[ $HOME =~ /home/* ]]
then
  ln -sf ~/.dotfiles/bashrc ~/.bashrc
else
  echo 'WARNING: bashrc not linked'
fi
