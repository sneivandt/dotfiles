# Windows Usage

Windows automation layer for this dotfiles project. The PowerShell entrypoint (`dotfiles.ps1`) always uses the fixed "windows" profile and wires together registry personalization, symlinks, and VS Code extensions in an idempotent fashion.

**New:** The dotfiles installer now installs itself as a PowerShell module, making the `Install-Dotfiles` and `Update-Dotfiles` commands available from anywhere in PowerShell.

## Quick Start

**Requirements:**
- **PowerShell**: Compatible with both PowerShell Core (pwsh) and Windows PowerShell (5.1+)
- **Administrator privileges**: Required for registry modification and symlink creation (not needed for dry-run mode)

The Windows script always uses the "windows" profile (profile selection is not available on Windows).

### Initial Installation

Open an elevated PowerShell session (either PowerShell Core or Windows PowerShell) then run:

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

### Using the Module Commands

After the initial installation, the dotfiles are available as PowerShell module commands:

```powershell
# Install or update dotfiles from anywhere
Install-Dotfiles

# Preview changes without modification
Install-Dotfiles -DryRun -Verbose

# Update repository and re-install (with automatic stashing)
Update-Dotfiles

# Get help
Get-Help Install-Dotfiles -Full
Get-Help Update-Dotfiles -Full
```

The module commands can be run from any directory without needing to navigate to the dotfiles repository.

Re‑run the script or commands at any time; operations are skipped when already satisfied (extensions installed, registry values unchanged, symlinks existing).

## What the Script Does

`dotfiles.ps1` loads each module under `src/windows` and executes these functions in order:

| Step | Module | Function | Description | Idempotency Cue |
|------|--------|----------|-------------|-----------------|
| 1 | `Git.psm1` | `Initialize-GitConfig` | Configures Git to handle symlinks as text files on Windows. | Only sets `core.symlinks=false` if not already configured. |
| 2 | `Git.psm1` | `Update-DotfilesRepository` | Updates the repository from remote with automatic stashing of local changes. | Skips if already up to date; stashes and re-applies local changes automatically. |
| 3 | `GitHooks.psm1` | `Install-RepositoryGitHooks` | Installs git hooks for the repository. | Skips if hooks already installed. |
| 4 | `Module.psm1` | `Install-DotfilesModule` | Installs the Dotfiles PowerShell module to the user's modules directory. | Skips if module already installed and up to date. |
| 5 | `Packages.psm1` | `Install-Packages` | Installs missing packages from `conf/packages.ini` using winget. | Skips already-installed packages. |
| 6 | `Registry.psm1` | `Sync-Registry` | Applies registry values from `conf/registry.ini`. | Each value compared to existing; paths created only if missing. |
| 7 | `Symlinks.psm1` | `Install-Symlinks` | Creates Windows user profile symlinks from `conf/symlinks.ini` filtered by profile. | Only creates links whose targets do not already exist. |
| 8 | `VsCodeExtensions.psm1` | `Install-VsCodeExtensions` | Ensures VS Code extensions listed in `conf/vscode-extensions.ini` are installed. | Checks against `code --list-extensions`. |
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

## Automatic Repository Updates

The installation process includes an automatic repository update step (`Update-DotfilesRepository`) that safely updates the dotfiles repository from the remote with robust handling of local changes:

### How It Works

1. **Detects Changes**: Checks for any staged, unstaged, or untracked files in the working tree
2. **Stashes Automatically**: If changes are found, creates a timestamped stash before updating
3. **Fetches and Merges**: Fetches from origin and merges updates from the remote branch
4. **Re-applies Changes**: Automatically re-applies the stash after a successful update
5. **Clear Error Messages**: Provides detailed guidance if manual intervention is needed

### Behavior

- **Clean working tree**: Updates proceed normally with no stashing
- **Dirty working tree**: Changes are automatically stashed, repository is updated, then changes are re-applied
- **Merge conflicts**: If conflicts occur during merge or stash re-application, the operation is aborted and you receive clear instructions on how to resolve manually
- **Already up to date**: Skips update if local HEAD matches remote HEAD

### Manual Resolution

If automatic stash re-application fails due to conflicts, you'll see a message like:

```
WARNING: Successfully updated dotfiles, but failed to re-apply your stashed changes
due to conflicts. Your changes are preserved in stash: dotfiles-auto-stash-2024-02-08_12-30-45

To resolve this manually:
    1. Review the conflicts: git status
    2. Manually apply the stash and resolve conflicts:
       git stash apply stash^{/dotfiles-auto-stash-2024-02-08_12-30-45}
    3. Resolve any conflicts in the affected files
    4. Once resolved, drop the stash:
       git stash drop stash^{/dotfiles-auto-stash-2024-02-08_12-30-45}
```

This ensures your local changes are never lost while keeping the update process safe and automatic.

## Package Management

Package installation uses Windows Package Manager (winget) to install missing packages from `conf/packages.ini`.

**Requirements:**
- **winget**: Built into Windows 11 and modern Windows 10. If not available, install from: https://aka.ms/getwinget

Configuration lives in `conf/packages.ini` under the **`[windows]` section**:

```ini
[windows]
Git.Git
Microsoft.PowerShell
Microsoft.VisualStudioCode
```

Each line is a **winget package ID** (case-sensitive). The script:
- Checks if each package is already installed (idempotent)
- Installs only missing packages
- Uses silent installation with automatic acceptance of licenses
- Handles elevation automatically when packages require it

To find package IDs, use:
```powershell
winget search <package-name>
```

To add packages:
1. Add the winget package ID to the `[windows]` section in `conf/packages.ini`
2. Re-run `./dotfiles.ps1`

**Note:** Package installation respects the profile system. Only packages in sections not excluded by the "windows" profile will be installed.

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

The dotfiles repository is automatically updated during installation via the `Update-DotfilesRepository` function, which handles local changes safely.

### Automatic Updates (Recommended)

Simply re-run the installer or use the module command:

```powershell
# Using the module command (available anywhere)
Update-Dotfiles

# Or re-run the installer script
./dotfiles.ps1
```

Both methods automatically:
- Stash any local changes
- Fetch and merge updates from remote
- Re-apply your local changes

See the [Automatic Repository Updates](#automatic-repository-updates) section for details on conflict handling.

### Manual Updates

If you prefer to update manually:

```powershell
git pull
./dotfiles.ps1
```

Note: Manual `git pull` may require stashing your changes first if the working tree is dirty.

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No output / nothing changes | Ensure you are running an elevated PowerShell session. |
| Symlink not created | Entry present in `conf/symlinks.ini` under `[windows]` section? Does source file exist in `symlinks/`? Does a real file already exist at target path (preventing link)? Running as admin? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* **Cross-Edition Compatible**: Scripts work with both PowerShell Core and Windows PowerShell (5.1+)
* **Dry-Run Mode**: The `-DryRun` parameter allows previewing changes without administrator privileges
* Script does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur
* Re-running is safe; modules emit section headers only when performing actions

## Extending Windows Layer

1. Create or modify module in `src/windows/` exporting a function.
2. Add its invocation to `dotfiles.ps1` (maintain logical ordering: Git config first, core prerequisites, then leaf operations).
3. Keep functions self‑guarded (no-op if already configured) to preserve idempotency.
