# Dotfiles [![Build Status](https://travis-ci.org/sneivandt/dotfiles.svg?branch=master)](https://travis-ci.org/sneivandt/dotfiles)

Configuration for a Linux development environment.

## Install

* Create symlinks
* Install plugins
* Install [dotfiles.sh](dotfiles.sh)

```
./dotfiles.sh install
```

Command line flags.

Short  | Long  | Behavior
-------|-------|---------
g      | gui   | Include graphical programs
r      | root  | Allow execution as root

## Configure

Files listed in *.symlinksignore* will be ignored.

## Docker

Start a container with this configuration.

```
docker-compose run --rm dotfiles
```
