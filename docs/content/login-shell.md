---
title: Login shell
description: The account login shell on Linux.
---

# Login shell

The login shell is an account setting that chooses the shell for future login
sessions. It is distinct from a terminal application's command and from the
shell process already running. Dotfiles chooses zsh on Linux; its prompt,
plugins, and startup files are managed separately through
[application settings](application-settings.md#shell-configuration).

| Task detail | Value |
|---|---|
| Selector | `login-shell` |
| Commands | `update` |
| Availability | Linux, outside CI |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [shell/login_shell.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/shell/login_shell.rs) |

## What it does

On Linux outside CI, sets the account login shell to zsh. This choice is set by the
task, not configurable in TOML. It waits for packages and reports missing zsh as unmet
work.

State comes from the account database, not `$SHELL`. Apply uses `usermod` as root, `sudo
-n usermod` when available without prompting, otherwise `chsh`. A matching shell name
with a missing/unusable executable still needs repair. Your currently running terminal
shell is not replaced.

After applying, a new login session is the meaningful check, not the current
value of `$SHELL`. If zsh cannot be found, resolve its package installation
before retrying rather than pointing the account at an unavailable executable.
The [chsh manual](https://man.archlinux.org/man/chsh.1.en) explains the
underlying account operation and its permission restrictions; dotfiles may
use `usermod` instead as described above.

## Configuration

The desired shell is zsh and is defined by the task. There is no TOML key for choosing a
different shell. Make zsh available through [System packages](system-packages.md) before
running this task.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only login-shell --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

The account keeps its selected login shell after `dotfiles remove`. Choose another
installed shell through the operating system's account tools if you want to change it;
removing shell configuration files does not change the account setting.
