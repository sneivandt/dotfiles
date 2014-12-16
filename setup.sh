#!/bin/bash

# Absolute path
REPO_PATH=$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd -P )

# Update submodules
git --git-dir $REPO_PATH/.git submodule init
git --git-dir $REPO_PATH/.git submodule update

# Symlinks
rm -rf ~/.vim
ln -sf $REPO_PATH/vim ~/.vim
ln -sf $REPO_PATH/tmux.conf ~/.tmux.conf
ln -sf $REPO_PATH/gitconfig ~/.gitconfig

# Symlink .bash_profile and .bashrc for non root users only
if [[ $HOME =~ /home/* ]]
then
  ln -sf $REPO_PATH/bash_profile ~/.bash_profile
  ln -sf $REPO_PATH/bashrc ~/.bashrc
fi

# X symlinks
if [[ -n $DISPLAY ]]
then
  ln -sf $REPO_PATH/xinitrc ~/.xinitrc
  ln -sf $REPO_PATH/Xresources ~/.Xresources
  ln -sf $REPO_PATH/gtkrc-2.0 ~/.gtkrc-2.0
fi

# i3 symlings
if [[ -f /usr/bin/i3 ]]
then
  rm -rf ~/.i3
  ln -sf $REPO_PATH/i3 ~/.i3
fi
