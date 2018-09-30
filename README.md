# Dotfiles [![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

These are my dotfiles including configuration for a generic Linux environment as well as [Arch Linux](https://www.archlinux.org/) and [WSL](https://docs.microsoft.com/en-us/windows/wsl/about) specific configuration.

## Install

```
./dotfiles.sh install
```

| Argument | Description                     |
| -        | -                               |
| --gui    | Configure GUI applications.     |
| --pack   | Install packages.               |
| --root   | Allow running as root.          |

## Docker

Run a Docker container with this configuration.

```
docker run -it sneivandt/dotfiles
```
