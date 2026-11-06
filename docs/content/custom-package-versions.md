---
title: Custom package versions
description: Release pins for managed Arch recipes.
---

# Custom package versions

| Task detail | Value |
|---|---|
| Selector | `custom-package-versions` |
| Commands | `update` |
| Availability | Arch/AUR support; work depends on managed recipes |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [packages/arch_recipes.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes.rs) |

## What it does

Finds stable upstream releases and refreshes the version pins for dotfiles-managed Arch
recipes. It runs on platforms with Arch/AUR support when managed recipes are selected.

A failed refresh is still a task failure. Existing pins can sometimes support a usable
snapshot, but they do not make the refresh successful. The [Custom package
repository](custom-package-repository.md) task prepares those snapshots; the [AUR
packages](aur-packages.md) task installs selected packages.

## Configuration

Each managed recipe has a `PKGBUILD` and release descriptor under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/). The descriptor
defines how to discover a stable upstream version. See [Arch package
recipes](package-management.md#arch-package-recipes) for the fields and a contributor
workflow. Selecting a recipe does not install it; add an `aur = true` package
declaration to request installation.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only custom-package-versions --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task has no remove action. Removing a recipe does not uninstall packages previously
built from it. Keep package removal separate from changes to release descriptors and
generated pins.
