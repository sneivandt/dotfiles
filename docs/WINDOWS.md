# Windows guide

On Windows, the PowerShell wrapper bootstraps the Rust CLI. Platform tasks manage
Developer Mode, packages, symlinks, registry state, VS Code extensions, PATH,
and optional WSL configuration.

## Requirements

- 64-bit Windows on x86-64 for release-binary downloads
- PowerShell
- Git
- winget for package installation
- Rust only when using wrapper `--build`

The published Windows asset currently targets x86-64. On an unsupported
architecture, build the CLI locally instead.

## Bootstrap

```powershell
Set-Location C:\Code\sneivandt\dotfiles
.\dotfiles.ps1 install --profile desktop --dry-run
.\dotfiles.ps1 install --profile desktop
```

The wrapper looks for `bin\dotfiles.exe`. If absent, it downloads the matching
release asset and checksum, verifies SHA-256, and verifies build provenance
before execution.

To build from the current checkout:

```powershell
.\dotfiles.ps1 --build check
```

The CLI replaces its own binary in place during a self update. Windows allows a
running executable to be renamed, so installation renames the current binary to
a backup, moves the verified download into place, smoke tests it, and restores
the backup if that check fails. The updated binary is then run to completion in
the same console, so output stays sequential.

## Developer Mode and symlinks

**Home symlinks** depends on **Windows Developer Mode**, so the capability is
enabled before symlinks are provisioned.
Developer Mode allows normal users to create symbolic links without running the
entire CLI elevated.

If symlink creation still fails:

1. Confirm Windows Developer Mode is enabled.
2. Start a fresh shell so capability changes are visible.
3. Check that the target is not an unrelated existing file.
4. Run only the symlink task with verbose output:

```powershell
dotfiles install --only symlinks --dry-run --verbose
```

The CLI plans elevation when a mutation requires it; do not run every command as
Administrator by default.

## Elevation

A normal `dotfiles install` runs unelevated in your own terminal. Elevation is
requested per task, not per command, and only when the task's current state
actually needs it:

| Task | When it needs Administrator |
|---|---|
| **Windows Developer Mode** | first run only, while the machine policy value is unset |
| **Home symlinks** | only when Developer Mode is off and file links are missing |

Every other Windows task writes to user scope and never elevates.

When a task needs elevation, the CLI names the affected tasks and opens one UAC
prompt. It runs a short-lived elevated child restricted to those tasks with
`--only <selectors> --no-parallel`. The child gets a separate console window
and run log. The parent remains unelevated and continues.

Declining the prompt is not fatal. Those tasks are recorded as skipped and the
rest of the run proceeds, so a converged machine never sees a prompt at all.

In CI and other non-interactive sessions, the CLI does not open a prompt. It
skips tasks that need elevation and records the reason.

To perform a skipped step later, run it directly from an elevated shell:

```powershell
dotfiles install --only developer-mode
```

## Packages

Windows package identifiers in `conf/packages.toml` are winget package IDs:

```toml
[windows]
packages = [
  "Git.Git",
  "Microsoft.PowerShell",
]
```

Only missing packages are installed. AUR and paru tasks are not applicable on
Windows.

The CLI prints each package ID before installation, so long downloads can be
identified. It tries `--scope user` first, then retries without a scope only when
no per-user installer exists. This keeps most packages out of machine scope and
avoids a UAC prompt. A machine-scope installer can still open a consent dialog
that winget cannot suppress. Declining it, or hitting a policy restriction,
marks that package as skipped rather than failing the run. To install those
packages, rerun from an elevated shell:

```powershell
dotfiles install --only packages
```

## Registry settings

`conf/registry.toml` declares named paths and value tables. Current
configuration covers console colors and behavior, PSReadLine colors, regional
formatting, Explorer, taskbar, search, Start, desktop icons, and window
management.

```toml
[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

The task converges declared values without deleting undeclared values. Existing
taskbar and Start pins are intentionally not managed because Windows stores
them in unstable opaque formats.

Preview registry changes:

```powershell
dotfiles install --only registry --dry-run --verbose
```

Some Explorer settings are read only when Explorer or the user session
restarts.

## PATH and wrapper

The wrapper task installs `dotfiles` beneath:

```text
%USERPROFILE%\.local\bin
```

The PATH task adds that location to the user's persistent PATH when needed.
Open a new terminal after the first installation before relying on the bare
`dotfiles` command.

## PowerShell configuration

PowerShell profile files are delivered from the repository's `symlinks/` tree.
The active Windows category selects them, and normal symlink convergence keeps
the home targets pointed at the checkout.

`dotfiles check` attempts PSScriptAnalyzer whenever `pwsh` is available. If the
PSScriptAnalyzer module is missing, the PowerShell validation task fails and
reports the module error.

## WSL

When the Linux binary runs inside WSL, **WSL configuration files** enables
systemd and disables Windows PATH injection in `/etc/wsl.conf` while preserving
unrelated settings. The Windows executable on the host does not run this task.

Because `wsl.conf` is system-level:

- the operation may need elevation inside the distribution
- changes generally require `wsl --shutdown` from Windows before taking effect
- the task is not applicable on native Linux or the Windows host

## Uninstall

```powershell
dotfiles uninstall --dry-run
dotfiles uninstall
```

Uninstall materializes managed links and removes the hook and wrapper. It does
not uninstall winget packages or restore registry values.
