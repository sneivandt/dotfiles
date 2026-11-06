---
title: Shell PATH
description: The user PATH entry for the launcher.
---

# Shell PATH

| Task detail | Value |
|---|---|
| Selector | `path` |
| Commands | `update` |
| Availability | Linux or Windows |
| [Requires success](task-reference.md#scheduling-model) | [Dotfiles launcher](dotfiles-launcher.md) |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [dotfiles/path.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/path.rs) |

## What it does

Requires the launcher. Adds `~/.local/bin` through a managed export in `~/.profile` on
Linux, or the user PATH on Windows. Open a new shell to use the persisted change.

Existing PATH entries are preserved. Remove leaves the PATH addition in place.

## Configuration

There is no TOML declaration. The task adds the launcher directory for the current user.
Application-specific PATH additions belong in your [shell
configuration](application-settings.md#shell-configuration).

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only path --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

The PATH addition remains after the launcher is removed. To retire it, edit the managed
export in `~/.profile` on Linux or the user PATH on Windows. Preserve unrelated entries
and open a new shell afterward.
