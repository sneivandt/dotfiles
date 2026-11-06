---
title: System packages
description: Arch pacman packages and Windows winget IDs.
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

Installs missing non-AUR declarations using pacman on Linux or winget on Windows. It
does not install packages through apt or dnf. A missing package manager leaves the task
skipped; if the installed-package query fails, the task stops rather than assuming every
package is missing. After a failed batch transaction, it checks again to report packages
that did install; the transaction still counts as a failure.

**Installing missing Arch packages can upgrade the whole system.** The command is
`pacman -Syu --needed --noconfirm`. If all declared packages are already present, this
task does not run an upgrade solely because the command is named update.

Windows installs missing IDs through winget. Installer failures fail the task, even if
other packages succeed; installers can request their own UAC consent. See [Windows
package behavior](package-management.md#windows) for user-scope fallback and failure
handling, and [Windows elevation](windows-and-wsl.md#elevation) before an elevated
retry.

## Configuration

Edit
[conf/packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml).

For example, declare common command-line tools using each provider's IDs.
Extend the existing arrays; this is not a replacement for the complete file:

```toml title="conf/packages.toml (example)"
[arch]
packages = ["git", "ripgrep"]

[windows]
packages = ["Git.Git", "Microsoft.PowerShell"]
```

A string is shorthand for `{ name = "...", aur = false }`. Package names are
provider-specific, not portable aliases. Use Arch names in Arch categories and winget
IDs in Windows categories.

Keep platform-specific entries in the corresponding categories. A tool with
the same command name on both systems may need two different package IDs.
On Arch, use [AUR packages](aur-packages.md) for community or custom recipes;
on other Linux distributions, install prerequisites with the distribution's
own package manager rather than adding an Arch entry that cannot run there.

Regular Arch entries use pacman. `aur = true` sends an entry through Paru; that includes
configured [dotfiles-managed recipes](package-management.md#custom-arch-packages), not
just remote AUR recipes. The host's package-manager repositories and policies, including
`IgnorePkg`, remain in effect.

Presence is the main contract here, not keeping every installed version at the
latest release. For deliberate upgrades, provider-specific discovery, and
uninstalling packages no longer wanted, follow
[Package management](package-management.md). Removing a category is not an
instruction to uninstall the packages it once selected.

## Preview and apply

Preview the missing packages for the selected platform and profile:

```sh
dotfiles update --only packages --dry-run
```

Check provider-specific names and the proposed missing-package list. Resolve
package-manager or inventory-query errors before [applying the
change](task-reference.md#preview-and-apply). On Arch, be ready for the full
system upgrade that accompanies installation; on Windows, installers may
request UAC consent. An already-present package is not a promise that its
version is current.

## Removal

This task does not uninstall packages or reverse an Arch system upgrade. Remove unwanted
declarations, then review and uninstall packages through the native package manager. See
[Package removal](package-management.md#remove-a-package).
