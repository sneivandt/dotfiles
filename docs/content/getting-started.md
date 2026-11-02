---
title: Getting started
description: Set up a machine, make the configuration your own, and understand what changes before applying them.
tableOfContents:
  minHeadingLevel: 2
  maxHeadingLevel: 3
---

# Getting started

Dotfiles uses a Rust command-line program to apply the packages, files, and
settings declared in this repository to **Arch Linux or Windows**.

This is a personal setup, not a collection of universally safe defaults.
Review the configuration before applying changes.

Already installed? See [Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Set up your machine

The installation path is: keep a stable checkout, review its configuration,
preview the selected work, then apply it.

### 1. Choose a permanent checkout

You need Git and a POSIX shell on Linux, or Git and PowerShell on Windows.
Downloading the CLI on Linux also needs `curl` or `wget`. See the
[Windows requirements](windows-and-wsl.md#requirements) before continuing there.

Fork the repository if you want to maintain your own changes, then clone your
fork into a directory you intend to keep. To use this repository directly:

```sh
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
```

Keep this directory: installed links and the launcher point back into it.
Moving or deleting the checkout can break applications.

The wrapper scripts, `dotfiles.sh` and `dotfiles.ps1`, obtain the CLI before
running your command. By default they download and verify a release from the
checkout's GitHub `origin`.

**A new fork does not have release binaries.** Choose one of these routes
before running its wrapper:

- **Use this project's released CLI with your fork's configuration.** If you
  trust this project's releases, explicitly select that publisher in your
  current terminal:

  ```sh
  export DOTFILES_RELEASE_REPO=sneivandt/dotfiles
  ```

  In PowerShell:

  ```powershell
  $env:DOTFILES_RELEASE_REPO = 'sneivandt/dotfiles'
  ```

  This chooses the binary, not the configuration repository. The downloaded
  binary also uses that publisher for self-updates. If your fork publishes
  compatible releases, you can use its default publisher instead.

- **Build the CLI from your fork.** Install the
  [source-build prerequisites](contributing.md#development-setup), then add
  `--build` after the wrapper name in **every command below**. For example,
  `./dotfiles.sh --build --version` or
  `.\dotfiles.ps1 --build --version` builds and prints the version without
  applying configuration. Build mode is not remembered between runs.

If you cloned this repository directly, the default download route needs no
publisher override or Rust installation.

### 2. Review the configuration

Choose `base` for a shell-focused machine or `desktop` for a workstation.
These are roles, **not safety levels**: both can change system settings.
The CLI adds categories for your operating system and selects matching
declarations. [Profiles and categories](profiles-and-categories.md) walks through
that selection.

There are two parts to an application configuration: a declaration says
**which file to install**, and a source file contains **the application's
settings**. For example, this is part of the real
[`conf/symlinks.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml):

```toml title="conf/symlinks.toml"
[base]
symlinks = [
  "config/git/config",
]
```

This is an excerpt, not a replacement for the existing `[base]` section.
Both profiles include `base`, so both select this entry:

| Part | In this example |
|---|---|
| Declaration | `"config/git/config"` in `conf/symlinks.toml` |
| Source to review or edit | [`symlinks/config/git/config`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config) |
| Destination in your home | `~/.config/git/config` |
| What applying does | Creates a link from the home destination to the source in this checkout |

Open that source and review its Git identity, defaults, and included files.
To change a Git preference, edit the source; to stop selecting this link,
change the declaration. Other files under
[`conf/`](https://github.com/sneivandt/dotfiles/tree/main/conf/) declare packages,
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

### 3. Preview the selected work

Run the preview from your checkout. With no existing profile selection, the
CLI asks you to choose `base` or `desktop` and remembers your answer.
For this first setup, `--no-repo-update` keeps repository synchronization out
of the run so you can work with the configuration you just reviewed.

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
Wrapper bootstrap can download files even for
`list` or `--help`. CLI dry-run can inspect tools, write logs and metadata, and
execute overlay check/preview scripts. It does not apply the normal task
changes. Read [Dry-run scope](commands-and-usage.md#dry-run-scope) before relying on it.
:::

Check the reported profile, platform, and overlay first. For the Git example,
look under **Home symlinks** for a planned link to `~/.config/git/config`
from this checkout. If it already points to the right source, it needs no
change; `--verbose` shows already-current work too.

Then review the **whole plan**, not just that link: packages, services, system
files, failures, and skips. A *task* is a group of work such as Home symlinks
or System packages. After selecting a profile, use `./dotfiles.sh list` (or
`.\dotfiles.ps1 list`) to discover task selectors. Use
[task selection](commands-and-usage.md#select-tasks) if you want a narrower
preview; `--only symlinks` selects all active links, not just the Git example.

### 4. Apply what you reviewed

These commands **change your machine**. They keep the same checkout content
as the preview by leaving repository synchronization disabled.

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

Once `launcher` and `path` have completed, open a new shell and run `dotfiles`
directly. Source-build users must still pass `--build`, for example
`dotfiles --build update --dry-run`.

For later maintenance, ordinary `dotfiles update` also synchronizes the
repository and updates agent dependencies when their declared version rules
allow it. Review and commit your configuration edits before using repository
sync, or keep `--no-repo-update` while working locally.
[Update lifecycle](update-lifecycle.md) explains these separate steps and
their controls. With no command, `dotfiles` defaults to `update`.
`remove` is partial cleanup, **not rollback or a full uninstall**.

## Find the right guide

For day-to-day work, start with [Commands and usage](commands-and-usage.md). To make this
setup your own, read [Configuration reference](configuration-reference.md) and
[Application settings](application-settings.md). If something goes wrong,
[Troubleshooting](troubleshooting.md) starts with the symptom.

Changing the CLI itself? [Contributing](contributing.md) explains where to
start and how to verify your changes.
