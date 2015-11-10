# Dotfiles

This is a project to store configuration files for various Linux applications. The provided installation script will create symlinks in $HOME.

The files which will be effected can be seen in [files-list](files-list).

**WARNING**: Existing dotfiles may be overridden by installing this configuration.

## Configure

Files listed in .listignore will be ignored.

## Install

To update dependencies and create symlinks in $HOME run the following command. Note that this will also update the vim plugins managed by [vim-plug](https://github.com/junegunn/vim-plug) and install atom packages.

    ./setup.sh install

## Uninstall

Remove all the symlinks created in $HOME. Note that the uninstall process will leave behind directories in $HOME that contained symlinks to ensure that other files, not managed by this project, are not also removed.

    ./setup.sh uninstall

## Root user

This installation will potentially override many files in the users $HOME. The installation will not proceed if run as root to protect the root configuration. If you would like to force the install to run as root you must run the following.

    ./setup.sh install --allow-root

## Docker image

Build a Debian image with configuration from this project using the included [Dockerfile](Dockerfile).

    docker build .
