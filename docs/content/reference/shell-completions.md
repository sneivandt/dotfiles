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

Writes the shell completion registration after repository synchronization:

- Linux: `symlinks/config/zsh/completions/_dotfiles` **inside the checkout**.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Candidates come directly from the running CLI, including configuration-aware selectors.
This task does not install the shell itself or manage its profile symlinks.

Writing the file is only half of the setup: your shell still has to load it.
If completion does not appear after an update, open a new shell and check the
managed profile's [completion setup](../guides/application-settings.md#completion),
or consult the [completion system manual](https://man.archlinux.org/man/zshcompsys.1.en)
for zsh's function-search rules. Avoid editing the generated file to add selectors;
update the CLI metadata or selected configuration instead.

## Configuration

There is no TOML declaration for this task because the running CLI generates its own
registration. [Application settings](../guides/application-settings.md#completion)
explains how the managed shell profiles load it.

## Preview and apply

Preview where the generated registration will be written:

```sh
dotfiles update --only completions --dry-run
```

Expect the zsh path inside the checkout on Linux or the PowerShell profile
fragment in your home directory on Windows. Resolve any destination read error or
unexpected symlink before [applying the change](task-reference.md#preview-and-apply).
If the update warns that the file changed while it was being generated, save any local
edits elsewhere before retrying since the next update regenerates it.
Afterward, open a new shell and press Tab after `dotfiles update --only `. If no
selectors appear, check profile loading before touching generated files.

## Removal

The generated completion file remains after `dotfiles remove`. Delete it manually if
you no longer want completions, and make sure your shell profile does not still expect it.
