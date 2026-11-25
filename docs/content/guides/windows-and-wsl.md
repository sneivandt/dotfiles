---
title: Windows and WSL
description: Prepare Windows, handle elevation and symlink requirements, and understand Windows-side and Linux-side WSL configuration.
---

# Windows and WSL

Run setup from **unelevated PowerShell in your normal Windows account**. That
keeps home links and current-user registry settings in the intended account.
For Windows Subsystem for Linux (WSL), first [choose the side to configure](#wsl):
Windows owns the WSL 2 VM settings, but each distribution owns its own Linux
startup and PATH settings.

## Requirements

For the published x86-64 Windows release, use Windows PowerShell or PowerShell 7,
Git for the checkout, and GitHub CLI (`gh`) to verify release provenance. Have
WinGet available if you want the Windows packages. Rust/Cargo and native build
prerequisites are only needed for a [source build](../development/contributing.md).

## Bootstrap

Clone to a permanent path: home links and the launcher will refer back to it.
Review `conf/` and [back up existing home configuration](application-settings.md#back-up-and-restore-a-file)
before moving any conflicting files, directories, or links. For a fork, see
[Using a fork](getting-started.md#using-a-fork). The examples use the release
binary; pass `--build` right after `.\dotfiles.ps1` to compile locally instead.

Open PowerShell in the checkout and run a preview:

```powershell
.\dotfiles.ps1 update --no-repo-update --dry-run
```

`--dry-run` previews the tasks, not the wrapper: it still downloads and
verifies a missing CLI. `--no-repo-update` skips repository sync; a preview
can still write a log and save an interactive profile choice. Review the
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

If a link fails, use the reported path to choose a fix:

- **File-link permission:** enable Developer Mode, then retry from your normal
  user's terminal. Home links are never delegated to another account.
- **Occupied home destination:** back up the existing entry and move it aside.
  Elevation won't resolve the conflict.
- **Plain-text symlink in the checkout:** repair the source, not the home
  destination.

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

When Developer Mode is off, dotfiles can request UAC consent to run
`developer-mode` in a short-lived Administrator console. The parent run links
to the child's log. No other dotfiles task goes to that child; UAC might have
authenticated a different account. An already-enabled setting needs no prompt,
and `--dry-run` never requests elevation to apply it.

The original process checks file-link capability when `symlinks` runs. If it
still can't create file links, pending file links are skipped; directory links
and already-correct links can proceed.

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

From your normal terminal, preview the active WinGet package IDs:

```powershell
.\dotfiles.ps1 update --only packages --dry-run
```

WinGet tries user scope first. See [Package management](package-management.md#windows)
for installer scope fallback, upgrades, and failure handling. If an installer
requires Administrator rights and you approve a package-only elevated retry,
`--only packages` covers **all active Windows packages**, not just the failed ID.

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

Before applying, record or export the current values you may want back.
Dotfiles does not back them up or restore them on `remove`. Changes cover
console behavior, regional formatting, Explorer, search, and window defaults.
Some take effect only after an application restart or a new login.

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
can fix the named fragment and reload. `dot` forwards its arguments to
`dotfiles`. The profile sets `code` for VS Code Insiders and `ai` for Codex
when those commands are available; it preserves read-only and constant aliases.

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

- **Networking for all WSL 2 distributions:** In the Windows checkout, use
  PowerShell to [preview the symlinks task](#developer-mode-and-symlinks). It links
  [.wslconfig](https://github.com/sneivandt/dotfiles/blob/main/symlinks/wslconfig)
  into your Windows home to select NAT networking. It doesn't merge an
  existing file. Preserve any settings you need before moving it aside.
- **Startup and PATH for one distribution:** In that distribution's checkout,
  preview with `./dotfiles.sh update --only system-files --dry-run`. Review
  **all** selected system files before applying without `--dry-run`, not just
  the WSL entry. The task may prompt for
  `sudo` to merge the [wsl.conf fragment](https://github.com/sneivandt/dotfiles/blob/main/system/wsl.conf)
  into `/etc/wsl.conf`. It preserves unrelated INI keys, enables systemd, and
  disables automatic Windows PATH imports. Call Windows executables by full
  path or add only the directories you want.

**Save work in every running distribution before restarting WSL.** From Windows
PowerShell, shut down all WSL distributions so `/etc/wsl.conf` and `.wslconfig`
changes take effect:

```powershell
wsl --shutdown
```

Reopen the distribution and run `systemctl is-system-running` inside it.
Dotfiles accepts `running` or `degraded` for WSL service tasks. If the state
is `starting`, wait and check again; if it's `offline`, recheck the `[boot]`
setting in that distribution's `/etc/wsl.conf`.

If you use the `ssh-wsl` Zsh helper to link Windows SSH directories, it can
find `cmd.exe` under `C:\Windows` even without Windows PATH imports. For a
different installation, set `WSL_CMD_EXE` to the Linux path of `cmd.exe` or
`WINHOME` to the absolute Linux path of your Windows user profile.

## Remove

Preview removal using the original profile and overlay while the checkout's
source files are still intact:

```powershell
.\dotfiles.ps1 remove --dry-run
```

Pass the original `--profile` and `--overlay` explicitly if your saved
selection has changed. **Remove is not rollback:** a matching home link
becomes a copy of its current source, not the file that preceded it. Installed
Windows packages, Developer Mode, registry values, and `PATH` entries stay in
place. Review
[Remove integrations](commands-and-usage.md#remove-integrations) before
applying.

Next, read [Security and trust](security-and-trust.md) to review the tool's
trust boundaries, or jump to [Troubleshooting](troubleshooting.md) if you ran
into a Windows or WSL error.
