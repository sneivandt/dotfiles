---
title: Dotfiles launcher
description: A command that delegates to this checkout.
---

# Dotfiles launcher

| Task | Selection |
|---|---|
| Selector | `launcher` |
| Commands | `update`, `remove` |

## What it does

Writes a small launcher script that calls this checkout's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

It repairs stale content; it does not copy the Rust binary. Moving the checkout can
invalidate the recorded path.

## Configuration

The destination and launcher format are built in. There is no TOML setting. The launcher
records the selected checkout path, so keep that checkout in a permanent location.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only launcher --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Removal deletes the launcher only while its contents still match the managed version. A
modified launcher is preserved, and the PATH entry is left in place. Preview with
`dotfiles remove --only launcher --dry-run` before applying.
