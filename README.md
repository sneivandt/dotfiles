# Dotfiles

Configuration for a Linux development environment.

### Programs

Program|Dotfiles
-------|--------
atom|[.atom/config.cson](files/atom/config.cson)
bash|[.bash_profile](files/bash_profile) [.bashrc](files/bashrc)
curl|[.curlrc](files/curlrc)
git|[.gitattributes](files/gitattributes) [.gitconfig](files/gitconfig) [.gitignore](files/gitignore)
gtk2|[.gtkrc-2.0](files/gtkrc-2.0)
gtk3|[.config/gtk-3.0/settings.ini](files/config/gtk-3.0/settings.ini)
i3|[.i3/config](files/i3/config)
nvim|[.config/nvim/init.vim](files/vim/vimrc)
ssh|[.ssh/config](files/ssh/config)
tmux|[.tmux.conf](files/tmux.conf)
vim|[.vim/vimrc](files/vim/vimrc)
wget|[.wgetrc](files/wgetrc)
x|[.xinitrc](files/xinitrc) [.Xresources](files/Xresources)
zsh|[.zshenv](files/zshenv) [.zshrc](files/zshrc)

## Install

The install will install git submodules, create symlinks in $HOME, install vim plugins and install atom packages.

    ./setup.sh install

## Uninstall

The uninstall will remove all the symlinks created in $HOME.

    ./setup.sh uninstall

## Configure

Symlinks listed in .filesignore will be ignored. Entries must match exactly the entries in [files-list](files-list) with each entry listed on its own line. Regular expressions are not supported.

## Root user

This installation will potentially override many in $HOME. The installation will not proceed if run as root to protect root configuration. If you would like to force the install to run as root you must provide the command line flag "--allow-root".

    ./setup.sh install --allow-root

## Vagrant

Build a vagrant image using Docker provider.

    vagrant up

SSH into the container.

    vagrant ssh
