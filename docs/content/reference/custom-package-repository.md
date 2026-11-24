---
title: Custom package repository
description: Prepare disposable PKGBUILD snapshots for managed Arch packages, inspect recipe metadata, and replace Paru local repository preferences.
---

# Custom package repository

Paru can build from local Git recipe repositories alongside the public AUR.
This task prepares disposable recipe snapshots from dotfiles-managed Arch
recipes and updates Paru's config to point to them. ("Repository" here means a
disposable recipe snapshot, not a binary pacman repo or your source checkout.)

| Task detail | Value |
|---|---|
| Selector | `custom-package-repository` |
| Commands | `update` |
| Availability | Arch/AUR support |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Custom package versions](custom-package-versions.md) |
| Implementation | [packages/arch_recipes.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes.rs) |

## What it does

Prepares disposable Git snapshots for every managed Arch recipe under the main
checkout's `packages/` directory (even recipes not selected by your current
profile) after `custom-package-versions` finishes, then writes Paru's local
repository configuration.

A broken custom recipe does not block unrelated AUR packages; if a selected
custom package's snapshot fails, that package is recorded as failed and never
silently falls back to the public AUR. Paru's `CleanAfter` cleans Paru's cache,
not your checkout. See
[Custom Arch packages](../guides/package-management.md#custom-arch-packages)
for the tracked recipes.

Preparation and installation are separate: applying this task executes PKGBUILD
metadata generation and writes snapshots plus `paru.conf`, without installing
the packages themselves. `--dry-run` neither executes recipes nor writes files.

::: warning Review recipes before preparing packages
PKGBUILDs execute shell code during metadata generation, even without building
or installing. Temporary copies are not a sandbox. Review sources before
applying preparation; Paru skips normal recipe review for these local repositories.
:::

## Configuration

Recipes live under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/), with
release descriptors marking ownership (see
[Custom package versions](custom-package-versions.md#configuration)) and
`aur = true` entries in [AUR packages](aur-packages.md#configuration) selecting
what to install.

Snapshots include `PKGBUILD`, generated `version.sh`, optional
`package.install`, `LICENSE`, `NOTICE`, and generated `.SRCINFO` (additional
local files require updating
[snapshots.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes/snapshots.rs)
first). Always edit tracked sources rather than cached snapshots or generated
Paru sections.

Editable Paru defaults live in
[symlinks/config/paru/paru.conf.base](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/paru/paru.conf.base).
The task resolves whichever source `.config/paru/paru.conf.base` points to in
your active symlink config (including overlay overrides or renamed sources) and
combines it with the build-cache path and generated repository sections. A
missing declaration or source fails loudly instead of falling back to the
public base.

::: warning Existing Paru configuration is replaced
Applying this task replaces the whole destination document, not just its
generated repository sections. Unrelated preferences in that document are not
merged or backed up. Review and back up an existing configuration before
applying, and put durable preferences in the tracked base file.
If the destination changes during preparation, Dotfiles reports a conflict.
Review the changed file and transfer any preferences you want to keep to
the base before retrying. A retry plans against the new contents; it does
not merge them.
:::

The output file is `PARU_CONF` if set, otherwise
`$XDG_CONFIG_HOME/paru/paru.conf`, otherwise `~/.config/paru/paru.conf` (empty
or relative overrides are rejected; use the same environment for `dotfiles` and
Paru). See the [PKGBUILD manual](https://man.archlinux.org/man/PKGBUILD.5.en)
and [Paru documentation](https://github.com/Morganamilo/paru) for upstream
format details, and always investigate cache ownership conflicts rather than
deleting unfamiliar directories or pointing Paru at your source checkout.

## Preview and apply

Preview snapshot and Paru config updates:

```sh
dotfiles update --only custom-package-repository --dry-run
```

Changed snapshots appear as `refresh <recipe>` (for example `refresh
openai-codex`), and Paru config updates appear as `update paru configuration`
(add `--verbose` to see snapshot and output paths). Resolve any ownership
conflicts, back up existing Paru preferences, and review every recipe before
[applying](task-reference.md#preview-and-apply), since metadata generation runs
PKGBUILD code for all tracked recipes.

## Removal

`dotfiles remove` does not touch this task. Retiring a recipe and uninstalling
packages built from it are separate steps; investigate cache ownership conflicts
before touching cache directories rather than deleting unfamiliar folders or
falling back to the remote AUR.
