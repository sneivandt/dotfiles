---
title: Dotfiles launcher
description: Install the dotfiles command launcher on Linux or Windows, preserve its checkout path, and distinguish it from the CLI binary and PATH entry.
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

Writes a small launcher script that invokes this checkout's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

The task repairs stale launcher content without copying the Rust binary. Because the
launcher records the checkout path, moving the checkout later can leave it pointing
to a missing directory.

Launcher installation and command discovery are separate tasks: [Shell PATH](shell-path.md)
adds the destination directory to your user search path. Both the destination
and launcher format are built in, with no TOML setting; keep the checkout in a
permanent location so the recorded path stays valid. If the launcher exists on
disk but your shell cannot find `dotfiles`, check `PATH` before reinstalling or
moving the checkout. [Getting started](../guides/getting-started.md#5-verify-the-installation)
shows how to verify the installed command.

## Preview and apply

Preview the launcher destination using your configured checkout:

```sh
dotfiles update --only launcher --dry-run
```

Confirm that the destination is this user's `.local/bin`. If an existing launcher
is reported as foreign or modified, back it up and move it aside rather than
overwriting it. After applying, verify that the launcher points to the intended
checkout, and check [Shell PATH](shell-path.md) if the shell still cannot find
`dotfiles`.

## Removal

Removal deletes the launcher only while its contents still match the managed version. Any
modified launcher is preserved, and the `PATH` entry is left in place. Preview with
`dotfiles remove --only launcher --dry-run` before applying.
