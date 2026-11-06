---
title: About this project
titleTemplate: false
description: Stuart Neivandt's dotfiles and Rust configuration engine for Arch Linux and Windows, with notes on agentic engineering.
outline: false
prev: false
next: false
head:
  - - script
    - type: application/ld+json
    - '{"@context":"https://schema.org","@type":"WebSite","name":"Dotfiles","url":"https://dotfiles.stuartneivandt.com/","creator":{"@type":"Person","name":"Stuart Neivandt","url":"https://stuartneivandt.com/"}}'
---

# About this project

Hi, I'm [Stuart Neivandt](https://stuartneivandt.com/). I started this as a
simple dotfiles repo back in 2014. Over time, it's grown into a configuration
engine for my Arch Linux and Windows machines, and a place to experiment with
how coding agents can help build and maintain software, even when that means
building more than I strictly need.

This is my own setup, not a general-purpose tool.

Start with **[Getting started](getting-started.md)** to review the configuration
and preview changes before applying anything to your machine.

The Rust CLI checks what's already in place and applies the work selected for
each machine. Profiles let me share core settings while choosing packages,
applications, and services for different machines. Linux package management
and graphical setup target Arch Linux.

Already using it? [Commands and usage](commands-and-usage.md) covers everyday
tasks. Use the [CLI reference](cli-reference.md), the
[task catalog](task-reference.md), and [Configuration basics](configuration-basics.md)
to understand commands, available tasks, and how to edit declarations.
To change the project itself, start with [Contributing](contributing.md).
