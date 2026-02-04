# Windows Usage

Windows automation layer for this dotfiles project. The PowerShell entrypoint (`dotfiles.ps1`) always uses the fixed "windows" profile and wires together registry personalization, symlinks, and VS Code extensions in an idempotent fashion.

## Quick Start

**Requirements:**
- **PowerShell Core (pwsh)**: All PowerShell scripts require PowerShell Core edition
- **Administrator privileges**: Required for registry modification and symlink creation (not needed for dry-run mode)

The Windows script always uses the "windows" profile (profile selection is not available on Windows).

Open an elevated PowerShell Core session then run:

```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.ps1

# Dry-run mode (preview changes without modification, no admin required)
./dotfiles.ps1 -DryRun

# With verbose output
./dotfiles.ps1 -Verbose

# Dry-run with verbose output
./dotfiles.ps1 -DryRun -Verbose
```

Re‑run the script at any time; operations are skipped when already satisfied (extensions installed, registry values unchanged, symlinks existing).

## What the Script Does

`dotfiles.ps1` loads each module under `src/windows` and executes these functions in order:

| Step | Module | Function | Description | Idempotency Cue |
|------|--------|----------|-------------|-----------------|
| 1 | `Git.psm1` | `Initialize-GitConfig` | Configures Git to handle symlinks as text files on Windows. | Only sets `core.symlinks=false` if not already configured. |
| 2 | `Registry.psm1` | `Sync-Registry` | Applies registry values from `conf/registry.ini`. | Each value compared to existing; paths created only if missing. |
| 3 | `Symlinks.psm1` | `Install-Symlinks` | Creates Windows user profile symlinks from `conf/symlinks.ini` filtered by profile. | Only creates links whose targets do not already exist. |
| 4 | `VsCodeExtensions.psm1` | `Install-VsCodeExtensions` | Ensures VS Code extensions listed in `conf/vscode-extensions.ini` are installed. | Checks against `code --list-extensions`. |
## Git Configuration

The repository contains symlinks (e.g., `symlinks/config/nvim` → `../vim`) that are tracked in Git. On Windows, creating actual symlinks during Git operations requires either Developer Mode enabled or Administrator privileges.

To avoid permission errors during `git pull` and `git checkout`, the script **automatically configures** Git to treat symlinks as regular text files:

```powershell
git config --local core.symlinks false
```

This configuration:
- Prevents `error: unable to create symlink: Permission denied` during Git operations
- Stores symlink targets as plain text files in the working directory
- Is applied automatically on first run (idempotent—won't change if already set)
- Does not affect the actual Windows symlink creation by `Install-Symlinks` (which creates proper symlinks in your user profile)

**Manual Configuration:** If you encounter symlink errors before running the script (e.g., during initial `git clone` or `git pull`), run:
```powershell
git config core.symlinks false
```

## Registry Customization

Registry configuration lives in `conf/registry.ini` using INI format with **registry paths as sections**:

```ini
[HKCU:\Console\PSReadLine]
NormalForeground = 0xF

[HKCU:\Control Panel\International]
sLongDate = MMMM d, yyyy
```

Each section header is a registry path, and entries use `name = value` format. Color table entries (ColorTable00-15) use 6-digit hex RGB format that gets automatically converted to the internal DWORD format.

**Note:** Registry configuration doesn't use profile filtering since registry settings are Windows-only by nature.

To add custom registry settings:
1. Add entries to appropriate section in `conf/registry.ini` (or create a new section with a registry path).
2. Re-run `./dotfiles.ps1`.

The script will create missing registry keys automatically.

## Symlinks

Symlink definitions live in `conf/symlinks.ini` under the **`[windows]` section**:

```ini
[windows]
AppData/Roaming/Code/User/settings.json
AppData/Roaming/Code - Insiders/User/settings.json
AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json
config/git/windows
```

Each line is a path relative to `$env:USERPROFILE`. The source file is located at `symlinks/<same-path>` in the repository. Forward slashes are automatically converted to backslashes for Windows.

### Smart Dot-Prefixing

Windows symlinks use intelligent dot-prefixing:
- **Well-known Windows folders** (AppData, Documents, etc.) remain as-is in the target path
- **Unix-style paths** (config, ssh, etc.) are automatically prefixed with a dot

Examples:
- `AppData/Roaming/Code/User/settings.json` → `%USERPROFILE%\AppData\Roaming\Code\User\settings.json`
- `config/git/config` → `%USERPROFILE%\.config\git\config`
- `ssh/config` → `%USERPROFILE%\.ssh\config`

This allows the same configuration repository to work across both Windows and Linux while respecting platform conventions.

**Note:** Windows symlinks now share the same configuration file as Linux (`conf/symlinks.ini`) but use the `[windows]` section.

To add a new link:
1. Place the source file under `symlinks/<path>` (create directories as needed).
2. Add the path to the `[windows]` section in `conf/symlinks.ini`.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `conf/vscode-extensions.ini` contains extensions in the `[extensions]` section. Remove a line and re-run to keep new installs from occurring (does not uninstall). Add lines to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

## Updating

Pull latest changes then re-run:

```powershell
git pull
./dotfiles.ps1
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| "Requires PowerShell Core" error | Install PowerShell Core (pwsh) instead of using Windows PowerShell. Download from https://github.com/PowerShell/PowerShell |
| No output / nothing changes | Ensure you are running an elevated PowerShell Core session. |
| Symlink not created | Entry present in `conf/symlinks.ini` under `[windows]` section? Does source file exist in `symlinks/`? Does a real file already exist at target path (preventing link)? Running as admin? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* **PowerShell Core Required**: All scripts use `#Requires -PSEdition Core` to ensure compatibility with PowerShell Core
* **Dry-Run Mode**: The `-DryRun` parameter allows previewing changes without administrator privileges
* Script does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur
* Re-running is safe; modules emit section headers only when performing actions

## Extending Windows Layer

1. Create or modify module in `src/windows/` exporting a function.
2. Add its invocation to `dotfiles.ps1` (maintain logical ordering: Git config first, core prerequisites, then leaf operations).
3. Keep functions self‑guarded (no-op if already configured) to preserve idempotency.
