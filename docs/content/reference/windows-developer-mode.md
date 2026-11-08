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

Enables the machine-level Developer Mode policy when unset, requiring Administrator
rights. The Home symlinks task waits for this task, then checks its own needs.
Unavailable Developer Mode does not block links that are already correct or directory
junctions that can be created without it.

Pending file symlinks may still need their own elevation. The CLI normally stays
unelevated and delegates only affected tasks. See [Windows
elevation](../guides/windows-and-wsl.md#elevation).

## Configuration

This task has no TOML settings. It manages the Windows Developer Mode policy as part of
Windows setup. See [Windows and WSL](../guides/windows-and-wsl.md#developer-mode-and-symlinks) for
symlink requirements.

The managed value is `AllowDevelopmentWithoutDevLicense` under
`HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock`, set to DWORD
`1`. This is machine-wide policy, unlike the HKCU declarations in
[User registry settings](user-registry-settings.md). Review organizational
device restrictions before applying it; changing a user registry declaration
cannot substitute for this machine-level setting.

## Preview and apply

Preview whether the machine's Developer Mode policy needs enabling:

```sh
dotfiles update --only developer-mode --dry-run
```

Check that the proposed change is the machine-wide value described above.
If organizational policy or elevation prevents it, resolve that restriction
before [applying the change](task-reference.md#preview-and-apply), rather than
adding an HKCU setting. Afterward, preview [Home symlinks](home-symlinks.md)
to assess the links' own remaining requirements.

## Removal

Developer Mode remains enabled after `dotfiles remove`. If you want to disable it, use
Windows settings and first consider applications and file symlinks that depend on it.
