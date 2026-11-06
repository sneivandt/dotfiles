---
title: AUR packages
description: Declare Arch User Repository packages, install missing AUR software through Paru, and understand build and upgrade risks.
---

# AUR packages

The [Arch User Repository](https://wiki.archlinux.org/title/Arch_User_Repository)
(AUR) distributes community-maintained build recipes, not the same reviewed
binary packages as Arch's official repositories. Paru builds and installs
selected recipes. In dotfiles, `aur = true` also selects locally managed
recipes, so the flag identifies the installation route rather than proving
the recipe came from the public AUR.

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
Use `aur = true` for packages handled by Paru. This illustrative example
selects the AUR build of VS Code; extend an existing category's package array
rather than replacing the full file:

```toml title="conf/packages.toml (example)"
[arch-desktop]
packages = [{ name = "visual-studio-code-bin", aur = true }]
```

This includes [dotfiles-managed recipes](custom-package-repository.md) as well as
packages from the remote AUR. Use Arch package names, not winget IDs. [System
packages](system-packages.md#configuration) describes the shared declaration format.

Review the recipe and its sources before installation. Package scripts are
code, and a declaration is not a trust or security review. This task installs
missing packages; it does not refresh every already-installed AUR application.
Use the [Arch upgrade workflow](package-management.md#arch-linux) for that.

## Preview and apply

Preview the missing packages selected for Paru:

```sh
dotfiles update --only aur-packages --dry-run
```

Check the package names and any unavailable custom-recipe reports. Resolve
those preparation failures instead of substituting a remote AUR package.
Before [applying the reviewed change](task-reference.md#preview-and-apply),
ensure Paru is healthy and review the recipes and their sources. A successful
preview of pacman's inventory does not validate package-supplied code.

## Removal

This task does not uninstall packages. Remove unwanted package declarations, then use
the native package manager deliberately. Review dependencies before uninstalling
anything.
