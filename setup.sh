#!/bin/bash

git submodule init
git submodule update

rm -rf ~/.vim
ln -sf ~/.dotfiles/vim ~/.vim
ln -sf ~/.dotfiles/vimrc ~/.vimrc
ln -sf ~/.dotfiles/tmux.conf ~/.tmux.conf
ln -sf ~/.dotfiles/gitconfig ~/.gitconfig

if [[ $HOME =~ /home/* ]]
then
  ln -sf ~/.dotfiles/bashrc ~/.bashrc
else
  echo 'WARNING: bashrc not linked'
fi
