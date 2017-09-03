# Dotfiles [![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

Configuration for my Linux development environment.

## Install

Install symlinks, package managers and dotfiles CLI.

```
./dotfiles.sh install
```

## Configure

Files listed in *symlinksignore* will be ignored.

## Docker

Run a container with this configuration.

```
docker run -it sneivandt/dotfiles
```
