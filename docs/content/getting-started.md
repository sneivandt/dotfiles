---
title: Getting started
description: Set up a machine, make the configuration your own, and understand what changes before applying them.
tableOfContents:
  minHeadingLevel: 2
  maxHeadingLevel: 3
---

# Getting started

Dotfiles uses a Rust command-line program to apply the packages, files, and
settings declared in this repository to an existing Arch Linux or Windows
installation. It does not install the operating system.

This is a personal setup, not a collection of universally safe defaults.
Review the configuration before applying changes.

Already installed? See [Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Supported environments

| Environment | What dotfiles manages |
|---|---|
| Arch Linux | Shared configuration and pacman/Paru packages; the desktop profile adds Hyprland and desktop services |
| Other Linux distributions | Shared configuration and applicable Linux settings; install required applications with your distribution's package manager |
| Windows | Windows configuration and winget packages; no Hyprland desktop |
| WSL | The Linux distribution and its WSL integration; configure the Windows host separately |

Published binaries support Linux x86-64/AArch64 and Windows x86-64. See
[Windows and WSL](windows-and-wsl.md) for that platform's requirements.

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
Moving or deleting the checkout can break applications.

The wrapper scripts, `dotfiles.sh` and `dotfiles.ps1`, download and verify the
CLI when needed, so you do not need Rust installed. To maintain your own public
changes, fork the repository and clone your fork instead. A new fork has no
release binaries; follow [Using a fork](cli-reference.md#using-a-fork) before
running its wrapper.

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
[Configuration reference](configuration-reference.md#symlinks) for the full example.
Other files under
[conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/) declare packages,
services, and settings rather than links.

**Installed links are live, not copied snapshots.** Once this link is installed,
editing its source changes what Git reads without another dotfiles update.
Some applications reload changed files immediately.

Use [Application settings](application-settings.md) to find other sources,
[Configuration reference](configuration-reference.md) for declaration formats,
or a [private overlay](private-overlays.md) for changes you do not want in the
public repository.

:::caution[Back up existing configuration first]
Installing a symlink can replace an
ordinary file or empty directory without a backup or confirmation prompt.
Nonempty directories cause a failure rather than being recursively removed.
Review any overlay scripts too: they are executable code.
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
  [system files](configuration-reference.md#system-files),
  [services](configuration-reference.md#systemd-units), and Windows
  [registry settings](windows-and-wsl.md#registry-settings).
- **Agent content:** packages can install executable integrations, and managed
  Copilot App workflows are enabled in autopilot mode. Review
  [deployment targets](agent-configuration.md#deployment-targets).
- **Git hooks:** preserve custom hooks at managed names before
  [installing replacements](contributing.md#installation-and-removal).

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

If you chose the source-build route, include `--build` after the wrapper name.
For example: `./dotfiles.sh --build update --no-repo-update --dry-run`.

:::caution[A preview is not a sandbox]
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

Keep `--build` in these commands too if you chose source builds.
Read the [Windows guide](windows-and-wsl.md) before accepting elevation prompts.
Applications may reload changed files immediately. Read the final summary:
a successful exit with skipped work is not necessarily a complete installation.

### 5. Verify the installation

Open a new shell after the launcher and PATH tasks complete. Source-build
users should add `--build` to launcher commands below. On Linux:

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
[Configuration reference](configuration-reference.md) for declaration syntax.
If something goes wrong,
[Troubleshooting](troubleshooting.md) starts with the symptom.

Changing the CLI itself? [Contributing](contributing.md) explains where to
start and how to verify your changes.
