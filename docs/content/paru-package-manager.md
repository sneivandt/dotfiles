---
title: Paru package manager
description: Installation and health checks for the AUR helper.
---

# Paru package manager

[Paru](https://github.com/Morganamilo/paru) is the AUR helper used by this setup.
It coordinates recipe retrieval, building, and installation with Arch's
packaging tools. This task ensures the helper itself is usable; the
[AUR packages](aur-packages.md) task uses it to install selected applications.

| Task detail | Value |
|---|---|
| Selector | `paru` |
| Commands | `update` |
| Availability | Pacman platform |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [packages/install.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/install.rs) |

## What it does

On pacman platforms, checks both a registered `paru` package and a working
`/usr/bin/paru --version`. It can rebuild an installed helper broken by a library
upgrade. Failed package queries or interrupted health probes fail rather than trigger a
speculative rebuild.

It waits for system packages without propagating unrelated failures. Rebuilding needs
`git`, `makepkg`, `sudo` and a working `cargo --version`; an unconfigured rustup proxy
is not enough. The rebuilt helper is checked again before dependent AUR installation.

Arch provisioning probes the target chroot, not the live ISO. AUR work uses the
validated `/usr/bin/paru`, not an arbitrary PATH copy.

An executable appearing on PATH is not enough: Paru can remain installed but
fail to start after the system libraries it links against change. That is why
the task checks both package registration and execution. If bootstrap is
unmet, resolve the reported build prerequisite before retrying instead of
substituting an unverified helper binary.

## Configuration

There is no dedicated TOML declaration. The task manages the Paru helper itself; [AUR
packages](aur-packages.md) selects the packages installed through it.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only paru --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task is not part of `dotfiles remove`. Removing a declaration or changing profiles
does not undo changes already applied. See [removal
scope](task-reference.md#remove-tasks) before planning a cleanup.
