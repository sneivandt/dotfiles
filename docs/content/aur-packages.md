---
title: AUR packages
description: Arch packages installed through Paru.
---

# AUR packages

| Task detail | Value |
|---|---|
| Selector | `aur-packages` |
| Commands | `update` |
| Availability | Arch/AUR support with AUR entries |
| [Requires success](task-reference.md#scheduling-model) | [Paru package manager](paru-package-manager.md) |
| [Runs after](task-reference.md#scheduling-model) | [Custom package repository](custom-package-repository.md) |
| Implementation | [packages/install.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/install.rs) |

## What it does

Installs missing `aur = true` entries after successful Paru bootstrap. It also waits for
custom package repository preparation without making recipe failures block every
unrelated AUR package.

Preview queries pacman's installed-package database without requiring the helper that
bootstrap has only planned to install. Query errors still fail the preview. Applying
this task builds and executes package-supplied code.

## Configuration

Edit
[conf/packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml).
Use `aur = true` in
[conf/packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml):

```toml title="conf/packages.toml (excerpt)"
[arch]
packages = [{ name = "powershell-dotfiles", aur = true }]
```

This includes [dotfiles-managed recipes](custom-package-repository.md) as well as
packages from the remote AUR. Use Arch package names, not winget IDs. [System
packages](system-packages.md#configuration) describes the shared declaration format.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only aur-packages --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task does not uninstall packages. Remove unwanted package declarations, then use
the native package manager deliberately. Review dependencies before uninstalling
anything.
