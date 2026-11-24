---
title: Paru package manager
description: Install and check Paru, the Arch User Repository helper, and understand its bootstrap tools, configuration, and removal limits.
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

On pacman platforms, checks both that a `paru` package is registered and that
`/usr/bin/paru --version` succeeds, rebuilding an installed helper if a library
upgrade broke it. Failed package queries or interrupted health probes fail the task
instead of triggering a speculative rebuild.

The task waits for system packages without letting unrelated package failures block it.
Rebuilding requires `git`, `makepkg`, `sudo`, and a working `cargo --version` (an
unconfigured rustup proxy is not enough), and the rebuilt helper is re-verified before
dependent AUR installations proceed.

During Arch provisioning, this task probes the target chroot rather than the live ISO.
AUR installations always invoke the validated `/usr/bin/paru` rather than an arbitrary
copy on `PATH`.

Finding `paru` on `PATH` is not enough on its own, because Paru can stay installed yet
fail to start after a shared library upgrade. Checking both package registration and
execution catches that drift. If bootstrap is unmet, fix the reported build prerequisite
before retrying instead of substituting an unverified binary.

## Configuration

There is no TOML declaration for this task. It manages the Paru helper itself, while
[AUR packages](aur-packages.md) controls which packages Paru installs.

## Preview and apply

Preview whether Paru is healthy, missing, or needs rebuilding:

```sh
dotfiles update --only paru --dry-run
```

If installation or a rebuild is proposed, verify the build prerequisites above before
[applying the change](task-reference.md#preview-and-apply); preview does not build
the helper or test the toolchain. After applying, confirm the task reports a validated
helper before retrying AUR installs, and diagnose any failed health query rather than
assuming a reinstall will fix it.

## Removal

This task is not part of `dotfiles remove`. Changing profiles or declarations does not
uninstall Paru; see [removal scope](task-reference.md#remove-tasks) before planning a
cleanup.
