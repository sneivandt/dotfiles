---
title: System packages
description: Arch pacman packages and Windows winget IDs.
---

# System packages

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

```toml title="conf/packages.toml (excerpt)"
[arch]
packages = [
  "git",
  "ripgrep",
  { name = "powershell-dotfiles", aur = true },
]

[windows]
packages = ["Git.Git", "Microsoft.PowerShell"]
```

A string is shorthand for `{ name = "...", aur = false }`. Package names are
provider-specific, not portable aliases. Use Arch names in Arch categories and winget
IDs in Windows categories.

Regular Arch entries use pacman. `aur = true` sends an entry through Paru; that includes
configured [dotfiles-managed recipes](package-management.md#custom-arch-packages), not
just remote AUR recipes. The host's package-manager repositories and policies, including
`IgnorePkg`, remain in effect.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only packages --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task does not uninstall packages or reverse an Arch system upgrade. Remove unwanted
declarations, then review and uninstall packages through the native package manager. See
[Package removal](package-management.md#remove-a-package).
