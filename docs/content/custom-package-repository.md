---
title: Custom package repository
description: Disposable snapshots of managed Arch recipes.
---

# Custom package repository

| Task | Selection |
|---|---|
| Selector | `custom-package-repository` |
| Commands | `update` |

## What it does

Prepares disposable Git snapshots of dotfiles-managed Arch recipes and configures Paru
to use them. It runs on platforms with Arch/AUR support and waits for version discovery
before preparing snapshots.

A recipe failure does not block unrelated AUR packages. A missing selected custom
package is counted as a failure and excluded from installation when its snapshot is
unavailable. Descriptor names establish dotfiles ownership, so a failed local recipe
must not silently fall back to the remote AUR.

Paru's `CleanAfter` cleans its cache, not this checkout. See [Custom Arch
packages](package-management.md#custom-arch-packages) for the available recipes.

## Configuration

Recipes live under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/). Version
descriptors identify which recipes dotfiles owns. Package installation is requested
separately through `aur = true` entries in [AUR
packages](aur-packages.md#configuration). [Package
management](package-management.md#arch-package-recipes) explains the snapshot format and
Paru configuration.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only custom-package-repository --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task is not selected by `dotfiles remove`. Retiring a recipe and uninstalling a
package built from it are separate decisions. Review the [recipe and cache
workflow](package-management.md#arch-package-recipes) before changing either.
