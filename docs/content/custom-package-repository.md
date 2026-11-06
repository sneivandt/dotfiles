---
title: Custom package repository
description: Prepare disposable PKGBUILD snapshots for managed Arch packages, inspect recipe metadata, and replace Paru local repository preferences.
---

# Custom package repository

Paru can build from local Git recipe repositories as well as the public AUR.
This task prepares that local input from reviewed dotfiles recipes and their
version pins. Here, "repository" means a disposable recipe snapshot, not a
published pacman binary repository and not the source checkout itself.

| Task detail | Value |
|---|---|
| Selector | `custom-package-repository` |
| Commands | `update` |
| Availability | Arch/AUR support |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Custom package versions](custom-package-versions.md) |
| Implementation | [packages/arch_recipes.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes.rs) |

## What it does

Prepares disposable Git snapshots of dotfiles-managed Arch recipes and configures Paru
to use them. It runs on platforms with Arch/AUR support and waits for version discovery
before preparing snapshots. Discovery and preparation cover every managed
recipe under the main checkout's `packages/`, even if the current profile does
not request its installation.

A recipe failure does not block unrelated AUR packages. A missing selected custom
package is counted as a failure and excluded from installation when its snapshot is
unavailable. Descriptor names establish dotfiles ownership, so a failed local recipe
must not silently fall back to the remote AUR.

Paru's `CleanAfter` cleans its cache, not this checkout. See [Custom Arch
packages](package-management.md#custom-arch-packages) for the available recipes.

Preparation and installation have different effects. Applying preparation
can execute PKGBUILD code to obtain metadata and writes snapshots and Paru
configuration; it does not install the application. Preview does not execute
recipes or write those outputs. Review the
[recipe safety guidance](arch-package-recipes.md#prepare-snapshots) first.

## Configuration

Recipes live under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/). Version
descriptors identify which recipes dotfiles owns. Package installation is requested
separately through `aur = true` entries in [AUR
packages](aur-packages.md#configuration). [Arch package
recipes](arch-package-recipes.md) explains the snapshot format and
Paru configuration.

The editable Paru defaults are
[symlinks/config/paru/paru.conf.base](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/paru/paru.conf.base).
The task generates a complete configuration from that file, the build-cache
location, and the prepared repository sections.

::: warning Existing Paru configuration is replaced
Applying this task replaces the whole destination document, not just its
generated repository sections. Unrelated preferences in that document are not
merged or backed up. Review and back up an existing configuration before
applying, and put durable preferences in the tracked base file.
:::

The destination is `PARU_CONF` when set, otherwise
`$XDG_CONFIG_HOME/paru/paru.conf` when that variable is set, otherwise
`~/.config/paru/paru.conf`. Set overrides to absolute paths; empty or relative
values are rejected. Use the same environment when running dotfiles and Paru.

For the underlying tools, use the
[PKGBUILD format reference](https://man.archlinux.org/man/PKGBUILD.5.en) and
[Paru documentation](https://github.com/Morganamilo/paru). Their formats do
not replace dotfiles' ownership checks. When preparation rejects a cache
directory, investigate its ownership instead of deleting unrelated content
or pointing Paru directly at the source checkout.

## Preview and apply

Preview snapshot and configuration changes with your configured launcher:

```sh
dotfiles update --only custom-package-repository --dry-run
```

Check the proposed snapshot paths and Paru configuration destination. Resolve
ownership conflicts before retrying; do not delete an unfamiliar cache.
Back up existing Paru preferences and review every recipe before [applying
the change](task-reference.md#preview-and-apply), which can execute PKGBUILD
code even for recipes whose applications are not selected for installation.

## Removal

This task is not selected by `dotfiles remove`. Retiring a recipe and uninstalling a
package built from it are separate decisions. Review the [recipe and cache
workflow](arch-package-recipes.md) before changing either.
