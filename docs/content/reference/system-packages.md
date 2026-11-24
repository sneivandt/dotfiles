---
title: System packages
description: Declare and install missing Arch Linux pacman packages or Windows winget IDs, preview provider actions, and understand upgrade and removal behavior.
---

# System packages

This task asks the platform's package manager to install declared applications
that are missing. Arch uses [pacman](https://man.archlinux.org/man/pacman.8.en);
Windows uses [WinGet](https://learn.microsoft.com/en-us/windows/package-manager/winget/).
Their package names and installer policies differ. Dotfiles shares selection
rules, not a universal package namespace.

| Task detail | Value |
|---|---|
| Selector | `packages` |
| Commands | `update` |
| Availability | Configured non-AUR packages |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [packages/install.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/install.rs) |

## What it does

Installs missing non-AUR packages using `pacman` on Arch Linux or `winget` on
Windows. Other Linux distributions are unsupported, even if a `pacman` binary happens
to be on `PATH`, and missing or unsupported package managers report unmet work.
If the inventory query fails, the task stops rather than assuming every package is
missing. After a failed Arch batch transaction, it re-queries installed packages so it
can report any that succeeded while still marking the transaction failed.

**Installing missing Arch packages can upgrade the whole system.** Dotfiles runs
`pacman -Syu --needed --noconfirm`. If every declared package is already installed,
this task does nothing and does not trigger a system upgrade.

Windows installs pass `--no-upgrade` to `winget` so an application installed since the
inventory check stays on its current version. Any installer failure fails the task even
when other packages succeed, and installers may show their own UAC prompts. See
[Package management](../guides/package-management.md#windows) for user-scope fallback
and failure handling, and [Windows elevation](../guides/windows-and-wsl.md#elevation)
before retrying elevated.

## Configuration

Edit
[conf/packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml).

For example, declare common command-line tools using each provider's native IDs
(extend the existing arrays rather than replacing the whole file):

```toml title="conf/packages.toml (example)"
[arch]
packages = ["git", "ripgrep"]

[windows]
packages = ["Git.Git", "Microsoft.PowerShell"]
```

A plain string is shorthand for `{ name = "...", aur = false }`. Because package names
are provider-specific rather than portable aliases, put Arch names in Arch categories
and winget IDs in Windows categories.

A tool with the same command name on both platforms often needs two different package
IDs. On Arch, use [AUR packages](aur-packages.md) for community or custom recipes; on
other Linux distributions, install prerequisites through the distro's own package
manager instead of adding an Arch entry that will not run there.

Regular Arch entries install with `pacman`, while `aur = true` routes an entry through
Paru (including [custom Arch recipes](../guides/package-management.md#custom-arch-packages),
not just remote AUR packages). The host's repositories and package-manager policies,
including `IgnorePkg`, remain in effect.

This task ensures packages are installed rather than keeping every package at its
latest release. For deliberate upgrades, package search, and uninstalls, see
[Package management](../guides/package-management.md). Removing a category never
uninstalls packages that it previously selected.

## Preview and apply

Preview the missing packages for your platform and profile:

```sh
dotfiles update --only packages --dry-run
```

Verify the provider-specific names and missing-package list, and resolve any
package-manager or inventory errors before [applying the
change](task-reference.md#preview-and-apply). On Arch, keep in mind that installing
missing packages also performs a full system upgrade; on Windows, installers may
prompt for UAC consent. Already-installed packages are skipped without checking for
newer versions.

## Removal

This task never uninstalls packages or rolls back an Arch system upgrade. Remove
unwanted declarations first, then uninstall the packages with the native package
manager as described in [Package removal](../guides/package-management.md#remove-a-package).
