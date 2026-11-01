---
title: Visual Studio Code
description: Customize shared VS Code settings, manage extensions, and understand Insiders, remote settings, and first-login installation.
---

# Visual Studio Code

The `desktop` profile installs **VS Code Insiders** on Arch and Windows.
Settings are shared with stable VS Code when it is present, but extension
installation targets one detected editor, preferring Insiders.

Dotfiles manages three separate things: the
[application package](package-management.md), linked settings files, and a
list of extensions. Changing one does not automatically change the others.

## Settings and keybindings

Edit the tracked
[`settings.json`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/settings.json)
and
[`keybindings.json`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/keybindings.json).
VS Code's settings UI can write through those links into the checkout, so
review the repository diff after changing preferences in the editor.

The shared settings select Tokyo Night, Source Code Pro with fallback fonts,
word wrapping, and save-on-focus-change. They hide build/cache directories,
configure language tooling, and use Zsh on Linux and PowerShell on Windows.
The editor's application-update mode is manual; extension updates are separate.

The [symlink declarations](configuration-reference.md#symlinks) install these
files in the following places:

| Environment | Managed location beneath the home directory |
|---|---|
| Linux desktop | `.config/Code/User/` and `.config/Code - Insiders/User/` |
| Windows desktop | `AppData/Roaming/Code/User/` and `AppData/Roaming/Code - Insiders/User/` |
| VS Code Server | `.vscode-server/data/Machine/settings.json` |
| VS Code Server Insiders | `.vscode-server-insiders/data/Machine/settings.json` |

The server locations receive **settings only**, not desktop keybindings.
Dotfiles must run in the home that owns those files; configuring Windows does
not configure a WSL distribution or remote host. Workspace settings can still
override user settings.

## Extensions

[`conf/vscode-extensions.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml)
is the source of truth for the extension list. Entries are Marketplace IDs,
grouped by [profile categories](profiles-and-categories.md). For example:

```toml
[desktop]
extensions = ["rust-lang.rust-analyzer", "tamasfe.even-better-toml"]
```

The current list covers language tooling, containers, Azure, GitHub integration,
and the theme. The Windows desktop category additionally selects Remote - WSL.
Use [Private overlays](private-overlays.md) for personal additions rather than
editing generated or installed extension directories.

On an already configured machine, preview just this task:

```sh
dotfiles update --only vscode-extensions --dry-run
```

Remove `--dry-run` after reviewing the plan. For a first installation, use the
normal update so the application package and its prerequisites are included.

The task finds `code-insiders` before `code` (`.cmd` launchers on Windows),
queries the installed extension IDs once, and installs missing IDs. Comparisons
ignore case. A failed inventory is an error, not an empty installation; missing
CLI support is reported as unmet work.

**The list is not a version lockfile or an uninstall list.** Already-present
IDs are left alone. Removing a declaration does not uninstall that extension,
and dotfiles removal leaves extensions installed. Use VS Code's Extensions
view for updates and deliberate removal.

## Arch desktop integration

The managed `code-insiders` launcher uses GNOME libsecret for credential
storage and forwards ordinary arguments to `/usr/bin/code-insiders`.
The desktop launcher and URL handler use the same integration. Keyring
problems belong in
[GNOME Keyring troubleshooting](troubleshooting.md#gnome-keyring-does-not-unlock-at-login),
not in extension configuration.

When provisioning Arch inside a chroot, dotfiles defers extension installation:
there is no real user session to verify Marketplace work. A durable marker
enables `dotfiles-first-login.service` to retry the extension task in the
graphical session. The marker is removed only after success.

## Troubleshooting

Read `dotfiles log --task vscode-extensions --verbose` first. It identifies the
selected CLI and captured failures. If extensions appear in the wrong editor,
check which Insiders/stable launcher is on PATH before reinstalling anything.

For settings that do not apply, check the selected profile, actual user/server
home, symlink destination, and workspace overrides. The shared file is not a
promise that every extension supports every target environment.

Agent integrations have additional configuration beyond extensions; see
[AI agent configuration](ai-agent-configuration.md).
