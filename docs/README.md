# Getting started

This repository contains my personal configuration. Review and adapt it before
applying it to your machine. The steps below take you from a checkout to a
preview, then an installation.

## 1. Choose a permanent checkout

Fork the repository if you want to keep your own configuration changes, then
clone your fork. To use this repository directly:

```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
```

Keep the checkout at a stable path. Installed home links and the launcher refer
back to it; moving or deleting it later can break applications.

You need Git and either a POSIX shell on Linux or PowerShell on Windows. The
wrappers download and verify the CLI when needed. Rust is only required to
[build from source](USAGE.md#wrapper-bootstrap). See the
[Windows requirements](WINDOWS.md#requirements) for platform prerequisites.

## 2. Review configuration and choose a profile

Desired state lives in [`conf/`](../conf/), with application configuration in
[`symlinks/`](../symlinks/). Review the packages, links, settings, and services
that will apply to your machine. Use the [Configuration guide](CONFIGURATION.md)
to customize them, or add a [private overlay](CONFIGURATION.md#overlays).
Review overlay scripts before running them.

Choose `base` for a shell-focused machine or `desktop` for a workstation.
These are roles, not safety levels; both can change system settings. See
[Profiles](PROFILES.md) for the categories each selects.

Back up existing home configuration before applying changes.
**Symlink installation can replace existing ordinary files and empty directories
without a backup.** A warning is not a confirmation prompt. Nonempty directories
cause a failure rather than being recursively removed.

## 3. Discover tasks and preview changes

Run from the repository root. These examples use `base` on Linux and `desktop`
on Windows; substitute the profile you chose.

### Linux

```bash
# May download the CLI; task discovery does not apply configuration.
./dotfiles.sh list --root . --profile base

# Preview this checkout and suppress the CLI release check.
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles update \
  --root . --profile base --no-repo-update --dry-run --verbose
```

### Windows

Open PowerShell as your normal user, normally **not** as Administrator:

```powershell
# May download the CLI; task discovery does not apply configuration.
.\dotfiles.ps1 list --root . --profile desktop

# Preview this checkout; does not apply task changes.
.\bin\dotfiles.exe update --root . --profile desktop --no-repo-update --dry-run --verbose
```

The Windows preview can still check for a CLI release and write its cache. See
[Windows bootstrap](WINDOWS.md#bootstrap) to suppress that check for a run.

**Wrapper bootstrap is outside CLI dry-run:** even help or task discovery can
download files. A CLI preview writes logs and lock metadata, inspects external
tools, and may run overlay scripts in their check or preview modes. Read
[Dry-run scope](USAGE.md#dry-run-scope) for the full boundaries.

Check the preview's profile, platform, and overlay, then review planned changes
and any failures or skips. To focus on particular work, use
[task selection](USAGE.md#select-tasks).

## 4. Apply the reviewed configuration

After reviewing the preview and backing up affected files, these commands
**apply** the current checkout without synchronizing it first.

On Linux:

```bash
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles update \
  --root . --profile base --no-repo-update
```

On Windows:

```powershell
.\bin\dotfiles.exe update --root . --profile desktop --no-repo-update
```

Read the [Windows guide](WINDOWS.md) before responding to elevation prompts.
Applications may auto-reload changed configuration immediately. A successful
exit with skipped tasks does not necessarily mean the installation is complete;
read the summary and accompanying reasons.

After the `launcher` and `path` tasks complete, open a new shell and use
`dotfiles` directly. Running `dotfiles` defaults to `update`, which also
synchronizes the repository and updates eligible APM pins. See
[Apply configuration](USAGE.md#apply-configuration) for how to control those
sources of change. [Remove](USAGE.md#remove) is limited cleanup, not rollback.

## Next steps

- [Usage](USAGE.md): commands, options, task selection, and logs.
- [Configuration](CONFIGURATION.md): packages, links, settings, and private overlays.
- [Troubleshooting](TROUBLESHOOTING.md): failures, unexpected skips, and recovery.
- [Contributing](CONTRIBUTING.md): CLI development, architecture, and validation.
