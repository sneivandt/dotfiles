# Dotfiles
```
Usage:
  dotfiles.sh
  dotfiles.sh {-I --install}   [-g] [-p] [-s]
  dotfiles.sh {-U --uninstall} [-g]
  dotfiles.sh {-T --test}
  dotfiles.sh {-h --help}

Options:
  -g  Configure GUI environment
  -p  Install system packages
  -s  Install systemd units
```
## Docker
[![Docker Build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)
```
docker run --rm -it sneivandt/dotfiles
```