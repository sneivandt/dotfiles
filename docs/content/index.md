---
title: About this project
description: A personal dotfiles project turned configuration engine for Arch Linux and Windows, and a place to explore agentic engineering.
---

# About this project

I started this as a dotfiles project before AI-assisted coding entered my
workflow. It has grown into a knowingly over-engineered, feature-rich
configuration management engine for my Arch Linux and Windows machines. I also
use it to explore agentic engineering and how coding agents can help build and
maintain software.

The repository keeps my shell, editor, desktop, and system settings together.
Its Rust CLI checks what is already in place, previews the selected work, and
applies changes. The `base` profile covers core shell tools and settings;
`desktop` adds graphical applications and services. Linux package management
and the desktop setup target Arch Linux.

This is my personal setup, **not a general-purpose installer**. Read the
configuration and preview the changes before using it on your machine.

If you're curious about trying it, [Getting started](getting-started.md) takes
you through the first review and preview. Already using it?
[Commands and usage](commands-and-usage.md) covers everyday tasks, and
[Configuration reference](configuration-reference.md) shows how to make the
setup your own.