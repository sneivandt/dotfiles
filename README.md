# Dotfiles

This is a project to store configuration files for various Linux applications. The provided installation script will create symlinks in $HOME.

The symlinks which will be created are listed in [files-list](files-list).

**WARNING**: Existing dotfiles may be overridden without warning by installing this configuration.

## Install

The installation performs the following actions:

  * Install git submodules
  * Create symlinks in $HOME
  * Install vim plugins
  * Install atom packages

Install command:

    ./setup.sh install

## Uninstall

The uninstall will remove all the symlinks created in $HOME.

Uninstall command:

    ./setup.sh uninstall

## Configure

Symlinks listed in .filesignore will be ignored. Entries must match exactly the entries in [files-list](files-list) with each entry listed on its own line. Regular expressions are not supported.

## Root user

This installation will potentially override many in $HOME. The installation will not proceed if run as root to protect root configuration. If you would like to force the install to run as root you must provide the command line flag "--allow-root".

Root install command:

    ./setup.sh install --allow-root

## Docker image

Build a Docker image with configuration from this project using the included [Dockerfile](Dockerfile).

    docker build .
