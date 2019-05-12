# Dotfiles

[![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

These are my dotfiles.

## Installation

```
./dotfiles.sh install
```

### Commands

| Command   | Description                                 |
| -         | -                                           |
| help      | Show usage instructions                     |
| install   | Install symlinks, packages and dotfiles CLI |
| uninstall | Remove symlinks                             |

### Options

| Short | Long | Description               |
| -     | -    | -                         |
| g     | gui  | Include GUI config        |
| s     | sudo | Include privileged config |

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
