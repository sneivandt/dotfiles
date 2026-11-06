---
title: Shell completions
description: Generate dotfiles tab completion for zsh and PowerShell, load shell registration, and diagnose missing command or task-selector suggestions.
---

# Shell completions

Completion lets the shell suggest dotfiles commands, flags, and task selectors
when you press Tab. The registration connects the shell to the running CLI's
candidates; it is not a second hand-maintained catalog of task names.

| Task detail | Value |
|---|---|
| Selector | `completions` |
| Commands | `update` |
| Availability | Linux (zsh) or Windows (PowerShell) |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Dotfiles repository](dotfiles-repository.md) |
| Implementation | [shell/completions.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/shell/completions.rs) |

## What it does

Writes completion registration after repository synchronization:

- Linux: `symlinks/config/zsh/completions/_dotfiles` **inside the checkout**.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Candidates come from the running CLI, including configuration-aware selectors. This task
does not install the shell or all its profile links.

Writing the registration is only half of setup: the shell must load it.
If completion is absent after an update, start a new shell and check the
managed profile's [completion setup](application-settings.md#completion).
For zsh's loading and function-search rules, see the
[completion system manual](https://man.archlinux.org/man/zshcompsys.1.en).
Do not edit the generated registration to add a selector; fix CLI metadata or
the selected configuration instead.

## Configuration

There is no TOML declaration for this task. The running CLI generates its own completion
registration. [Shell configuration](application-settings.md#completion) explains how the
managed profiles load it.

## Preview and apply

Preview the generated registration's destination:

```sh
dotfiles update --only completions --dry-run
```

Expect the zsh path inside the checkout on Linux or the PowerShell profile
fragment in home on Windows. Resolve a destination read error or unexpected
symlink before [applying the change](task-reference.md#preview-and-apply).
Then start a new shell and try Tab after `dotfiles update --only `. If no
selectors appear, check profile loading before editing generated content.

## Removal

The generated completion registration remains after `dotfiles remove`. Remove it
separately if you no longer want completion, and check that your shell profile does not
still expect that registration.
