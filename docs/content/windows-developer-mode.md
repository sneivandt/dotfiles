---
title: Windows Developer Mode
description: Machine policy for Windows development features.
---

# Windows Developer Mode

| Task | Selection |
|---|---|
| Selector | `developer-mode` |
| Commands | `update` |

## What it does

Enables the machine-level Developer Mode policy when unset, requiring Administrator
rights. The Home symlinks task waits for this task, then checks its own needs.
Unavailable Developer Mode does not block links that are already correct or directory
junctions that can be created without it.

Pending file symlinks may still need their own elevation. The CLI normally stays
unelevated and delegates only affected tasks. See [Windows
elevation](windows-and-wsl.md#elevation).

## Configuration

This task has no TOML settings. It manages the Windows Developer Mode policy as part of
Windows setup. See [Windows and WSL](windows-and-wsl.md#developer-mode-and-symlinks) for
symlink requirements.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only developer-mode --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Developer Mode remains enabled after `dotfiles remove`. If you want to disable it, use
Windows settings and first consider applications and file symlinks that depend on it.
