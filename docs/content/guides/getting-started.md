---
title: Getting started
description: Install dotfiles on Arch Linux, Windows, or WSL, choose a release or source build, back up existing configuration, and preview the first setup.
---

# Getting started

Dotfiles configures an existing Arch Linux, Linux, Windows, or WSL
installation rather than installing the operating system itself. This
walkthrough works from a Git checkout so you can inspect the defaults, back up
your existing files, and preview every change before applying anything.

If dotfiles is already installed on this machine, skip ahead to
[Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Choose your setup route

If you want to use this repository and its published CLI binary, jump straight
to [Set up your machine](#set-up-your-machine). The wrapper downloads the
executable automatically, so you do not need a Rust toolchain.

| Goal | Before you start |
|---|---|
| Keep your configuration in a fork | [Choose a release publisher](#using-a-fork), because a new fork does not inherit upstream release binaries. |
| Run the CLI from source | Install the [build prerequisites](../development/contributing.md#source-builds) and pass `--build` on every wrapper or launcher invocation. |

The checkout provides your configuration, while the release publisher or source
build provides the CLI binary that applies it. On WSL, follow the Linux steps
inside each distribution and the Windows steps on the host.

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

Start by picking a role: `base` for a shell-focused setup, or `desktop` for a
full workstation. These are roles, **not safety levels**, since either profile
can change system settings. Dotfiles combines your chosen profile with the
detected operating system to decide which declarations apply;
[Profiles and categories](profiles-and-categories.md) explains how that
matching works.

As an example, both profiles select `"config/git/config"` in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml),
which links `~/.config/git/config` to the tracked
[symlinks/config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config).
Read through its Git identity, defaults, and included files. Edit the tracked
source to change a preference, or edit the declaration to stop managing that
link (see [Home symlinks](../reference/home-symlinks.md#configuration) for the
full example). The other files under
[conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/) declare packages,
services, and settings rather than links;
[Configuration basics](configuration-basics.md) covers how to edit them and how
their paths resolve.

**Installed links are live, not copied snapshots.** Once a link is in place,
editing the tracked source changes what the application reads right away,
without running another dotfiles update. Some applications even reload those
changes immediately.

When you want to tailor things further, use
[Application settings](application-settings.md) to locate the tracked config
files, the [task pages](../reference/task-reference.md) for declaration syntax,
or a [private overlay](private-overlays.md) for settings that should stay out
of the public repository.

::: warning Back up existing configuration first
Preview the update first, then back up and move aside any existing files,
directories, or unrelated links that conflict with a managed link. Dotfiles
leaves conflicting entries in place rather than overwriting them. If you use a
private overlay with scripts, review those scripts before running a preview,
because previews execute their check and dry-run modes.
:::

Before applying for the first time, review these defaults carefully:

- **Git identity:** check the configured name, email, and
  [HTTPS-to-SSH push rewriting](application-settings.md#git).
  Pushing over SSH requires your own SSH authentication.
- **Automatic cleanup:** both Linux profiles enable daily
  [~/tmp cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service)
  for items untouched for 8 full days across modification, metadata change, and
  access times (freshly extracted files are kept even when their archived
  timestamps are old). Arch desktops also enable
  [~/screenshots cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-screenshots.service)
  after 31 days. Both services delete old files, links, and empty directories.
- **System settings:** both roles can touch privileged configuration. Inspect
  [system files](../reference/system-configuration-files.md#configuration),
  [services](../reference/systemd-units.md#configuration), and Windows
  [registry settings](windows-and-wsl.md#registry-settings).
- **Agent content:** packages can install executable integrations, and managed
  Copilot App workflows run in autopilot mode. Review
  [deployment targets](agent-configuration.md#deployment-targets).
- **Git hooks:** back up any custom hooks that share managed filenames before
  [installing replacements](../development/contributing.md#installation-and-removal).

Follow the [Git backup example](application-settings.md#back-up-and-restore-a-file)
when moving existing files aside, and keep backups outside managed destinations
and cleanup directories.

### 3. Preview the selected work

Run a preview from your checkout in an interactive terminal. When no profile is
saved yet, the CLI prompts you to pick `base` or `desktop` and remembers your
choice (for unattended runs, pass `--profile base` or `--profile desktop`
explicitly). Passing `--no-repo-update` during initial setup ensures the run
uses the exact checkout you just reviewed instead of fetching new commits.

On Linux:

```sh
./dotfiles.sh update --no-repo-update --dry-run
```

On Windows, run PowerShell as your normal user rather than as Administrator:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

::: warning A preview is not a sandbox
The wrapper may download the CLI binary before the preview begins. Dry-run
still writes a run log, can save your profile or overlay selection, and runs
overlay check and preview scripts. It does not apply the planned configuration
changes. See [Dry-run scope](commands-and-usage.md#dry-run-scope) for the exact
boundaries.
:::

Start by checking the header line to confirm the profile, platform, and
overlay. For the Git example above, look under Home symlinks for a planned link
from `~/.config/git/config` to this checkout. If a link already matches, it is
omitted unless you pass `--verbose`.

Review the **entire plan**, not just one link: check planned packages,
services, system files, failures, and skips. Each *task* groups related work,
such as Home symlinks or System packages. Once a profile is saved, run
`./dotfiles.sh list` (or `.\dotfiles.ps1 list`) to see the available task
selectors. Use [task selection](commands-and-usage.md#select-tasks) to preview
a narrower slice, keeping in mind that `--only symlinks` still covers every
active symlink rather than a single file.

With `--no-symbols`, status markers print as plain text. Here is an excerpt
captured from an isolated Windows fixture with a missing Git link and no VS
Code CLI:

```text
Update · dry run · base profile · Windows

SKIPPED VS Code extensions · VS Code CLI not found

PLANNED Home symlinks
  link ~/.config/git/config → symlinks/config/git/config
```

The header confirms the active role and platform. `PLANNED` names each task
with work to do, with indented lines showing individual destinations and
sources. `SKIPPED` tells you why a task could not run (here, install the VS
Code CLI if you want extensions managed). Your own preview will typically list
more tasks, printed in completion order.

### 4. Apply what you reviewed

These commands **modify your machine** using the current checkout. Repository
sync stays disabled, and the CLI re-inspects the machine rather than replaying
a cached plan from the preview.

On Linux:

```sh
./dotfiles.sh update --no-repo-update
```

On Windows:

```powershell
.\dotfiles.ps1 update --no-repo-update
```

On Windows, read the [Windows and WSL guide](windows-and-wsl.md) before
approving any elevation prompts. Because symlinks are live, applications may
pick up changed files immediately. Check the final summary carefully: an exit
code of 0 with skipped tasks can still mean part of your setup was not applied.

### 5. Verify the installation

Open a new shell so the updated PATH and launcher take effect. On Linux:

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

You should see `dotfiles` resolving inside your home directory's `.local/bin`,
your Git config pointing into the checkout (or overlay), and `dotfiles list`
showing tasks for your saved profile and platform. If you skipped a task or
edited the link list, verify the integrations you enabled instead.

Follow up on every failed or skipped task in the apply summary (see
[Recover from a partial update](troubleshooting.md#recover-from-a-partial-update)
if anything stopped partway through). Keep in mind that `dotfiles check`
validates configuration files and tracked sources, not installed packages, live
links, or running services.

Once initial setup is working, routine maintenance is just `dotfiles update`,
which also syncs the repository and updates declared agent dependencies. Commit
local edits first, or keep passing `--no-repo-update` while making local
changes (see [Update lifecycle](commands-and-usage.md#update-lifecycle)). Also
note that `dotfiles remove` performs scoped cleanup rather than a rollback or
full uninstall.

## Where to go next

Continue to [Commands and usage](commands-and-usage.md) for everyday workflows,
task selection, and reading logs. When you are ready to customize software and
configuration, explore [Package management](package-management.md),
[Application settings](application-settings.md), and the
[task pages](../reference/task-reference.md). If anything goes wrong,
[Troubleshooting](troubleshooting.md) lets you start directly from the symptom.

If you plan to change the CLI itself, start with
[Contributing](../development/contributing.md).
