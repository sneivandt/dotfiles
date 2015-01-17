#!/usr/bin/env bash

# Do not run for root users
[[ $EUID -eq 0 ]] && echo 'Error: Not runnable as root' 1>&2 && exit

# Absolute path to setup.sh
p=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)

# Update git submodules
echo -e "\033[1;34m::\033[0m\033[1m Updating git submodules ...\033[0m"
git -C $p submodule init | sed "s/^/ /"
git -C $p submodule update | sed "s/^/ /"

# Vim plugins
echo -e "\033[1;34m::\033[0m\033[1m Installing vim plugins ...\033[0m"
vim +PluginInstall +qall

# Create symlinks
echo -e "\033[1;34m::\033[0m\033[1m Creating symlinks ...\033[0m"
echo " This may overrite files in your home directory"
echo -e -n "\033[1;34m::\033[0m\033[1m Proceed with setup? [y/N] \033[0m" && read -p "" && [[ ! $REPLY =~ ^[yY]$ ]] && exit
for file in $(ls $p -I setup.sh -I README.md)
do
  echo " "$file
  ln -snf $p/$file ~/.$file
done
