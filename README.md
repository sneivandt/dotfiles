# Dotfiles
## Usage
```
Usage: dotfiles.sh <command> [<options>]

Commands:

  -I, --install    : Install
  -U, --uninstall  : Uninstall
  -h, --help       : Display usage

Options:

  -g               : Configure GUI programs
  -s               : Use sudo

Examples:

  dotfiles.sh --install      # Install
  dotfiles.sh --uninstall    # Uninstall
```
## Docker
[![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)
```
docker run -it sneivandt/dotfiles
```
