---
title: Dotfiles
description: A personal dotfiles project turned configuration engine for Arch Linux and Windows, and a place to explore agentic engineering.
---

# Dotfiles

This repository keeps my Arch Linux and Windows shell, editor, desktop, and
system settings together. [Getting started](getting-started.md) walks through
reviewing the configuration and previewing changes before applying them.

This is my personal setup, **not a general-purpose installer**. Read the
configuration and preview the changes before using it on your machine.

Its Rust CLI checks what is already in place and applies the selected work.
The `base` profile covers core shell tools and settings; `desktop` adds
graphical applications and services. Linux package management and the desktop
setup target Arch Linux.

I started this as a dotfiles repo, before coding agents were part of my
workflow. It's grown into a configuration engine that does more than I
strictly need, but gives me room to explore how agents can help build and
maintain software.

Already using it? [Commands and usage](commands-and-usage.md) covers everyday
tasks, and [Configuration reference](configuration-reference.md) shows how to
make the setup your own.
