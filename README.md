# Dotfiles

Configuration for a Linux development environment.

## Programs

Program | Dotfiles
--------|---------
bash    | [.bash_profile](files/bash_profile) [.bashrc](files/bashrc)
curl    | [.curlrc](files/curlrc)
git     | [.gitattributes](files/gitattributes) [.gitconfig](files/gitconfig) [.config/git/config](files/config/git/config)
nvim    | [.config/nvim/init.vim](files/vim/vimrc)
ssh     | [.ssh/config](files/ssh/config)
tmux    | [.tmux.conf](files/tmux.conf)
vim     | [.vim/vimrc](files/vim/vimrc)
wget    | [.wgetrc](files/wgetrc)
zsh     | [.zshenv](files/zshenv) [.zshrc](files/zshrc)

## Graphical Programs

Program | Dotfiles
--------|---------
atom    | [.atom/config.cson](files/atom/config.cson)
compton | [.config/compton.cfg](files/config/compton.cfg)
gtk2    | [.gtkrc-2.0](files/gtkrc-2.0)
gtk3    | [.config/gtk-3.0/settings.ini](files/config/gtk-3.0/settings.ini)
gvim    | [.vim/gvimrc](files/vim/gvimrc)
i3      | [.i3/config](files/i3/config)
X       | [.xinitrc](files/xinitrc) [.Xresources](files/Xresources)

## Install

Install will create symlinks in $HOME, install editor plugins and put [dot.sh](dot.sh) on the users path.

    ./dot.sh install

## Uninstall

Uninstall will remove all the symlinks created in $HOME.

    ./dot.sh uninstall

## Flags

Command line flags.

Short  | Long  | Behavior
-------|-------|---------
g      | gui   | Include graphical programs
r      | root  | Allow execution as root

## Configure

If you want to ignore some files, create a file *.symlinksignore* and list the files there. This file should have the same structure as [.symlinks](.symlinks) but without the *g* flags for graphical programs.

## Docker

Start a container with this configuration.

    docker-compose run --rm dotfiles
