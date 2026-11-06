---
title: Dotfiles launcher
description: A command that delegates to this checkout.
---

# Dotfiles launcher

The launcher makes `dotfiles` callable without typing the checkout's full path.
It is a small entrypoint into the shell or PowerShell wrapper, not another
installation of the Rust program. Binary download, source-build selection,
and argument forwarding remain the wrapper's responsibility.

| Task detail | Value |
|---|---|
| Selector | `launcher` |
| Commands | `update`, `remove` |
| Availability | Linux or Windows |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [dotfiles/wrapper.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/wrapper.rs) |

## What it does

Writes a small launcher script that calls this checkout's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

It repairs stale content; it does not copy the Rust binary. Moving the checkout can
invalidate the recorded path.

Launcher installation and command discovery are separate: [Shell PATH](shell-path.md)
adds the destination directory to the user's search path. If the launcher
exists but the shell cannot find it, inspect PATH before reinstalling or
moving the checkout. [Getting started](getting-started.md#5-verify-the-installation)
shows how to check the resulting installation.

## Configuration

The destination and launcher format are built in. There is no TOML setting. The launcher
records the selected checkout path, so keep that checkout in a permanent location.

## Preview and apply

Preview the launcher destination using your configured checkout:

```sh
dotfiles update --only launcher --dry-run
```

Check that the destination is this user's `.local/bin`. If an existing launcher
is reported as foreign or modified, back it up and move it aside rather than
overwriting it. After [applying the reviewed
change](task-reference.md#preview-and-apply), confirm the launcher points to the
intended checkout. If the shell cannot find it, check [Shell PATH](shell-path.md).

## Removal

Removal deletes the launcher only while its contents still match the managed version. A
modified launcher is preserved, and the PATH entry is left in place. Preview with
`dotfiles remove --only launcher --dry-run` before applying.
