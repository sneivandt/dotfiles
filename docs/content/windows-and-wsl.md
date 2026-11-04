---
title: Windows and WSL
description: Prepare Windows, handle elevation and symlink requirements, and understand Windows-side and Linux-side WSL configuration.
---

# Windows and WSL

Run dotfiles from your **normal Windows account**, not an Administrator shell.
It asks for elevation only where needed. Windows and WSL have separate home
directories and must be configured separately.

For shared commands and options, see [Usage](commands-and-usage.md). This guide covers
Windows setup, permissions, packages, and the changes that need a new session.

## Requirements

| Requirement | Why |
|---|---|
| x86-64 Windows | The published Windows release asset supports this architecture |
| PowerShell | Runs `dotfiles.ps1`; Windows PowerShell and `pwsh` are supported by the wrapper |
| Git | Checkout, repository synchronization, hooks, and saved selections |
| winget | Installs configured Windows packages |
| GitHub CLI (`gh`) | Verifies release build provenance; bootstrap warns if it is absent |
| Rust/Cargo and native build prerequisites | Only needed for a [source build](contributing.md) |

Keep the checkout at a stable path: home links and the installed launcher refer
back to it. Review `conf/` and **back up existing home configuration** before
applying. Move aside any existing files, directories, or unrelated links that
block installation.

## Bootstrap

