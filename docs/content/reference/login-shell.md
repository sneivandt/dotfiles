---
title: Login shell
description: Set the Linux account's login shell to zsh, check shell availability and account state, and distinguish login settings from the current session.
---

# Login shell

The login shell is an account setting that chooses the shell for future login
sessions. It is distinct from a terminal application's command and from the
shell process already running. Dotfiles chooses zsh on Linux; its prompt,
plugins, and startup files are managed separately through
[application settings](../guides/application-settings.md#shell-configuration).

| Task detail | Value |
|---|---|
| Selector | `login-shell` |
| Commands | `update` |
| Availability | Linux, outside CI |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [shell/login_shell.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/shell/login_shell.rs) |

## What it does

On Linux outside CI, sets the account login shell to zsh. The shell choice is built
into the task rather than configured in TOML. It waits for system packages and reports
a missing zsh executable as unmet work.

State is read from the account database rather than `$SHELL`. Apply runs `usermod`
directly as root, `sudo -n usermod` when passwordless sudo is available, or `chsh`
during an interactive run. Non-interactive runs report unmet work if authentication
would require a prompt. The sudo check tests the exact `usermod` command, so a narrow
passwordless rule does not need to allow unrelated commands. If the account already
names zsh but the executable is missing or not listed in `/etc/shells`, the task still
repairs it (both `/usr/bin/zsh` and `/bin/zsh` are accepted). Your currently running
terminal shell is left alone.

Verify the result in a new login session rather than checking `$SHELL` in the current
terminal. If zsh cannot be found, install its package first instead of pointing your
account at a missing executable. The
[chsh manual](https://man.archlinux.org/man/chsh.1.en) documents the underlying account
change and its permission rules; Dotfiles may use `usermod` instead as noted above.

## Configuration

The target shell is fixed to zsh with no TOML key to choose another shell. Make sure
zsh is installed through [System packages](system-packages.md) before running this task.

## Preview and apply

Preview whether the account needs switching to zsh:

```sh
dotfiles update --only login-shell --dry-run
```

Resolve any missing zsh executable or account-query error before [applying the
change](task-reference.md#preview-and-apply). Apply may require account-management
permissions or authentication. Confirm the new shell in a fresh login session rather
than inspecting `$SHELL` in your current terminal.

## Removal

Your account keeps its login shell after `dotfiles remove`. Switch back through your
operating system's account tools if needed; removing shell config files does not change
the account setting.
