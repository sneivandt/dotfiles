---
title: VS Code extensions
description: Missing editor extensions.
---

# VS Code extensions

Extensions add editor functionality such as language support and formatting.
They are installed through the editor CLI, separately from installing the
editor package or linking its settings. This task ensures the selected IDs
are present; it does not pin extension versions or configure their preferences.

| Task detail | Value |
|---|---|
| Selector | `vscode-extensions` |
| Commands | `update` |
| Availability | Configured extensions |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md), [AUR packages](aur-packages.md), conditional on readiness |
| Implementation | [editors/vscode_extensions.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/editors/vscode_extensions.rs) |

## What it does

After waiting for package tasks, finds an editor CLI, reads installed extensions, and
installs missing ones. If no CLI is available, the task is skipped rather than reported
as successful.

Arch chroot provisioning instead defers installation to `dotfiles-first-login.service`,
which retries until configured extensions are installed in a real user session.

## Configuration

Edit
[conf/vscode-extensions.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml).

```toml
[desktop]
extensions = ["rust-lang.rust-analyzer", "tamasfe.even-better-toml"]
```

Use full `<publisher>.<extension>` strings. Repeated IDs, including case-only variants,
are installed once using the first spelling. The task needs an available VS Code CLI and
installs missing extensions; Arch chroot provisioning defers this to first login. See
[VS Code extensions](#what-it-does).

An extension ID is the publisher-qualified Marketplace identifier, not the
display name shown in the editor. The
[VS Code command-line reference](https://code.visualstudio.com/docs/configure/command-line)
documents listing, installing, and uninstalling extensions. When installation
appears to succeed in the wrong editor, check which editor CLI was selected
and inspect that editor's Extensions view. Preferences belong in
[Application settings](application-settings.md#visual-studio-code), not this list.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only vscode-extensions --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Installed extensions remain after removing their declarations or running `dotfiles
remove`. Use the selected editor's Extensions view for deliberate removal and updates;
this list is not an uninstall list.
