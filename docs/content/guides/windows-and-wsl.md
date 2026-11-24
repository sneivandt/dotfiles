---
title: Windows and WSL
description: Prepare Windows, handle elevation and symlink requirements, and understand Windows-side and Linux-side WSL configuration.
---

# Windows and WSL

Always run dotfiles from your **normal Windows account** rather than an
Administrator shell; dotfiles requests elevation only for the narrow machine
tasks that require it. Windows and WSL keep separate home directories, so you
configure each side independently.

If you're configuring WSL, start by checking [which side owns the setting](#wsl).
For cross-platform commands and flags, see
[Commands and usage](commands-and-usage.md).

## Requirements

| Requirement | Why |
|---|---|
| x86-64 Windows | The published Windows release asset targets x86-64 |
| PowerShell | Runs `dotfiles.ps1` (both Windows PowerShell and `pwsh` are supported) |
| Git | Checkout, repository sync, hooks, and saved selections |
| WinGet | Installs declared Windows packages |
| GitHub CLI (`gh`) | Verifies release build provenance during bootstrap and self-update |
| Rust/Cargo and native build prerequisites | Only needed for a [source build](../development/contributing.md) |

Clone the checkout to a permanent path, since home symlinks and the installed
launcher point straight back to it. Review `conf/` and **back up existing home
configuration** before applying, moving aside any existing files, directories,
or unrelated links that would conflict.

## Bootstrap

