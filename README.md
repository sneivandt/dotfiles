# Dotfiles [![Docker Automated build](https://img.shields.io/docker/automated/sneivandt/dotfiles.svg)](https://hub.docker.com/r/sneivandt/dotfiles/)

System configuration.

## Usage

Install symlinks, package managers, packages and dotfiles CLI.

```
./dotfiles.sh install
```

| Argument | Description                     |
| -        | -                               |
| --gui    | Include graphical applications. |
| --root   | Allow dotfiles to run as root.  |

## Docker

Run a container with this configuration.

```
docker run -it sneivandt/dotfiles
```
