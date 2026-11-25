---
title: Getting started
description: Install dotfiles on Arch Linux, Windows, or WSL, choose a release or source build, back up existing configuration, and preview the first setup.
---

# Getting started

Start with an existing Linux or Windows installation. You'll make a permanent
copy of this repository (a *checkout*), review the settings it would apply,
and preview the changes before deciding whether to use them. The checkout
holds your configuration; its wrapper downloads the CLI, so you don't need Rust.

Arch Linux and Windows can install packages as part of setup. On other Linux
distributions, install the applications you want with your distribution's
package manager. For WSL, follow the Linux steps inside the distribution and
the Windows steps separately on the host.

The steps below use the project's published CLI for Linux x86-64/AArch64 or
Windows x86-64. If you maintain a fork or want to build the CLI yourself,
see [Using a fork](#using-a-fork) before the first preview. Already installed?
Go to [Commands and usage](commands-and-usage.md).

## Set up your machine

### 1. Clone into a permanent directory

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

Choose a *profile*: `base` for the shared terminal setup, or `desktop` to
include workstation software and settings such as Hyprland on Arch Linux.
Either profile can change system settings. Your profile and operating system
determine [which configuration entries apply](profiles-and-categories.md).

For example, both profiles select `"config/git/config"` in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml),
which links `~/.config/git/config` to the tracked
[symlinks/config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config).
Read through its Git identity, defaults, and included files. Edit the tracked
source to change a preference, or edit the declaration to stop managing that
link (see [Home symlinks](../reference/home-symlinks.md#configuration) for the
full example). Once installed, the link is live: Git reads the tracked file,
not a snapshot. For settings that should stay private, use a
[separate overlay](private-overlays.md).

Before applying for the first time, review these defaults carefully:

- **Git identity:** check the configured name, email, and
  [HTTPS-to-SSH push rewriting](application-settings.md#git).
  Pushing over SSH requires your own SSH authentication.
- **Automatic cleanup:** both Linux profiles enable daily
  [~/tmp cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service)
  for files, links, and empty directories whose contents, metadata, and access
  times are all at least 8 days old. Arch desktops also enable
  [~/screenshots cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-screenshots.service)
  for files, links, and empty directories last modified at least 31 days ago.
- **System settings:** both roles can touch privileged configuration. Inspect
  [system files](../reference/system-configuration-files.md#configuration),
  [services](../reference/systemd-units.md#configuration), and Windows
  [registry settings](windows-and-wsl.md#registry-settings).
- **Agent content:** packages can install executable integrations, and managed
  Copilot App workflows run in autopilot mode. Review
  [deployment targets](agent-configuration.md#deployment-targets).
- **Git hooks:** back up any custom hooks that share managed filenames before
  [installing replacements](../development/contributing.md#installation-and-removal).

### 3. Preview the selected work

Run a preview from your checkout in an interactive terminal. When no profile is
saved yet, the CLI prompts you to pick `base` or `desktop` and remembers your
choice after configuration loads. For unattended runs, pass `--profile base`
or `--profile desktop` explicitly. `--no-repo-update` keeps setup on the
checkout you just reviewed instead of fetching new commits.

::: warning A preview is not a sandbox
If you use an overlay, read its scripts first: their check and preview modes
execute code as your user. The wrapper may download the CLI, and the CLI can
write a run log and save the selected profile or overlay. It does not apply
the planned configuration. See [Dry-run scope](commands-and-usage.md#dry-run-scope).
:::

On Linux:

```sh
./dotfiles.sh update --no-repo-update --dry-run
```

On Windows, run PowerShell as your normal user rather than as Administrator:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

Check the header for the profile, platform, and overlay. The output groups
related work into *tasks*. Here's a Windows preview with `--no-symbols`, a
missing Git link, and no VS Code CLI (excerpt from an isolated test):

```text
Update · dry run · base profile · Windows

SKIPPED VS Code extensions · VS Code CLI not found

PLANNED Home symlinks
  link ~/.config/git/config → symlinks/config/git/config
```

`PLANNED` shows where the Git link would point. `SKIPPED` explains why VS Code
extensions won't be installed. Read the **whole plan**, including packages,
services, system files, failures, and skips. Unchanged items are hidden unless
you pass `--verbose`. You can [select fewer tasks](commands-and-usage.md#select-tasks),
but selecting Home symlinks still covers every active link, not just Git.

::: warning Back up existing configuration first
Dotfiles leaves conflicting files, directories, and unrelated links in place.
If you want a managed link at one of those paths, inspect the existing entry,
[back it up](application-settings.md#back-up-and-restore-a-file), and move it
aside. Keep backups outside managed paths and cleanup directories. Repeat the
preview and confirm the planned destination and source before applying.
:::

### 4. Apply what you reviewed

These commands **modify your machine**. Keep any task filters you used in the
preview. The CLI inspects the machine again; it doesn't replay a cached plan.
Don't count on `dotfiles remove` to undo an update: it can detach managed
links, for example, but it won't uninstall packages or restore old settings.

On Linux:

```sh
./dotfiles.sh update --no-repo-update
```

On Windows:

```powershell
.\dotfiles.ps1 update --no-repo-update
```

On Windows, read the [Windows and WSL guide](windows-and-wsl.md) before
approving any elevation prompts. Check the final summary: even with exit code
0, a skipped task may mean part of your setup was not applied.

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

If setup stopped partway through, [recover from the failed run](troubleshooting.md#recover-from-a-partial-update).
`dotfiles check` validates declarations and tracked sources; it doesn't verify
the installed links, packages, or services.

## Using a fork

The configuration comes from your fork, but a new fork has no release binaries.
If you trust this project's CLI to apply your fork's configuration, set the
release publisher in the terminal you'll use for setup:

```sh
export DOTFILES_RELEASE_REPO=sneivandt/dotfiles
```

In PowerShell:

```powershell
$env:DOTFILES_RELEASE_REPO = 'sneivandt/dotfiles'
```

The downloaded CLI uses its publisher for self-updates. Keep the variable in
your shell configuration if you want future wrapper downloads to use it too.
A fork that publishes compatible releases can use its own publisher instead.

To build from source, install the [build prerequisites](../development/contributing.md#source-builds)
and try `./dotfiles.sh --build --version` or `.\dotfiles.ps1 --build --version`.
Build mode isn't saved: include `--build` on each wrapper or launcher command.

## Where to go next

For routine maintenance, `dotfiles update` syncs the checkout and applies the
selected configuration. Commit local edits first, or keep `--no-repo-update`
while testing them. See [Commands and usage](commands-and-usage.md) for task
selection and logs, or [Configuration basics](configuration-basics.md) to
change what your next update will manage.
