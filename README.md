<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/public/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/public/assets/logo-light.svg">
    <img src="docs/public/assets/logo-light.svg" height="48" alt="Dotfiles">
  </picture>
</h1>

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
  <img src="docs/public/assets/terminal-screenshot.svg" width="760" alt="Animated terminal session: one dotfiles update groups CLI upgrade and repository sync notices beneath its header, then links configuration, refreshes shell completions, and installs missing packages">
</p>

## What it manages

Which settings apply depends on the operating system and the
[profile](https://dotfiles.stuartneivandt.com/profiles-and-categories):
`base` covers command-line tools, while `desktop` adds workstation settings.

| Area | Examples from this setup |
|---|---|
| Shells and terminals | Zsh, Bash, and PowerShell startup files, dotfiles completions, tmux, Alacritty, and Windows Terminal |
| Editors | Shared Vim and Neovim configuration, plus VS Code settings, keybindings, and extensions |
| Git, SSH, and GPG | Git aliases and global settings, hooks for this checkout, SSH client settings, and GPG agent settings |
| Coding agents | Copilot and Codex preferences, plus skills and plugins installed through Agent Package Manager |
| Packages | Arch packages through pacman, AUR packages through paru, custom Arch recipes, and Windows packages through winget |
| Arch desktop | Hyprland window management, the Quickshell bar and menus, mako notifications, and screen locking |
| Linux system | systemd services and cleanup timers, plus pacman and PAM settings under `/etc` on Arch |
| Windows | Developer Mode and per-user File Explorer and taskbar preferences |
| WSL | Networking on the Windows host, plus systemd startup and Windows PATH handling inside the Linux distribution |

## Guides and reference

- [Getting started](https://dotfiles.stuartneivandt.com/getting-started): Back up existing settings and preview your first setup.
- [Commands and usage](https://dotfiles.stuartneivandt.com/commands-and-usage): Select tasks, apply changes, and inspect the results.
- [Task reference](https://dotfiles.stuartneivandt.com/task-reference): Look up what a task changes, how to configure it, and what it leaves behind.
- [Contributing](https://dotfiles.stuartneivandt.com/contributing): Find where to make a change and which checks to run.
