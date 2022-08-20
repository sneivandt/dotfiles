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
[![Publish Docker image](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)
```
docker run --rm -it sneivandt/dotfiles
```