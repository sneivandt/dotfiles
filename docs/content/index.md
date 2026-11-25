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
  tagline: My personal Arch Linux and Windows setup, managed with a Rust CLI.
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
---

<section class="home-about">

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
your own machine, start with [Getting started](guides/getting-started.md) to
review the defaults, back up existing files, and preview what would change
before applying anything.

</section>
