# Dotfiles

[![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

These are my dotfiles.

## Installation

```
./dotfiles.sh --install
```

### Commands

| Command | Description   |
| -       | -             |
| -I      | Install       |
| -U      | Uninstall     |
| -h      | Display usage |

### Options

| Option | Description            |
| -      | -                      |
| -g     | Configure GUI programs |
| -s     | Use sudo               |

## Modules

Additional configuration will be lazily loaded if required.

+ [Base GUI](https://github.com/sneivandt/dotfiles-base-gui)
+ [Arch](https://github.com/sneivandt/dotfiles-arch)
+ [Arch GUI](https://github.com/sneivandt/dotfiles-arch-gui)

## Docker

Run a container with this configuration.

```
docker run -it sneivandt/dotfiles
```
