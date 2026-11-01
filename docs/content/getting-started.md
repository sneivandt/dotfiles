---
title: Getting started
description: Set up a machine, make the configuration your own, and understand what changes before applying them.
tableOfContents:
  minHeadingLevel: 2
  maxHeadingLevel: 3
---

# Getting started

Set up and maintain **Arch Linux or Windows** with declarative configuration
and a Rust CLI.

This is a personal setup, not a collection of universally safe defaults.
Review the configuration before applying changes.

Already installed? See [Commands and usage](commands-and-usage.md) or
[Troubleshooting](troubleshooting.md).

## Set up your machine

The installation path is: keep a stable checkout, review its configuration,
preview the selected work, then apply it.

### 1. Choose a permanent checkout

Fork the repository if you want to maintain your own changes, then clone your
fork. To use this repository directly:

```sh
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
```

Keep this directory: installed links and the launcher point back into it.
Moving or deleting the checkout can break applications.

You need Git and a POSIX shell on Linux or PowerShell on Windows. The wrappers
download a verified CLI when needed; Rust is only needed to
[build from source](commands-and-usage.md#wrapper-bootstrap). Windows has additional
[requirements](windows-and-wsl.md#requirements).

### 2. Review the configuration

[`conf/`](https://github.com/sneivandt/dotfiles/tree/main/conf/) declares what to manage.
[`symlinks/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/) holds the application files linked into your home.
Use [Configuration](configuration-reference.md) to change those declarations or add a
[private overlay](private-overlays.md).

Choose `base` for a shell-focused machine or `desktop` for a workstation.
These are roles, **not safety levels**: both can change system settings.
[Profiles](profiles-and-categories.md) explains what each includes.

:::caution[Back up existing configuration first]
Installing a symlink can replace an
ordinary file or empty directory without a backup or confirmation prompt.
Nonempty directories cause a failure rather than being recursively removed.
Review any overlay scripts too: they are executable code.
:::

### 3. Discover tasks and preview

Run the preview from your checkout. With no existing profile selection, the
CLI asks you to choose `base` or `desktop` and remembers your answer.

On Linux:

```sh
./dotfiles.sh update --dry-run
```

On Windows, start PowerShell as your normal user, not as Administrator:

```powershell
.\dotfiles.ps1 update --dry-run
```

:::caution[A preview is not a sandbox]
Wrapper bootstrap can download files even for
`list` or `--help`. CLI dry-run can inspect tools, write logs and metadata, and
execute overlay check/preview scripts. It does not apply the normal task
changes. Read [Dry-run scope](commands-and-usage.md#dry-run-scope) before relying on it.
:::

Check the reported profile, platform, overlay, planned changes, failures, and
skips. After selecting a profile, use `./dotfiles.sh list` (or
`.\dotfiles.ps1 list`) to discover tasks. Use
[task selection](commands-and-usage.md#select-tasks) to narrow the work.

### 4. Apply what you reviewed

These commands **change your machine**. Normal updates also synchronize the
repository; see [Update lifecycle](update-lifecycle.md) for the sequence and controls.

On Linux:

```sh
./dotfiles.sh update
```

On Windows:

```powershell
.\dotfiles.ps1 update
```

Read the [Windows guide](windows-and-wsl.md) before accepting elevation prompts.
Applications may reload changed files immediately. Read the final summary:
a successful exit with skipped work is not necessarily a complete installation.

Once `launcher` and `path` have completed, open a new shell and run `dotfiles`
directly. With no command, it defaults to `update`, including repository sync
and eligible agent-package ref updates. See [Update lifecycle](update-lifecycle.md) to control those
changes. `remove` is partial cleanup, **not rollback or a full uninstall**.

## Find the right guide

For day-to-day work, start with [Commands and usage](commands-and-usage.md). To make this
setup your own, read [Configuration reference](configuration-reference.md) and
[Application settings](application-settings.md). If something goes wrong,
[Troubleshooting](troubleshooting.md) starts with the symptom.

Changing the CLI itself? [Contributing](contributing.md) explains where to
start and how to verify your changes.
