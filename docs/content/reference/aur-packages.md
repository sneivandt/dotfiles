---
title: AUR packages
description: Declare Arch User Repository packages, install missing AUR software through Paru, and understand build and upgrade risks.
---

# AUR packages

The [Arch User Repository](https://wiki.archlinux.org/title/Arch_User_Repository)
(AUR) hosts community build recipes rather than official Arch binary packages,
and Paru builds and installs them. In `dotfiles`, `aur = true` also routes
locally managed recipes through Paru, so the flag selects the installer path
rather than proving a package came from the public AUR.

| Task detail | Value |
|---|---|
| Selector | `aur-packages` |
| Commands | `update` |
| Availability | Arch/AUR support with AUR entries |
| [Requires success](task-reference.md#scheduling-model) | [Paru package manager](paru-package-manager.md) |
| [Runs after](task-reference.md#scheduling-model) | [Custom package repository](custom-package-repository.md) |
| Implementation | [packages/install.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/install.rs) |

## What it does

Installs missing `aur = true` entries through Paru once `paru` succeeds,
waiting for `custom-package-repository` without letting one broken custom
recipe block unrelated AUR packages.

`--dry-run` queries pacman's installed database without requiring Paru to be
installed yet (query errors still fail the preview). Applying this task builds
and executes package-supplied recipe code.

## Configuration

Declare AUR packages in
[conf/packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml)
with `aur = true` (extending an existing category's array rather than replacing
the file):

```toml title="conf/packages.toml (example)"
[arch-desktop]
packages = [{ name = "visual-studio-code-bin", aur = true }]
```

Use Arch package names, not winget IDs; see
[System packages](system-packages.md#configuration) for the shared schema and
[Custom package repository](custom-package-repository.md) for locally managed
recipes.

Review recipes and upstream sources before installing: PKGBUILDs execute shell
code, and adding a name to `packages.toml` isn't a security review. This task
only installs missing packages; use the
[Arch upgrade workflow](../guides/package-management.md#arch-linux) to upgrade
existing AUR installs.

## Preview and apply

Preview missing AUR packages selected for Paru:

```sh
dotfiles update --only aur-packages --dry-run
```

Verify the package names and fix any failed custom-recipe preparations rather
than falling back to a remote AUR package. Confirm Paru is healthy and review
recipes before [applying](task-reference.md#preview-and-apply); a clean pacman
inventory preview does not inspect recipe code.

## Removal

This task never uninstalls packages. Remove the entry from `conf/packages.toml`,
review dependencies, and uninstall deliberately with your package manager.
