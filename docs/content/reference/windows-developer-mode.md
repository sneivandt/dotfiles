---
title: Windows Developer Mode
description: Enable Windows Developer Mode machine policy for file symlinks and understand Administrator requirements, junctions, and persistent settings.
---

# Windows Developer Mode

[Developer Mode](https://learn.microsoft.com/en-us/windows/advanced-settings/developer-mode)
is a Windows machine setting for development features. Dotfiles uses it to
support file symlink creation without running every setup task as Administrator.
It is not a switch for Windows Terminal, WSL, or the user's editor settings.

| Task detail | Value |
|---|---|
| Selector | `developer-mode` |
| Commands | `update` |
| Availability | Windows |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [system/developer_mode.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/developer_mode.rs) |

## What it does

Enables the machine-level Developer Mode policy when unset, which requires
Administrator rights. [Home symlinks](home-symlinks.md) waits for this task and then
checks its own link capability, so unavailable Developer Mode never blocks links that
are already correct or directory junctions that work without it.

Home symlinks stay in your original user process and recheck file-link capability once
this task finishes. If file links remain unavailable, they are skipped instead of being
delegated to a potentially different Administrator account. See
[Windows elevation](../guides/windows-and-wsl.md#elevation).

## Configuration

This task has no TOML settings; it manages the Windows Developer Mode policy as part of
Windows setup. See [Windows and WSL](../guides/windows-and-wsl.md#developer-mode-and-symlinks)
for symlink requirements.

Specifically, it sets `AllowDevelopmentWithoutDevLicense` under
`HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock` to DWORD `1`.
Because this is a machine-wide policy (unlike the `HKCU` entries in
[User registry settings](user-registry-settings.md)), check organizational device
policies before applying it; adding a user registry setting cannot replace this
machine-level flag.

## Preview and apply

Preview whether the machine's Developer Mode policy needs enabling:

```sh
dotfiles update --only developer-mode --dry-run
```

Confirm that the proposed change is the machine-wide value described above. If
organizational policy or elevation blocks it, resolve that restriction before
[applying the change](task-reference.md#preview-and-apply) rather than adding an
`HKCU` entry. Afterward, preview [Home symlinks](home-symlinks.md) to see what link
work remains.

## Removal

Developer Mode stays enabled after `dotfiles remove`. To turn it off, use Windows
Settings and check first whether any applications or file symlinks depend on it.
