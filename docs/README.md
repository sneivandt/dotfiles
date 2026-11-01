---
title: Getting started
nav_order: 1
permalink: /
---

# Dotfiles documentation

Read these guides at [dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/),
or browse the same Markdown files in the repository.

Start here to install the configuration, change it, or work on the Rust CLI.
This is a personal setup for Arch Linux and Windows, not a collection of
universally safe defaults. Review the parts you intend to use before applying
them.

## Find the right guide

| I want to... | Read |
|---|---|
| Set up a machine for the first time | [Getting started](#getting-started) below; [Windows](WINDOWS.md) for platform requirements |
| Run an update, select tasks, or read logs | [Usage](USAGE.md) |
| Choose between a shell setup and a desktop | [Profiles](PROFILES.md) |
| Change packages, links, settings, or private configuration | [Configuration](CONFIGURATION.md) |
| Customize the shell, editor, desktop, or shortcuts | [Application configuration](APPLICATIONS.md) |
| Find a task's selector and understand what it changes | [Task reference](TASKS.md) |
| Configure agent packages and plugins | [Agent tooling](AGENT-TOOLING.md) |
| Maintain Arch package recipes | [Local packages](LOCAL-PACKAGES.md) |
| Understand Git hooks or fix a rejected commit | [Hooks](HOOKS.md) |
| Diagnose a failure or an unexpected skip | [Troubleshooting](TROUBLESHOOTING.md) |
| Understand trust, downloads, privileges, and log privacy | [Security](SECURITY.md) |
| Contribute a change or understand the CLI design | [Contributing](CONTRIBUTING.md), [Architecture](ARCHITECTURE.md) |
| Choose checks or run a specific fixture suite | [Testing](TESTING.md), [Test recipes](TESTING-RECIPES.md) |
| Understand required CI checks and releases | [CI and releases](CI.md) |

## Getting started

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
[build from source](USAGE.md#wrapper-bootstrap). Windows has additional
[requirements](WINDOWS.md#requirements).

### 2. Review the configuration

[`conf/`](https://github.com/sneivandt/dotfiles/tree/main/conf/) declares what to manage.
[`symlinks/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/) holds the application files linked into your home.
Use [Configuration](CONFIGURATION.md) to change those declarations or add a
[private overlay](CONFIGURATION.md#overlays).

Choose `base` for a shell-focused machine or `desktop` for a workstation.
These are roles, **not safety levels**: both can change system settings.
[Profiles](PROFILES.md) explains what each includes.

**Back up existing configuration first.** Installing a symlink can replace an
ordinary file or empty directory without a backup or confirmation prompt.
Nonempty directories cause a failure rather than being recursively removed.
Review any overlay scripts too: they are executable code.

### 3. Discover tasks and preview

Run these commands from the checkout. Substitute your chosen profile.

On Linux:

```sh
# Bootstrap the CLI if necessary, then list available work.
./dotfiles.sh list --root . --profile base

# Preview this checkout, without repository sync or a CLI release check.
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles update \
  --root . --profile base --no-repo-update --dry-run --verbose
```

On Windows, start PowerShell as your normal user, not as Administrator:

```powershell
.\dotfiles.ps1 list --root . --profile desktop
.\bin\dotfiles.exe update --root . --profile desktop --no-repo-update --dry-run --verbose
```

The Windows preview can still check for a CLI release, but does not write the
release cache or replace the binary. See [Windows bootstrap](WINDOWS.md#bootstrap)
to suppress that check.

**A preview is not a sandbox.** Wrapper bootstrap can download files even for
`list` or `--help`. CLI dry-run can inspect tools, write logs and metadata, and
execute overlay check/preview scripts. It does not apply the normal task
changes. Read [Dry-run scope](USAGE.md#dry-run-scope) before relying on it.

Check the reported profile, platform, overlay, planned changes, failures, and
skips. Use [task selection](USAGE.md#select-tasks) to narrow the work.

### 4. Apply what you reviewed

These commands **change your machine**. They use the current checkout without
synchronizing it first.

On Linux:

```sh
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles update \
  --root . --profile base --no-repo-update
```

On Windows:

```powershell
.\bin\dotfiles.exe update --root . --profile desktop --no-repo-update
```

Read the [Windows guide](WINDOWS.md) before accepting elevation prompts.
Applications may reload changed files immediately. Read the final summary:
a successful exit with skipped work is not necessarily a complete installation.

Once `launcher` and `path` have completed, open a new shell and run `dotfiles`
directly. With no command, it defaults to `update`, including repository sync
and eligible agent-package ref updates. See [Usage](USAGE.md) to control those
changes. `remove` is partial cleanup, **not rollback or a full uninstall**.
