---
title: Getting started
description: Install dotfiles on Arch Linux, Windows, or WSL, choose a release or source build, back up existing configuration, and preview the first setup.
---

# Getting started

Start with a working operating system. Dotfiles configures an existing
installation; it does not install Arch Linux or Windows. This walkthrough
uses a Git checkout so you can read the author's settings, back up your own,
and preview the changes before applying them.

Already installed? See [Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Choose your setup route

If you're using this repository and its released CLI, go straight to
[Choose a permanent checkout](#1-choose-a-permanent-checkout). The wrapper
downloads the executable; you don't need Rust.

| Goal | Before you start |
|---|---|
| Keep your configuration in a fork | [Choose a release publisher](#using-a-fork). A new fork doesn't inherit upstream binaries. |
| Run the CLI from source | Install the [build prerequisites](../development/contributing.md#source-builds) and pass `--build` on every wrapper or launcher invocation. |

The checkout supplies the settings; the release publisher or source build
supplies the program that applies them. On WSL, follow the Linux instructions
inside each distribution and the Windows instructions for the host.

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

Use this repository directly to follow its shared configuration, or fork and
clone your own copy to maintain public changes. Private additions can stay in a
[separate overlay](private-overlays.md).

### Using a fork

**A new fork has no release binaries.** If you trust this project's released
CLI with your fork's configuration, select its publisher in the setup terminal:

```sh
export DOTFILES_RELEASE_REPO=sneivandt/dotfiles
```

In PowerShell:

```powershell
$env:DOTFILES_RELEASE_REPO = 'sneivandt/dotfiles'
```

The downloaded binary also uses that publisher for self-updates; keep the
variable in your shell configuration if future bootstrap downloads should do
the same. A fork with compatible releases uses its own default publisher.

To build from source instead, test with `./dotfiles.sh --build --version` or
`.\dotfiles.ps1 --build --version`. Build mode is not remembered, so keep
`--build` on every wrapper or launcher invocation.

## Set up your machine

### 1. Choose a permanent checkout

You need Git and a POSIX shell on Linux, or Git and PowerShell on Windows.
For a released CLI, install GitHub CLI (`gh`) before the first download so the
wrapper can verify where the binary was built. On Linux, download bootstrap
also needs `curl` or `wget`, `sha256sum`, and `flock`. See
[download verification](security-and-trust.md#build-provenance-verification)
or the [Windows requirements](windows-and-wsl.md#requirements) for details.

Clone the repository (or your fork) into a directory you intend to keep:

```sh
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
```

Keep this directory: installed links and the launcher point back into it.
If its location must change later, use
[Move a checkout](commands-and-usage.md#move-a-checkout).

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
  after 8 complete days since modification, metadata change, and access.
  Freshly extracted files are retained even when their archived modification
  times are old. Arch desktops also select
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

The launcher should resolve under your home's `.local/bin`, the Git link should
point into the intended checkout or overlay, and `dotfiles list` should show
tasks matching your saved role and platform. If you excluded a task or link,
verify the integrations you selected instead.

Account for every failure or skipped task in the apply summary (use
[partial-failure recovery](troubleshooting.md#recover-from-a-partial-update) for
unfinished work). `dotfiles check` validates configuration and source files; it
does not verify installed packages, links, or running services.

For later maintenance, `dotfiles update` (the default command) also syncs the
repository and updates declared agent dependencies. Commit local edits first or
keep `--no-repo-update` while working locally; see
[Update lifecycle](commands-and-usage.md#update-lifecycle).
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
