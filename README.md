# Dotfiles

Configuration for a Linux development environment.

Program | Dotfiles
--------|---------
atom    | [.atom/config.cson](files/atom/config.cson)
bash    | [.bash_profile](files/bash_profile) [.bashrc](files/bashrc)
curl    | [.curlrc](files/curlrc)
git     | [.gitattributes](files/gitattributes) [.gitconfig](files/gitconfig) [.gitignore](files/gitignore) [.config/git/config](files/config/git/config)
gtk2    | [.gtkrc-2.0](files/gtkrc-2.0)
gtk3    | [.config/gtk-3.0/settings.ini](files/config/gtk-3.0/settings.ini)
i3      | [.i3/config](files/i3/config)
nvim    | [.config/nvim/init.vim](files/vim/vimrc)
ssh     | [.ssh/config](files/ssh/config)
tmux    | [.tmux.conf](files/tmux.conf)
vim     | [.vim/vimrc](files/vim/vimrc)
wget    | [.wgetrc](files/wgetrc)
x       | [.xinitrc](files/xinitrc) [.Xresources](files/Xresources)
zsh     | [.zshenv](files/zshenv) [.zshrc](files/zshrc)

## Install

stall will install git submodules, create symlinks in $HOME and install editor plugins.

    ./dot.sh install

## Uninstall

Uninstall will remove all the symlinks created in $HOME.

    ./dot.sh uninstall

## Flags

Short | Long       | Behavior
------|------------|----------
g     | gui        | Include graphical programs
r     | allow-root | Allow root user

## Configure

If you want to ignore some files, create a file .symlinksignore and list the files there. This file should have the same structure as [.symlinks](.symlinks) but without the "g" flags for graphical programs.

## Vagrant

Build a vagrant image using Docker provider and ssh into the container.

    vagrant up && vagrant ssh
