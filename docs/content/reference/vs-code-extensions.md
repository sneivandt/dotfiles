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
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) and [AUR packages](aur-packages.md) (conditional); [Systemd units](systemd-units.md) (first-login setup) |
| Implementation | [editors/vscode_extensions.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/editors/vscode_extensions.rs) |

## What it does

Reads the selected editor's installed extension list and installs only missing IDs.
Afterward, it re-reads the list even if an install command failed, reporting an
extension as applied only when its install command succeeded and the editor lists its
ID. Missing IDs and command failures are still reported even if the extension appears
in the list. If no editor CLI is found, the task skips.

If the final verification query fails, no new installs are reported as applied (which
does not necessarily mean they failed to install). Cancelling stops remaining installs
and leaves unverified work unconfirmed. After any failure or cancellation, rerun the
same task so it queries a fresh list and installs only what is still missing.

CLI discovery prefers `code-insiders` over `code` on Linux, and `code-insiders.cmd`
over `code.cmd` on Windows. On Linux, a Dotfiles Insiders shim whose underlying editor
was removed is skipped. All configured extensions install into the chosen CLI even if
stable VS Code is currently open; there is no editor-selection setting in this file.

Because an Arch chroot has no graphical user session, deferring installs requires the
selected configuration to both symlink `dotfiles-first-login.service` and declare that
user service enabled. During apply, Dotfiles verifies the unit is installed and enabled
before queuing the extension list, profile, and overlay for first graphical login. If
deferral fails, rerun this task after logging in.

## Configuration

Edit
[conf/vscode-extensions.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml).

```toml
[desktop]
extensions = ["rust-lang.rust-analyzer", "tamasfe.even-better-toml"]
```

Use full `<publisher>.<extension>` Marketplace IDs rather than display names shown in
the editor. Duplicate IDs (including case-only differences) install once using the
first spelling. See [What it does](#what-it-does) for CLI selection and Arch chroot
deferral.

The [VS Code command-line reference](https://code.visualstudio.com/docs/configure/command-line)
documents listing, installing, and uninstalling extensions. If extensions appear to
install into the wrong edition, check which editor CLI was picked and inspect that
edition's Extensions view. Editor settings themselves belong in
[Application settings](../guides/application-settings.md#visual-studio-code).

## Preview and apply

Preview missing extensions with your configured launcher:

```sh
dotfiles update --only vscode-extensions --dry-run
```

Review the missing IDs and pass `--verbose` to confirm which editor CLI was selected.
If no CLI is available, install the editor and make its launcher discoverable before
retrying. After [applying the reviewed change](task-reference.md#preview-and-apply),
check that edition's Extensions view or run its CLI with `--list-extensions` (or check
after first graphical login when provisioning inside a chroot).

## Removal

Installed extensions stay put when you remove declarations or run `dotfiles remove`.
Use the editor's Extensions view to uninstall or update extensions; this file declares
extensions to ensure are installed, not an uninstall list.
