---
title: Introduction
description: Stuart Neivandt's dotfiles and Rust configuration engine for Arch Linux and Windows, with notes on agentic engineering.
head:
  - tag: title
    content: Dotfiles by Stuart Neivandt
  - tag: link
    attrs:
      rel: canonical
      href: https://dotfiles.stuartneivandt.com/
  - tag: meta
    attrs:
      property: og:url
      content: https://dotfiles.stuartneivandt.com/
  - tag: script
    attrs:
      type: application/ld+json
    content: |
      {
        "@context": "https://schema.org",
        "@type": "WebSite",
        "name": "Dotfiles",
        "url": "https://dotfiles.stuartneivandt.com/",
        "description": "Stuart Neivandt's dotfiles and Rust configuration engine for Arch Linux and Windows.",
        "creator": {
          "@type": "Person",
          "name": "Stuart Neivandt",
          "url": "https://github.com/sneivandt"
        }
      }
tableOfContents: false
---

# Introduction

This repository keeps my Arch Linux and Windows shell, editor, desktop, and
system settings together. [Getting started](getting-started.md) walks through
reviewing the configuration and previewing changes before applying them.

This is my personal setup, **not a general-purpose installer**. Read the
configuration and preview the changes before using it on your machine.

Its Rust CLI checks what is already in place and applies the selected work.
Profiles let the configuration adapt to different machines, from core settings
to packages, applications, and services. Linux package management and the
graphical setup target Arch Linux.

I'm [Stuart Neivandt](https://github.com/sneivandt), and I started this as a
dotfiles repo before coding agents were part of my workflow. It's grown into
a configuration engine that does more than I strictly need, but gives me room
to explore how agents can help build and maintain software.

Already using it? [Commands and usage](commands-and-usage.md) covers everyday
tasks, and [Configuration reference](configuration-reference.md) shows how to
make the setup your own.
