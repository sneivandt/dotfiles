---
title: VS Code extensions
description: Declare VS Code extension IDs, install missing extensions for the selected profile, and diagnose unavailable tools or failed installations.
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
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) and [AUR packages](aur-packages.md) when needed for readiness; [Systemd units](systemd-units.md) when the first-login user service and its link are selected for setup |
| Implementation | [editors/vscode_extensions.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/editors/vscode_extensions.rs) |

## What it does

Dotfiles reads the selected editor's extension list and installs only missing IDs.
It reads the list again even if an install command failed. It reports an install
as applied only when the command succeeded and the editor lists the ID. The task
names missing IDs and reports command failures, even if the editor now lists
the extension. If no CLI is available, the task is skipped.

If the final list cannot be read, verification fails: no new installs are
reported as applied, but that does not mean they are absent. Cancelling stops
further installs and leaves unverified work unconfirmed. After a failure or
interruption, retry the same task. It reads a fresh list and requests only the
IDs still missing.

The search prefers `code-insiders` over `code` on Linux, and
`code-insiders.cmd` over `code.cmd` on Windows. On Linux, a Dotfiles
Insiders shim whose editor has been removed is skipped. The selected CLI receives all
configured extensions; having stable Code open does not make it the selected
edition. There is no editor-selection key in this declaration.

An Arch chroot has no graphical user session. To defer installation, the
selected configuration must link the `dotfiles-first-login.service` unit file
and declare the user service enabled. During apply, dotfiles checks that the
unit is installed and enabled before queuing the extensions. At first
graphical login the service uses the recorded profile, overlay, and
extension list. If deferral fails, run the extension task after login.

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
[Application settings](../guides/application-settings.md#visual-studio-code), not this list.

## Preview and apply

Preview missing extensions with your configured launcher:

```sh
dotfiles update --only vscode-extensions --dry-run
```

Check the missing IDs and use `--verbose` to see the selected CLI. If it is
unavailable, install the intended editor and make its launcher discoverable
before retrying. After [applying the reviewed
change](task-reference.md#preview-and-apply), inspect that edition's Extensions
view or run its CLI with `--list-extensions`. In a chroot, verify the deferred
installation after the first real user login instead.

## Removal

Installed extensions remain after removing their declarations or running `dotfiles
remove`. Use the selected editor's Extensions view for deliberate removal and updates;
this list is not an uninstall list.
