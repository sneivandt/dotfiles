---
title: Shell completions
description: Completion registration for the current CLI.
---

# Shell completions

| Task | Selection |
|---|---|
| Selector | `completions` |
| Commands | `update` |

## What it does

Writes completion registration after repository synchronization:

- Linux: `symlinks/config/zsh/completions/_dotfiles` **inside the checkout**.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Candidates come from the running CLI, including configuration-aware selectors. This task
does not install the shell or all its profile links.

## Configuration

There is no TOML declaration for this task. The running CLI generates its own completion
registration. [Shell configuration](application-settings.md#completion) explains how the
managed profiles load it.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only completions --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

The generated completion registration remains after `dotfiles remove`. Remove it
separately if you no longer want completion, and check that your shell profile does not
still expect that registration.
