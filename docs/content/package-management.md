---
title: Package management
description: Choose packages and understand installation, upgrades, and removal with pacman, Paru, and winget.
---

# Package management

Dotfiles installs the packages selected by your profile. It uses native
package managers rather than maintaining a separate application store.
Installing missing packages, upgrading existing applications, and updating
the dotfiles CLI are different operations.

## Choose packages

Edit [`conf/packages.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml).
[Categories](profiles-and-categories.md) select entries for the current machine.
The AUR (Arch User Repository) provides community-maintained Arch packages;
Paru is the helper that builds and installs them.

| Source | Declaration | Installer |
|---|---|---|
| Arch repositories | Package name in an Arch category | pacman |
| AUR | `{ name = "spotify", aur = true }`, for example | Paru |
| Maintained local Arch recipe | Its package name with `aur = true` | Paru's prepared local repository |
| Windows | Exact winget ID, such as `Git.Git` | winget |

For example:

```toml title="conf/packages.toml"
[arch]
packages = ["ripgrep"]

[windows]
packages = ["Git.Git"]
```

Use the appropriate existing category rather than adding duplicate sections.
A [private overlay](private-overlays.md) can add packages or replace a matching
entry. See [Package declarations](configuration-reference.md#packages) for the
complete format.

Other Linux distributions can use shared configuration, but dotfiles does not
install their system packages. Provide the needed tools with that distribution's
package manager.

## Install and upgrade

Use the normal update preview to include package prerequisites:

```sh
dotfiles update --dry-run
```

Review the package actions and the platform-specific behavior below, then
remove `--dry-run` to apply. The native `packages` task and Arch's
`aur-packages` task query installed state and request missing packages.
A failed inventory is an error, not an empty machine.

### Arch Linux

:::caution[Installing a missing package can upgrade the system]
Pacman installation uses `pacman -Syu --needed --noconfirm`, so it can also
synchronize databases and upgrade other installed packages. This is not an
add-only operation.
:::

The `paru` task prepares a working AUR helper, including repairing a detected
broken helper after library changes. AUR installation runs without an extra
sudo wrapper around Paru.

Some declarations use [dotfiles-managed recipes](#custom-arch-packages) instead of the AUR.
Review those tracked recipes before applying: preparation runs recipe code,
and Paru skips its normal recipe review for these local repositories.

Dotfiles does not request already-installed AUR packages merely to upgrade
them. For normal dotfiles maintenance followed by a full package upgrade:

```sh
dotfiles update
paru -Syu
```

The second command upgrades repository, AUR, and custom packages, not only the
ones declared in this checkout. Review its transaction.

### Windows

Winget uses exact IDs and installs missing packages; dotfiles does not run
`winget upgrade --all`. Use winget separately when you intend to upgrade
already-installed applications.

For each missing package, installation first requests user scope. It retries
without a scope only when no suitable user-scope installer exists. Winget
prompts are disabled and agreements accepted, but an installer can still
request UAC consent.

Administrator-required, cancelled, and policy-blocked installs count as
**failed items**. Independent packages may continue, but that does not turn the
task into a success. An "already installed" or "no applicable upgrade" response
counts as current. A failed or unparseable `winget list` fails even in preview.

Keep the main run unelevated. See [Windows elevation](windows-and-wsl.md#elevation)
before retrying an installer that genuinely requires Administrator rights.

## Remove a package

Deleting an entry stops selecting it; it does **not** uninstall the application.
`dotfiles remove` leaves packages installed too.

First remove the declaration from every contributing configuration source,
including an overlay if applicable. Then use the native package manager for
a deliberate uninstall. Otherwise a later dotfiles update can install the
package again. Review dependent packages and any linked application settings
separately.

## Custom Arch packages

Custom Arch packages use recipes maintained in this repository to package
upstream binary releases. These **PKGBUILDs** are dotfiles-managed recipes,
rather than recipes fetched from the AUR. A normal update discovers available
versions, prepares copies of the reviewed recipes for Paru, and installs missing
selected packages.

| Package | Recipe | Upstream |
|---|---|---|
| `apm-dotfiles` | [`packages/apm/`](https://github.com/sneivandt/dotfiles/tree/main/packages/apm/) | Microsoft APM |
| `github-copilot-app-dotfiles` | [`packages/github-copilot-app/`](https://github.com/sneivandt/dotfiles/tree/main/packages/github-copilot-app/) | GitHub Copilot App |
| `powershell-dotfiles` | [`packages/powershell/`](https://github.com/sneivandt/dotfiles/tree/main/packages/powershell/) | Microsoft PowerShell |
| `github-copilot-cli-dotfiles` | [`packages/github-copilot-cli/`](https://github.com/sneivandt/dotfiles/tree/main/packages/github-copilot-cli/) | GitHub Copilot CLI |
| `openai-codex-dotfiles` | [`packages/openai-codex/`](https://github.com/sneivandt/dotfiles/tree/main/packages/openai-codex/) | OpenAI Codex CLI |

**A refreshed recipe is not an upgraded application.** Run the
[Arch upgrade workflow](#arch-linux) to upgrade packages that are already
installed. Paru alone uses the last prepared recipe snapshot; it does not
discover a newer release or read your latest source edits.

You do not need to edit a recipe just to select one of these packages.

**For maintainers:** See [Arch package recipes](arch-package-recipes.md) for
adding or changing a recipe. Preparation failures have their own
[troubleshooting table](arch-package-recipes.md#logs-and-troubleshooting).
