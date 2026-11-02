<h1 align="center"><img src="docs/public/favicon.svg" width="24" height="24" align="bottom" alt=""> Dotfiles</h1>

<p align="center">
  <strong>My Arch Linux and Windows configuration, managed with a Rust CLI.</strong>
</p>

<p align="center">
  <a href="https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml"><img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/sneivandt/dotfiles/ci.yml?branch=main&style=flat-square&label=CI"></a>
  <a href="https://github.com/sneivandt/dotfiles/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/sneivandt/dotfiles?style=flat-square&label=release"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-9ece6a?style=flat-square"></a>
</p>

<p align="center">
  <strong><a href="https://dotfiles.stuartneivandt.com/">Read the documentation</a></strong><br>
  Setup instructions, everyday commands, and customization guides.
</p>

Keep shell, editor, desktop, and system settings together in a repository.
The CLI checks what is already in place, previews the selected work, and
applies the changes. This is a personal setup, not a general-purpose
installer: review it before using it on your machine.

Desired state lives in [conf/](conf/), with application settings under
[symlinks/](symlinks/) and managed system-file fragments under [system/](system/).

<p align="center">
  <img src="docs/public/assets/terminal-screenshot.svg" width="760" alt="Animated terminal session: one dotfiles update first updates the CLI, then links configuration, refreshes shell completions, and installs missing packages">
</p>

## What it manages

| Area | Configuration |
|---|---|
| Shell | Zsh, Bash, PowerShell, login shell, tmux, PATH, and completions |
| Editors | Vim/Neovim and VS Code settings, keybindings, and extensions |
| Terminal | Alacritty and Windows Terminal |
| Git | Global settings, aliases, and repository hooks |
| Packages | Arch and AUR packages through pacman/paru, custom Arch packages from dotfiles-managed recipes, and Windows packages through winget |
| Arch desktop | Hyprland, Quickshell bar and menus, mako notifications, fuzzel launcher, locking, wallpaper, and GTK/font settings |
| Services | systemd user and system services, sockets, and timers |
| System configuration | Managed settings under `/etc`, including pacman, PAM, and WSL configuration |
| SSH and GPG | SSH client and GPG agent configuration |
| Agent tooling | Agent packages and plugins, Copilot and Codex settings |
| Windows | Current-user registry values and Developer Mode |
| WSL | Windows-side WSL networking settings and Linux-side integration |

The selected configuration depends on the profile and platform.

The `base` profile covers shell tools and core configuration for servers and
WSL. The `desktop` profile adds GUI apps and desktop services. Both profiles
are available on Linux and Windows; Linux package management and the Linux
desktop setup target Arch.

## Usage

See [Getting started](https://dotfiles.stuartneivandt.com/getting-started.html) for setup and usage.

## Development

See [Contributing](https://dotfiles.stuartneivandt.com/contributing.html) for building, testing, and changing the CLI.
