---
layout: home
title: About this project
titleTemplate: false
description: How Stuart Neivandt manages shell, editor, package, and Hyprland desktop settings across Arch Linux and Windows with a Rust CLI.
head:
  - - script
    - type: application/ld+json
    - '{"@context":"https://schema.org","@type":"WebSite","name":"Dotfiles","url":"https://dotfiles.stuartneivandt.com/","creator":{"@type":"Person","name":"Stuart Neivandt","url":"https://stuartneivandt.com/"}}'
hero:
  name: Dotfiles
  tagline: My personal shell, editor, package, and Hyprland setup kept in one checkout, with previews before anything changes.
  image:
    src: /favicon.svg
    alt: Dotfiles geometric tilde, slash, and dot emblem
  actions:
    - theme: brand
      text: Getting started
      link: /getting-started
    - theme: alt
      text: Commands
      link: /commands-and-usage
    - theme: alt
      text: GitHub
      link: https://github.com/sneivandt/dotfiles
features:
  - title: Shells and editors
    details: Zsh, Bash, PowerShell, tmux, Alacritty, Vim, Neovim, and VS Code settings kept live through symlinks.
    link: /application-settings
    linkText: Application settings
  - title: Arch Linux desktop
    details: Hyprland window management, a Quickshell bar and menus, mako notifications, and systemd cleanup timers.
    link: /arch-desktop
    linkText: Arch desktop
  - title: Windows and WSL
    details: Winget packages, Developer Mode, per-user Explorer and taskbar preferences, and WSL integration.
    link: /windows-and-wsl
    linkText: Windows and WSL
  - title: Coding agents
    details: Copilot and Codex preferences, plus reusable skills and plugins installed through Agent Package Manager.
    link: /agent-configuration
    linkText: Agent configuration
  - title: Profiles and dry-run previews
    details: Share terminal basics everywhere with base, add workstation tools with desktop, and preview changes first.
    link: /profiles-and-categories
    linkText: Profiles and categories
  - title: Rust CLI engine
    details: Dependency-ordered tasks, private overlays, configuration validation, and self-updating release binaries.
    link: /architecture
    linkText: Architecture
---

<section class="home-showcase">
<figure class="home-preview">
  <img src="/assets/terminal-screenshot.svg" width="760" height="548" alt="Animated terminal session: one dotfiles update groups CLI upgrade and repository sync notices beneath its header, then links configuration, refreshes shell completions, and installs missing packages">
  <figcaption>A single <code>dotfiles update</code> upgrades the CLI, syncs the checkout, links configs, and installs packages.</figcaption>
</figure>

<div class="home-about">

## About this project

Hi, I'm [Stuart Neivandt](https://stuartneivandt.com/). I've kept my dotfiles
here since 2014. Over the years they grew into a Rust CLI that keeps my shell,
editor, and package setup in sync across Arch Linux and Windows, and sets up
the Hyprland desktop on my workstation. It's also where I experiment with
coding agents, sometimes building more than I strictly need.

This is a personal setup, not a general-purpose tool. If I want a different
shell alias, I edit the config file instead of adding a CLI flag. Profiles let
me share the terminal basics everywhere without pulling a full desktop onto
every machine.

If you're here to borrow ideas, explore the guides above or browse the
[repository](https://github.com/sneivandt/dotfiles). If you want to try it on
your own machine, start with [Getting started](getting-started.md) to review
the defaults, back up existing files, and preview what would change before
applying anything.

</div>
</section>
