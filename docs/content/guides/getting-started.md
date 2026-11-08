---
title: Getting started
description: Install dotfiles on Arch Linux, Windows, or WSL, choose a release or source build, back up existing configuration, and preview the first setup.
outline: [2, 3]
---

# Getting started

Dotfiles uses a Rust command-line program to apply the packages, files, and
settings declared in this repository to an existing Arch Linux or Windows
installation. It does not install the operating system.

This is a personal setup, not a collection of universally safe defaults.
Review the configuration before applying changes.

Already installed? See [Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Choose your setup route

| Configuration and binary | Before the first preview | Next step |
|---|---|---|
| This checkout and its released CLI | Git, a POSIX shell on Linux or PowerShell on Windows, and the download requirements below | [Choose a permanent checkout](#1-choose-a-permanent-checkout) |
| Your fork and this project's released CLI | Deliberately select this project's release publisher | [Using a fork](#using-a-fork), then the setup walkthrough |
| Your fork and its own released CLI | Publish compatible release assets in your fork | Review [download verification](security-and-trust.md#release-downloads), then the setup walkthrough |
| Current source instead of a released CLI | The pinned Rust toolchain and native build prerequisites; keep `--build` in each wrapper invocation | [Source builds](../development/contributing.md#source-builds), then the setup walkthrough |

These routes change where the executable comes from, not which configuration
is safe for your machine. All require reviewing the selected declarations and
backing up existing configuration before applying. Use the Windows commands
in the walkthrough for Windows; run the Linux commands inside each WSL distribution.

## Supported environments

| Environment | What dotfiles manages |
|---|---|
| Arch Linux | Shared configuration and pacman/Paru packages; the desktop profile adds Hyprland and desktop services |
| Other Linux distributions | Shared configuration and applicable Linux settings; install required applications with your distribution's package manager |
| Windows | Windows configuration and winget packages; no Hyprland desktop |
| WSL | The Linux distribution and its WSL integration; configure the Windows host separately |

Published binaries support Linux x86-64/AArch64 and Windows x86-64. Use
[Arch desktop](arch-desktop.md) for the Hyprland session and desktop services,
or [Windows and WSL](windows-and-wsl.md) for Windows requirements, elevation,
and Linux integration.

## Choose a checkout

Use this repository directly if you want to follow its shared configuration.
To maintain your own public changes, fork it and clone your fork instead.
Either way, review the personal defaults before applying them. Private
additions can stay in a [separate overlay](private-overlays.md).

The wrapper scripts download and verify the CLI, so the normal setup needs no
Rust toolchain. A fork needs one extra choice before its first download.

### Using a fork

**A new fork has no release binaries.** If you trust this project's released
CLI and want to use it with your fork's configuration, select its publisher
in the terminal you will use for setup:

```sh
export DOTFILES_RELEASE_REPO=sneivandt/dotfiles
```

In PowerShell:

```powershell
$env:DOTFILES_RELEASE_REPO = 'sneivandt/dotfiles'
```

This selects the binary, not the configuration repository. The downloaded
binary also uses that publisher for self-updates. The setting lasts for this
terminal; keep it in your shell configuration if future bootstrap downloads
should use the same publisher. A fork with compatible releases can use its
own default publisher.

Alternatively, follow [Source builds](../development/contributing.md#source-builds). With that
route, add `--build` after the wrapper or installed launcher name in **every
dotfiles command** below, including verification through the launcher.
For example, `./dotfiles.sh --build --version` or
`.\dotfiles.ps1 --build --version` builds and prints the version without
applying configuration. Build mode is not remembered between invocations.

## Set up your machine

Keep the checkout at a permanent path, review its configuration, and preview
the changes before applying them.

### 1. Choose a permanent checkout

You need Git and a POSIX shell on Linux, or Git and PowerShell on Windows.
On Linux, downloading the CLI also needs `curl` or `wget`. Install GitHub CLI
(`gh`) to verify where release binaries were built. The first download warns
and continues without it, but later CLI self-updates require it. See
[download verification](security-and-trust.md#build-provenance-verification)
or the [Windows requirements](windows-and-wsl.md#requirements) for details.

To use this repository directly, clone it into a directory you intend to keep:

```sh
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
```

Keep this directory: installed links and the launcher point back into it.
Moving or deleting the checkout can break applications. If its location must
change later, use [Move a checkout](commands-and-usage.md#move-a-checkout).

If you chose a fork, substitute its clone URL and keep the publisher selection
or source-build option described above.

### 2. Review the configuration

Choose `base` for a shell-focused machine or `desktop` for a workstation.
These are roles, **not safety levels**: both can change system settings.
The CLI adds categories for your operating system and selects matching
declarations. [Profiles and categories](profiles-and-categories.md) walks through
that selection.

For example, both profiles select `"config/git/config"` in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml).
This links `~/.config/git/config` to the tracked
[symlinks/config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config).
Review its Git identity, defaults, and included files. Edit the source to change
a preference, or the declaration to stop selecting the link. See
[Home symlinks](../reference/home-symlinks.md#configuration) for the full example.
Other files under
[conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/) declare packages,
services, and settings rather than links. [Configuration
basics](configuration-basics.md) explains how to edit those declarations and
interpret their paths.

**Installed links are live, not copied snapshots.** Once this link is installed,
editing its source changes what Git reads without another dotfiles update.
Some applications reload changed files immediately.

Use [Application settings](application-settings.md) to find other sources,
[task pages](../reference/task-reference.md) for declaration formats,
or a [private overlay](private-overlays.md) for changes you do not want in the
public repository.

::: warning Back up existing configuration first
Preview the update, then back up and move aside any files, directories, or
unrelated links that block a managed link. Dotfiles leaves these entries in
place and reports the conflicts. Review any overlay scripts before running
the preview. They execute code.
:::

Before the first apply, check these defaults in particular:

- **Git identity:** review the supplied name, email, and
  [HTTPS-to-SSH push rewriting](application-settings.md#git).
  SSH pushes need your own authentication.
- **Automatic cleanup:** both Linux profiles select daily
  [~/tmp cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service)
  after 8 complete days since modification. Arch desktops also select
  [~/screenshots cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-screenshots.service)
  after 31 days. These services delete old files, links, and empty directories.
- **System settings:** both roles can select privileged changes. Inspect
  [system files](../reference/system-configuration-files.md#configuration),
  [services](../reference/systemd-units.md#configuration), and Windows
  [registry settings](windows-and-wsl.md#registry-settings).
- **Agent content:** packages can install executable integrations, and managed
  Copilot App workflows are enabled in autopilot mode. Review
  [deployment targets](agent-configuration.md#deployment-targets).
- **Git hooks:** preserve custom hooks at managed names before
  [installing replacements](../development/contributing.md#installation-and-removal).

Use the [Git backup example](application-settings.md#back-up-and-restore-a-file)
for an existing configuration file. Keep backups outside managed destinations
and cleanup directories.

### 3. Preview the selected work

Run the preview from your checkout in an interactive terminal. With no existing
profile selection, the CLI asks you to choose `base` or `desktop` and remembers
your answer. For unattended use, supply `--profile base` or `--profile desktop`.
For this first setup, `--no-repo-update` uses the checkout you just reviewed
without fetching repository changes.

On Linux:

```sh
./dotfiles.sh update --no-repo-update --dry-run
```

On Windows, start PowerShell as your normal user, not as Administrator:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

::: warning A preview is not a sandbox
The wrapper can download the CLI before the preview starts. Dry-run writes
logs, may save your selections, and can execute overlay check/preview scripts.
It does not apply the planned configuration changes. See
[Dry-run scope](commands-and-usage.md#dry-run-scope) for the full limits.
:::

Check the reported profile, platform, and overlay first. For the Git example,
look under Home symlinks for a planned link to `~/.config/git/config`
from this checkout. If it already points to the right source, it needs no
change; `--verbose` shows already-current work too.

Then review the **whole plan**, not just that link: packages, services, system
files, failures, and skips. A *task* is a group of work such as Home symlinks
or System packages. After selecting a profile, use `./dotfiles.sh list` (or
`.\dotfiles.ps1 list`) to discover task selectors. Use
[task selection](commands-and-usage.md#select-tasks) if you want a narrower
preview; `--only symlinks` selects all active links, not just the Git example.

With `--no-symbols`, the status labels appear as text. This excerpt was captured
from an isolated Windows fixture with a missing Git link and no VS Code CLI:

```text
Update · dry run · base profile · Windows

SKIPPED VS Code extensions · VS Code CLI not found

PLANNED Home symlinks
  link ~/.config/git/config → symlinks/config/git/config
```

The header identifies the selected role and platform. `PLANNED` names the task;
the indented line shows the home destination and checkout source. `SKIPPED`
means the editor extensions would not be installed. Resolve that prerequisite
if you need the extensions. Your full preview can contain more tasks and show
them in a different order.

### 4. Apply what you reviewed

These commands **change your machine** using the current checkout. They keep
repository synchronization disabled, but inspect the machine again rather
than execute a saved plan.

On Linux:

```sh
./dotfiles.sh update --no-repo-update
```

On Windows:

```powershell
.\dotfiles.ps1 update --no-repo-update
```

Read the [Windows guide](windows-and-wsl.md) before accepting elevation prompts.
Applications may reload changed files immediately. Read the final summary:
a successful exit with skipped work is not necessarily a complete installation.

### 5. Verify the installation

Open a new shell after the launcher and PATH tasks complete. On Linux:

```sh
command -v dotfiles
readlink "$HOME/.config/git/config"
dotfiles list
```

On Windows:

```powershell
Get-Command dotfiles -All
Get-Item "$HOME/.config/git/config" | Select-Object FullName, LinkType, Target
dotfiles list
```

The launcher should resolve under your home's `.local/bin`, and the Git link
should point into the intended checkout or overlay. The listing should contain tasks
appropriate for your saved role and platform; confirm the selected profile and
platform in the apply run's header. If you intentionally excluded a task or link,
verify the integrations you did select instead.

Read the apply summary and account for every failure or skipped task. Use
[partial-failure recovery](troubleshooting.md#recover-from-a-partial-update)
for unfinished work. `dotfiles check` validates configuration and source files;
it does not prove that packages, links, or running services were installed.

For later maintenance, ordinary `dotfiles update` also synchronizes the
repository and can install newer agent dependencies. Review and commit your
configuration edits before using repository sync, or keep `--no-repo-update`
while working locally.
[Update lifecycle](commands-and-usage.md#update-lifecycle) explains these separate steps and
their controls. With no command, `dotfiles` defaults to `update`.
`remove` is partial cleanup, **not rollback or a full uninstall**.

## Find the right guide

For day-to-day work, start with [Commands and usage](commands-and-usage.md).
To make this setup your own, find the application in
[Application settings](application-settings.md), then use
[task pages](../reference/task-reference.md) for declaration syntax.
If something goes wrong,
[Troubleshooting](troubleshooting.md) starts with the symptom.

Changing the CLI itself? [Contributing](../development/contributing.md) explains where to
start and how to verify your changes.
