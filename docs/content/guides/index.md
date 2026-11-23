---
title: About this project
titleTemplate: false
description: How Stuart Neivandt uses dotfiles and a Rust CLI to manage shell, editor, and desktop settings on Arch Linux and Windows.
outline: false
prev: false
next: false
head:
  - - script
    - type: application/ld+json
    - '{"@context":"https://schema.org","@type":"WebSite","name":"Dotfiles","url":"https://dotfiles.stuartneivandt.com/","creator":{"@type":"Person","name":"Stuart Neivandt","url":"https://stuartneivandt.com/"}}'
---

# About this project

Hi, I'm [Stuart Neivandt](https://stuartneivandt.com/). I started keeping my
dotfiles here in 2014. Today I use a Rust CLI to keep my shell and editor
settings in sync across Arch Linux and Windows, and to set up the Hyprland
desktop on my Arch workstation. The repo is also where I experiment with
coding agents, sometimes building more than I strictly need.

This is my own setup, not a general-purpose tool. If I want a different shell
alias, I edit the configuration file rather than add a CLI option. Profiles
let me share the basics without installing a desktop on every machine.

**[Getting started](getting-started.md)** walks you through reviewing the
settings, backing up existing files, and previewing what would change on
your machine.