Clone and review the repository following [Getting started](getting-started.md)
(or configure a fork or source build under
[Using a fork](getting-started.md#using-a-fork)). The examples below use
published release binaries; pass `--build` right after `.\dotfiles.ps1` to
compile locally instead.

Open PowerShell in the checkout and run a preview:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

The wrapper downloads and verifies the CLI automatically if needed. Review the
Windows symlink and elevation behavior below, then work through the
[preview](getting-started.md#3-preview-the-selected-work) and
[apply](getting-started.md#4-apply-what-you-reviewed) steps. The rest of this
guide uses `.\dotfiles.ps1` so commands work even before `dotfiles` is on
`PATH`.

### Download and update failures

Never disable checksum or provenance verification to push past a failed
download; start with [Wrapper download fails](troubleshooting.md#wrapper-download-fails).

Windows locks executables while they are running, which can prevent replacing
an in-use binary during self-update. If self-update fails, read the error
carefully before retrying or moving files. See the
[CLI self-update reference](../reference/cli-reference.md#cli-self-update) for
verification, post-update version checks, and recovery.

## Developer Mode and symlinks

Enabling Windows Developer Mode lets your normal user account create file
symlinks without Administrator privileges. Dotfiles can turn it on for you via
a short-lived elevated task. Directory links can fall back to junctions when
Developer Mode is off, and already-correct links need no elevation at all.

Include the Developer Mode task when previewing link setup:

```powershell
.\dotfiles.ps1 list --graph update --only symlinks --with-deps
.\dotfiles.ps1 update --only symlinks --with-deps --dry-run
```

Running `--only symlinks` without `--with-deps` skips the Developer Mode task
while still running the symlink task's own capability checks, so directory
links and already-correct links can still proceed even if Developer Mode isn't
enabled yet.

When a link fails, distinguish the three common causes:

| Problem | What to do |
|---|---|
| File-link creation is not permitted | Enable Developer Mode, then retry from your normal user's terminal |
| A home target conflicts | Back it up and move it aside before retrying |
| Git checked out a source symlink as plain text | Repair the source checkout rather than the home target |

To check for plain-text symlink placeholders without changing anything:

```powershell
git config --show-origin --get core.symlinks
git ls-files --stage -- symlinks | Select-String '^120000 '
```

Git mode `120000` marks a symlink. Compare each reported source's actual
filesystem type against that mode (an unset `core.symlinks` alone doesn't prove
a checkout is broken). Turning on Developer Mode later won't automatically
convert existing text placeholders into real links: preserve any local edits,
enable Git symlink support, and either restore just the affected sources or use
a fresh symlink-capable clone. Dotfiles rejects detected placeholders rather
than linking plain-text stubs as live configuration.

## Elevation

Dotfiles runs your main session unelevated and launches a short-lived
Administrator child only for machine-scoped work that requires it:

| Task | When it needs Administrator rights |
|---|---|
| `developer-mode` | The machine Developer Mode setting is not yet enabled |

The elevated child opens its own console and writes its own log linked from the
parent run. Work that is already current never prompts, and `--dry-run` never
requests elevation to apply changes.

Only `developer-mode` is delegated to the elevated child. Because UAC can
authenticate as a different Administrator account, home symlinks and user
registry settings always stay in your original unelevated process. Right after
`developer-mode` runs, `symlinks` rechecks file-link capability; if it is still
unavailable, pending file links are skipped while directory links and
already-correct links continue.

- Declined UAC prompts or unavailable prompting leave affected tasks skipped.
  Blocking dependents stop, while independent tasks continue.
- Under `--fail-on-skip` or in CI, those skips cause the command to fail.
- A child process that fails or cannot start is reported as a failure, not
  declined consent.

**WinGet installers are separate:** individual installers can still raise their
own UAC prompts even though dotfiles runs the package task unelevated.

If you prefer to enable Developer Mode yourself from an Administrator terminal,
scope the command to that single task:

```powershell
.\dotfiles.ps1 update --only developer-mode
```

Then switch back to your normal terminal for everything else, since running a
full update under a different Administrator account would target that
account's home directory and registry hive.

## Packages

Windows installs missing packages through WinGet. See
[Package management](package-management.md#windows) for exact IDs, user-to-machine
scope fallback, upgrades, and failure handling.

```powershell
.\dotfiles.ps1 update --only packages --dry-run
```

If an installer genuinely requires Administrator rights and you approve it, run
a package-only elevated retry, keeping in mind that `--only packages` covers
**all active packages**, not one specific ID.

## Registry settings

The `registry` task applies current-user (`HKCU`) values from
`conf/registry.toml` while leaving undeclared values alone. Notice that the
tables in `conf/registry.toml` are logical settings groups rather than profile
categories, so review them even when using `base`. See
[User registry settings](../reference/user-registry-settings.md) for value
types and schema details.

```powershell
.\dotfiles.ps1 update --only registry --dry-run
```

Managed settings cover console behavior, regional formatting, Explorer, search,
and window defaults. Dotfiles does not back up prior registry values or restore
them on `remove`, and some settings only take effect after restarting the
affected application or signing out and back in.

<details>
<summary>Legacy ForegroundLockTimeout note</summary>

Older checkouts set `ForegroundLockTimeout` to `0` under
`HKCU\Control Panel\Desktop` to let apps take focus. Current updates leave that
value alone; inspect or reset it in Registry Editor if you applied an older
checkout.

</details>

## PATH and wrapper

The launcher task creates `%USERPROFILE%\.local\bin\dotfiles.cmd`, and the PATH
task adds `%USERPROFILE%\.local\bin` to your persistent user `PATH`. The
launcher invokes this checkout's `dotfiles.ps1`, preferring `pwsh` over Windows
PowerShell when installed.

Open a new terminal to verify it:

```powershell
Get-Command dotfiles -All
Test-Path "$env:USERPROFILE\.local\bin\dotfiles.cmd"
```

If `PATH` hasn't refreshed yet, keep calling `.\dotfiles.ps1` directly. If you
have multiple checkouts, remember that `--root` selects configuration rather
than the binary, so invoke the desired checkout's wrapper when both matter.

## PowerShell configuration

The Windows symlink declarations map your PowerShell profile to these
home-relative paths:

| Shell | Managed destination |
|---|---|
| PowerShell 7 | `Documents/PowerShell/Microsoft.PowerShell_profile.ps1` |
| Windows PowerShell | `Documents/WindowsPowerShell/Microsoft.PowerShell_profile.ps1` |

Because these are fixed home-relative paths rather than dynamic Windows Known
Folder lookups, check what each shell actually uses on your machine:

```powershell
$PROFILE.CurrentUserCurrentHost
Test-Path -LiteralPath $PROFILE.CurrentUserCurrentHost
```

OneDrive-redirected Documents folders or embedded hosts like VS Code can look
in a different path. If yours differs, update the home-relative destination in
your [symlink declarations](../reference/home-symlinks.md#configuration) before
applying (absolute target paths aren't supported, and parent links pointing
outside your home require explicit external-target permission). Always keep one
shared tracked profile instead of maintaining a second copy.

Edit the tracked
[Microsoft.PowerShell_profile.ps1](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/powershell/Microsoft.PowerShell_profile.ps1),
not the installed copy. It configures a path- and Git-aware prompt, command
history, and predictive suggestions when PSReadLine supports them. If any
startup fragment errors, the profile warns and keeps loading the rest so you
can fix the named fragment and reload. `dot` aliases `dotfiles`, and `code`
points to VS Code Insiders when available (leaving read-only or constant `code`
and `ai` aliases intact).

Open a new PowerShell window to pick up edits. Tab completion for `dotfiles`
itself is installed separately by the
[Shell completions task](commands-and-usage.md#shell-completions).

## Windows Terminal

The `windows-desktop` category links
[settings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json)
into Windows Terminal's packaged-app `LocalState` directory. It defaults to
PowerShell 7 (`pwsh.exe`) and includes an Arch WSL profile; if either isn't
installed on your machine, edit the tracked JSON before applying. Changes made
in Windows Terminal's settings UI may write back through the symlink, so check
`git diff` afterward.

## WSL

| To configure | Run dotfiles in | What this checkout changes |
|---|---|---|
| WSL 2 networking for all distributions | Windows PowerShell, using `.\dotfiles.ps1` | Links [.wslconfig](https://github.com/sneivandt/dotfiles/blob/main/symlinks/wslconfig) into your Windows home to select NAT networking |
| Startup and PATH for one distribution | A Linux shell inside that distribution, using `./dotfiles.sh` | Merges the [wsl.conf fragment](https://github.com/sneivandt/dotfiles/blob/main/system/wsl.conf) into `/etc/wsl.conf` to enable systemd and turn off automatic Windows PATH imports |

The Windows CLI does not touch `/etc/wsl.conf`. Inside Linux, the
`system-files` task preserves unrelated INI keys and may prompt for `sudo`.
With automatic Windows PATH injection disabled, call Windows executables by
full path or add only the specific directories you want.

`ssh-wsl` locates your Windows profile at runtime and can find `cmd.exe` under
`C:\Windows` even with Windows PATH injection off. If Windows is installed
elsewhere, set `WSL_CMD_EXE` to the Linux path of `cmd.exe` or `WINHOME` to the
Linux path of your Windows user profile.

**Save work in every running distribution before restarting WSL.** From Windows
PowerShell, shut down all WSL distributions so `/etc/wsl.conf` and `.wslconfig`
changes take effect:

```powershell
wsl --shutdown
```

Reopen your distribution afterward. Until systemd is actually running inside
WSL, systemd service tasks remain inapplicable.

## Remove

Preview removal using the original profile and overlay while the checkout's
source files are still intact:

```powershell
.\dotfiles.ps1 remove --dry-run
```

Pass the original `--profile` and `--overlay` explicitly if your saved
selection has changed. **Remove is not rollback:** installed Windows packages,
Developer Mode, registry values, and `PATH` entries stay in place. Review
[Remove integrations](commands-and-usage.md#remove-integrations) before
applying.

Next, read [Security and trust](security-and-trust.md) to review the tool's
trust boundaries, or jump to [Troubleshooting](troubleshooting.md) if you ran
into a Windows or WSL error.
