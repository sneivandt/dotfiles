# Dotfiles [![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

These are my dotfiles.

## Installation

Install this configuration.

```
./dotfiles.sh install
```

Optional arguments.

| Argument | Description                 |
| -        | -                           |
| --gui    | Configure GUI applications. |
| --pack   | Install packages.           |
| --root   | Allow running as root.      |

## Modules

Additional configuration will be lazily loaded if required.

+ [Base GUI](https://github.com/sneivandt/dotfiles-base-gui)
+ [Arch](https://github.com/sneivandt/dotfiles-arch)
+ [Arch GUI](https://github.com/sneivandt/dotfiles-arch-gui)
+ [WSL](https://github.com/sneivandt/dotfiles-wsl)

## Docker

Run a container with this configuration.

```
docker run -it sneivandt/dotfiles
```
