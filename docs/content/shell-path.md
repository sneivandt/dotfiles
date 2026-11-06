---
title: Shell PATH
description: The user PATH entry for the launcher.
---

# Shell PATH

PATH is the list of directories a shell searches for commands. This task
persists the directory containing the [Dotfiles launcher](dotfiles-launcher.md)
so a future shell can find it. It does not install commands, choose the login
shell, or repair every application's PATH configuration.

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

Open a new terminal to use the updated PATH. Existing processes keep the
environment they inherited at startup. On Linux, loading also depends on the
shell's startup files. The [Bash invocation reference](https://www.gnu.org/software/bash/manual/html_node/Invoking-Bash.html)
explains login and interactive shell modes; the managed
[shell configuration](application-settings.md#shell-configuration) supplies
the repository-specific startup behavior.

## Configuration

There is no TOML declaration. The task adds the launcher directory for the current user.
Application-specific PATH additions belong in your [shell
configuration](application-settings.md#shell-configuration).

## Preview and apply

Preview the persisted launcher-directory addition:

```sh
dotfiles update --only path --dry-run
```

The intended addition is your home directory's `.local/bin`, not the checkout
or its `bin/` directory. Resolve any profile or saved-PATH read error before
[applying the change](task-reference.md#preview-and-apply). Open a new shell and
confirm it finds the launcher; retrying this task does not refresh an existing
terminal's environment.

## Removal

The PATH addition remains after the launcher is removed. To retire it, edit the managed
export in `~/.profile` on Linux or the user PATH on Windows. Preserve unrelated entries
and open a new shell afterward.
