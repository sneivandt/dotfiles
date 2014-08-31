#!/bin/bash

git submodule init
git submodule update

rm -rf ~/.vim
ln -sf ~/.dotfiles/vim ~/.vim
ln -sf ~/.dotfiles/gitconfig ~/.gitconfig
ln -sf ~/.dotfiles/tmux.conf ~/.tmux.conf

if [[ $HOME =~ /home/* ]]
then
  ln -sf ~/.dotfiles/bashrc ~/.bashrc
else
  echo 'WARNING: bashrc not modified'
fi