Use [Getting started](getting-started.md) to clone and review the configuration.
If you use a fork without releases, choose its
[binary publisher or source-build route](cli-reference.md#using-a-fork)
before running the wrapper.

The examples below assume downloaded release binaries. If you chose source
builds, keep `--build` after the wrapper name in every command, for example
`.\dotfiles.ps1 --build update --no-repo-update --dry-run`.
Build mode is not remembered between invocations.

Open PowerShell in the checkout and preview an update:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

The wrapper downloads and verifies the CLI if needed. Follow
[Preview the selected work](getting-started.md#3-preview-the-selected-work)
for profile selection and the preview's limits, then
[Apply what you reviewed](getting-started.md#4-apply-what-you-reviewed)
for the Windows apply command and first launcher check.
Review Windows-specific symlink and elevation behavior below before applying.
The remaining examples use the wrapper so they also work before the launcher
is installed.

### Download and update failures

Do not disable checksum or provenance verification to make a download work.
Check the [bootstrap troubleshooting guide](troubleshooting.md#wrapper-download-fails).

Windows can prevent replacement of an executable that is still in use. If
self-update fails, read the error before retrying or moving files. The
[self-update reference](cli-reference.md#cli-self-update) explains
verification, the post-update version check, and recovery if that check fails.

## Developer Mode and symlinks

Developer Mode lets a normal user create file symlinks. Dotfiles can enable
it through a narrowly scoped elevated task. Directory links can instead fall
back to junctions, and already-correct links do not need elevation.

Include the Developer Mode task when previewing link setup:

```powershell
.\dotfiles.ps1 list --graph update --only symlinks --with-deps
.\dotfiles.ps1 update --only symlinks --with-deps --dry-run
```

`--only symlinks` without `--with-deps` omits Developer Mode, but does not bypass
the link task's own capability checks. Unavailable Developer Mode does not
automatically block directory links or links that are already correct.

If linking fails, distinguish these causes:

| Problem | What to do |
|---|---|
| File-link creation is not permitted | Resolve Developer Mode or the requested elevation |
| A home target conflicts | Back it up and move it aside before retrying |
| Git checked out a source symlink as plain text | Repair the source checkout, not the home target |

For the last case, inspect the checkout without changing it:

```powershell
git config --show-origin --get core.symlinks
git ls-files --stage -- symlinks | Select-String '^120000 '
```

Git mode `120000` denotes a symlink. Compare the reported source's actual type
with its Git mode; an unset `core.symlinks` alone is not a diagnosis. Enabling
Developer Mode later does not turn an existing text placeholder into a link.
Preserve local edits, enable Git symlink support, then restore only affected
sources or use a fresh symlink-capable checkout. Dotfiles rejects detected
placeholders rather than installing their text as configuration.

## Elevation

Dotfiles keeps the main run unelevated. When necessary it names the affected
tasks and starts a short-lived Administrator child for that scope:

| Task | When it needs Administrator rights |
|---|---|
| `developer-mode` | The machine setting is not enabled |
| `symlinks` | File links need changing and unprivileged creation is unavailable |

The child has its own console and log, linked from the parent run.
Already-current work does not need a prompt. Dry-run does not request elevation
to apply changes.

- Declined consent or unavailable prompting leaves affected work skipped.
  Blocking dependents cannot run, but independent tasks continue.
- With `--fail-on-skip` or in CI, those skips make the command fail.
- A child that fails, or cannot start, is a failure, not declined consent.

**Winget installers are separate:** they can show their own UAC prompts even
though dotfiles did not elevate the package task.

If you deliberately enable Developer Mode from an Administrator terminal,
limit the change:

```powershell
.\dotfiles.ps1 update --only developer-mode
```

Return to the ordinary terminal for the remaining work. Running everything as
another Administrator account can target that account's home and settings.

## Packages

Windows installs selected packages through winget. See
[Package management](package-management.md#windows) for exact IDs, scope
fallback, upgrades, and how installation failures are handled.

```powershell
.\dotfiles.ps1 update --only packages --dry-run
```

If an installer genuinely needs Administrator rights and you authorize it,
use a package-only elevated rerun. Remember that `--only packages` selects
**all active packages**, not an individual ID.

## Registry settings

The `registry` task applies current-user (`HKCU`) values from
`conf/registry.toml`, preserving undeclared values. Its tables are named
settings groups, not profile categories: review them even for `base`.

```powershell
.\dotfiles.ps1 update --only registry --dry-run
```

Settings include console, regional formatting, Explorer, search, and window
behavior. Dotfiles does not back up the original values, and `remove` does not
restore them. Some settings take effect only when the application or user
session restarts; save work before restarting either.

## PATH and wrapper

The launcher task writes `%USERPROFILE%\.local\bin\dotfiles.cmd`, and the PATH
task adds that directory to persistent user PATH. The launcher calls this
checkout's `dotfiles.ps1`, preferring `pwsh` over Windows PowerShell.

Open a new terminal before testing it:

```powershell
Get-Command dotfiles -All
Test-Path "$env:USERPROFILE\.local\bin\dotfiles.cmd"
```

If PATH is not ready, keep using the explicit wrapper or binary. Multiple
launchers may point to different checkouts. `--root` selects configuration,
not the executable; use the desired checkout's wrapper when both matter.

## PowerShell configuration

The Windows symlinks install the managed profile for PowerShell and Windows
PowerShell. Edit the tracked
[Microsoft.PowerShell_profile.ps1](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/powershell/Microsoft.PowerShell_profile.ps1),
not an installed copy.
The profile provides a path and Git-aware prompt, command history, and
history-based suggestions when the installed PSReadLine supports them.
`dot` is shorthand for `dotfiles`, and `code` selects VS Code Insiders when
available.

Open a new session after editing the profile. Dotfiles command completion is
installed separately by the [completions task](commands-and-usage.md#shell-completions).

## Windows Terminal

The `windows-desktop` category links the tracked
[settings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json)
into Windows Terminal's packaged-app `LocalState` directory. It selects a
PowerShell 7 (`pwsh.exe`) profile by default and includes an Arch WSL profile.
If either is unavailable on your machine, edit the tracked settings before
installing the link. Windows Terminal may write changes back through the link;
review the repository diff after editing settings in the app.

## WSL

Run the **Linux wrapper inside the distribution** to manage its Linux home
and `/etc` settings. Running the Windows executable does not manage
`/etc/wsl.conf`.

On Windows, the linked
[.wslconfig](https://github.com/sneivandt/dotfiles/blob/main/symlinks/wslconfig)
in your home selects NAT networking for WSL 2. It controls WSL 2 globally.

Inside each distribution, the
[/etc/wsl.conf fragment](https://github.com/sneivandt/dotfiles/blob/main/system/wsl.conf)
enables systemd and disables automatic Windows PATH injection.
The Linux `system-files` task preserves unrelated INI settings
and can need sudo. Disabling PATH injection means Windows commands are not
automatically discoverable by name inside Linux; use their full paths or add
only the directories you need.

Changes to WSL startup settings need a restart. Save all WSL work, then run
this from Windows:

```powershell
wsl --shutdown
```

**This terminates all running WSL distributions and their processes.** Reopen
the intended distribution afterward. Until systemd starts, configuring its
startup setting alone does not make service tasks applicable.

## Remove

Preview with the original profile/overlay while the sources still exist:

```powershell
.\dotfiles.ps1 remove --dry-run
```

If the saved selection changed, pass the original `--profile` and `--overlay`
explicitly. **Remove is not rollback:** Windows packages, Developer Mode,
registry values, and PATH changes remain. Review the
[removal behavior and limits](commands-and-usage.md#remove-integrations) before applying.
