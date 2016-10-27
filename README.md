# Dotfiles [![Build Status](https://travis-ci.org/sneivandt/dotfiles.svg?branch=master)](https://travis-ci.org/sneivandt/dotfiles)

Configuration for a Linux development environment.

## Install

* Create symlinks in $HOME
* Install editor plugins
* Install [dotfiles.sh](dotfiles.sh) on the user $PATH.

```
./dotfiles.sh install
```

Command line flags.

Short  | Long  | Behavior
-------|-------|---------
g      | gui   | Include graphical programs
r      | root  | Allow execution as root

## Configure

Files listed in *.symlinksignore* will be ignored. This file should be similar to [.symlinks](.symlinks).

## Docker

Start a container with this configuration.

```
docker-compose run --rm dotfiles
```
