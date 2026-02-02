# Windows Usage

Opinionated Windows automation layer for this dotfiles project. The PowerShell entrypoint wires together registry personalization, fonts, symlinks, and VS Code extensions in an idempotent fashion with profile-based filtering.

## Quick Start

Open an elevated PowerShell (most tasks require admin to write HKCU console keys, install fonts, and create symlinks in some protected locations) then run:

```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.ps1
```

Re‑run the script at any time; operations are skipped when already satisfied (fonts present, extensions installed, registry values unchanged, symlinks existing).

### Dry-Run Mode

Preview what would be changed without making any system modifications:

```powershell
./dotfiles.ps1 -DryRun
```

This automatically enables verbose output to show exactly what would be installed or configured.

### Uninstalling

To remove all dotfiles-managed symlinks (leaving registry settings, fonts, and VS Code extensions intact):

```powershell
./dotfiles-uninstall.ps1
```

This removes only symlinks that point to files in this repository. Add `-DryRun` to preview what would be removed.

## What the Script Does

`dotfiles.ps1` loads each module under `src/` and executes these functions in order:

| Step | Module | Function | Description | Idempotency Cue |
|------|--------|----------|-------------|-----------------|
| 1 | `Git.psm1` | `Update-GitSubmodules` | Initializes / updates all tracked submodules (fonts, vim plugins). | Only runs `git submodule update` if status indicates drift (`+` / `-`). |
| 2 | `Registry.psm1` | `Sync-Registry` | Applies registry values from `conf/registry.ini`. | Each value compared to existing; paths created only if missing. Includes improved error handling for permission issues. |
| 3 | `Font.psm1` | `Install-Fonts` | Installs fonts listed in `conf/fonts.ini`. | Skips if font already exists in system or per-user font directory. Includes error handling for installation failures. |
| 4 | `Symlinks.psm1` | `Install-Symlinks` | Creates Windows user profile symlinks from `conf/symlinks.ini` filtered by profile. | Only creates links whose targets do not already exist. Includes comprehensive error handling for parent directory creation and symlink creation. |
| 5 | `VsCodeExtensions.psm1` | `Install-VsCodeExtensions` | Ensures VS Code extensions listed in `conf/vscode-extensions.ini` are installed. | Checks against `code --list-extensions`. Supports profile-specific sections. Includes error reporting for failed installations. |

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

### VS Code Insiders Support

The Windows installation includes support for VS Code Insiders. The `AppData/Roaming/Code - Insiders` directory is a symlink to the regular `Code` directory, allowing both VS Code stable and Insiders to share the same configuration. This means:
- Settings and keybindings are synchronized between both versions
- You only need to maintain one set of configuration files
- Changes in one version appear in the other automatically

### Git Windows Configuration

The `config/git/windows` file contains Windows-specific Git settings:

```ini
[core]
  editor = code --wait
  autocrlf = true

[diff]
  tool = vscode
[difftool "vscode"]
  cmd = code --wait --diff $LOCAL $REMOTE
```

This file is symlinked to `~/.config/git/windows` and can be included in your main Git config with:

```ini
[include]
  path = ~/.config/git/windows
```

Or by running:

```powershell
git config --global include.path ~/.config/git/windows
```

To add a new link:
1. Place the source file under `symlinks/<path>` (create directories as needed).
2. Add the path to the `[windows]` section in `conf/symlinks.ini`.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `conf/vscode-extensions.ini` contains extensions organized by sections:

```ini
[extensions]
github.copilot
ms-vscode.powershell
# ... common extensions for all platforms

[windows]
ms-vscode-remote.remote-wsl
# ... Windows-specific extensions
```

Extensions are organized by profile sections:
- **`[extensions]`** - Common extensions installed on all platforms
- **`[windows]`** - Windows-specific extensions (e.g., WSL remote)
- **`[arch]`** or **`[desktop]`** - Platform-specific extensions for Linux

The script installs extensions for both `code` and `code-insiders` if available. Remove a line and re-run to keep new installs from occurring (does not uninstall). Add lines to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

**Error Handling:** Failed extension installations are now reported with warnings, allowing the script to continue with other extensions.

## Fonts

Font installation delegates to `extern/fonts/install.ps1` (a git submodule from powerline/fonts repository). Fonts are configured in `conf/fonts.ini` in the `[fonts]` section. The submodule is automatically updated when running `./dotfiles.ps1`.

The script includes error handling to report font installation failures while continuing with other fonts.

## Updating

Pull latest changes then re-run:

```powershell
git pull
./dotfiles.ps1
```

All submodules are checked and updated automatically when the script runs.

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No output / nothing changes | Ensure you are running an elevated PowerShell session. |
| Symlink not created | Entry present in `conf/symlinks.ini` under `[windows]` section? Does source file exist in `symlinks/`? Does a real file already exist at target path (preventing link)? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| Font not applied in terminal | Confirm the terminal profile is set to the installed font manually (script installs, but doesn't change terminal profile). |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* Script now includes comprehensive error handling for all operations (symlinks, fonts, VS Code extensions, registry)
* Failed operations are reported with warnings, allowing the script to continue
* Script does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur
* Registry error handling now distinguishes between "value doesn't exist" and permission errors
* Re-running is safe; modules emit section headers only when performing actions
* Use `-DryRun` to preview changes before applying them

## Extending Windows Layer

1. Create or modify module in `src/` exporting a function.
2. Add its invocation to `dotfiles.ps1` (maintain logical ordering: core prerequisites first, leaf operations last).
3. Keep functions self‑guarded (no-op if already configured) to preserve idempotency.