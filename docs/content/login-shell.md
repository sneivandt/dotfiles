---
title: Login shell
description: Set the Linux account's login shell to zsh, check shell availability and account state, and distinguish login settings from the current session.
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
-n usermod` when available without prompting, or `chsh` in an interactive run.
Non-interactive runs report unmet work when authentication would require a prompt.
A matching shell name
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

Preview whether the account needs to switch to zsh:

```sh
dotfiles update --only login-shell --dry-run
```

Resolve missing zsh or account-query errors before [applying the
change](task-reference.md#preview-and-apply). Apply may need account-management
permissions or authentication. Verify the selected shell in a new login
session; the current terminal and its `$SHELL` value are not the result of
this update.

## Removal

The account keeps its selected login shell after `dotfiles remove`. Choose another
installed shell through the operating system's account tools if you want to change it;
removing shell configuration files does not change the account setting.
