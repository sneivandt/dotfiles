<h1 align="center"><img src="docs/public/favicon.svg" width="24" height="24" align="bottom" alt=""> Dotfiles</h1>

<p align="center">
  My Arch Linux and Windows configuration, managed with a Rust CLI.
</p>

<p align="center">
  <a href="https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml"><img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/sneivandt/dotfiles/ci.yml?branch=main&style=flat-square&label=CI"></a>
  <a href="https://github.com/sneivandt/dotfiles/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/sneivandt/dotfiles?style=flat-square&label=release"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-9ece6a?style=flat-square"></a>
</p>

I keep my shell, editor, desktop, and system settings here so I can manage them
together. The CLI checks what's already in place and applies the changes I select,
with a dry-run option to preview them first. This is my personal setup rather than
a general-purpose tool, so read the [documentation](https://dotfiles.stuartneivandt.com/)
and review the settings before using it on your own machine.

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
| Services | systemd services, sockets, and timers, including automatic cleanup of old temporary files and screenshots |
| System configuration | Managed settings under `/etc`, including pacman, PAM, and WSL configuration |
| SSH and GPG | SSH client and GPG agent configuration |
| Agent tooling | Agent packages and plugins, Copilot and Codex settings |
| Windows | Current-user registry values and Developer Mode |
| WSL | Windows-side WSL networking settings and Linux-side integration |

## Reference

- [Getting started](https://dotfiles.stuartneivandt.com/getting-started.html): setup and usage.
- [Contributing](https://dotfiles.stuartneivandt.com/contributing.html): building, testing, and changing the CLI.
