# Windows Guide

Windows is a first-class target. The PowerShell wrapper bootstraps the Rust CLI,
and platform tasks converge Developer Mode, packages, symlinks, registry state,
VS Code extensions, PATH, and optional WSL configuration.

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
release asset and checksum and verifies SHA-256 before execution.

To build from the current checkout:

```powershell
.\dotfiles.ps1 --build test
```

The wrapper can also replace itself through a pending self-update. Installation
uses a rollback-safe replacement sequence so a failed update does not leave the
entry point missing.

## Developer Mode and symlinks

**Install symlinks** depends on **Enable developer mode**, so the capability is
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

## Packages

Windows package identifiers in `conf\packages.toml` are winget package IDs:

```toml
[windows]
packages = [
  "Git.Git",
  "Microsoft.PowerShell",
]
```

Only missing packages are installed. AUR and paru tasks are not applicable on
Windows.

## Registry settings

`conf\registry.toml` declares named paths and value tables. Current
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

PowerShell profile files are delivered from the repository's `symlinks\` tree.
The active Windows category selects them, and normal symlink convergence keeps
the home targets pointed at the checkout.

`dotfiles test` attempts PSScriptAnalyzer whenever `pwsh` is available. If the
PSScriptAnalyzer module is missing, the PowerShell validation task fails and
reports the module error.

## WSL

When the Linux binary runs inside WSL, **Configure WSL** enables systemd and
disables Windows PATH injection in `/etc/wsl.conf` while preserving unrelated
settings. This is separate from running the Windows executable on the host.

Because `wsl.conf` is system-level:

- the operation may need elevation inside the distribution
- changes generally require `wsl --shutdown` from Windows before taking effect
- the task is not applicable on native Linux or the Windows host

## Uninstall

```powershell
dotfiles uninstall --dry-run
dotfiles uninstall
```

Uninstall materializes managed links and removes the hook and wrapper
integrations. It does not uninstall winget packages or restore registry values.
Those boundaries prevent broad destructive rollback of shared machine state.
