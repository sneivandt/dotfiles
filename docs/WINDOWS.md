# Windows Usage

Windows dotfiles management powered by the Rust core engine. The PowerShell entrypoint (`dotfiles.ps1`) is a thin wrapper that downloads (or builds) the Rust binary and forwards all arguments to it, providing registry personalization, symlinks, package installation, VS Code extensions, and GitHub Copilot CLI skills in an idempotent fashion.

**See Also:**
- [Usage Guide](USAGE.md) - General usage instructions
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Architecture](ARCHITECTURE.md) - Implementation details
- [Troubleshooting](TROUBLESHOOTING.md) - Windows-specific troubleshooting

## Quick Start

**Requirements:**
- **PowerShell**: Compatible with both PowerShell Core (pwsh) and Windows PowerShell (5.1+)
- **Administrator privileges**: Required for registry modification and symlink creation (not needed for dry-run mode)

The Windows profile is used by default when running on Windows.

### Initial Installation

Open an elevated PowerShell session (either PowerShell Core or Windows PowerShell) then run:

```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop

# Dry-run mode (preview changes without modification, no admin required)
.\dotfiles.ps1 install -p desktop -d

# With verbose output
.\dotfiles.ps1 install -p desktop -v

# Build and run from source (development)
.\dotfiles.ps1 -Build install -p desktop
```

Re‑run the script at any time; operations are skipped when already satisfied (extensions installed, registry values unchanged, symlinks existing).

## What the Binary Does

`dotfiles.ps1` downloads (or builds) the Rust binary and forwards all arguments to it. The binary runs the following tasks on Windows:

| Step | Task | Description | Idempotency Cue |
|------|------|-------------|-----------------|
| 1 | Developer Mode | Enables Windows developer mode (required for symlink creation). | Skips if already enabled. |
| 2 | Sparse Checkout | Configures git sparse checkout based on profile. | Skips if already configured. |
| 3 | Update Repository | Updates the repository from remote (`git pull --ff-only`). | Skips if already up to date. |
| 4 | Git Config | Configures git settings (e.g., `core.symlinks=true`, `core.autocrlf=false`). | Skips if already configured. |
| 5 | Git Hooks | Installs repository git hooks. | Skips if hooks already installed. |
| 6 | Packages | Installs missing packages from `conf/packages.toml` using winget. | Skips already-installed packages. |
| 7 | Symlinks | Creates Windows user profile symlinks from `conf/symlinks.toml`. | Only creates links whose targets do not already exist. |
| 8 | Registry | Applies registry values from `conf/registry.toml`. | Each value compared to existing; paths created only if missing. |
| 9 | VS Code Extensions | Installs VS Code extensions from `conf/vscode-extensions.toml`. | Checks against `code --list-extensions`. |
| 10 | Copilot Skills | Downloads GitHub Copilot CLI skills from `conf/copilot-skills.toml`. | Skips if skill files already exist. |

Tasks that don't apply to Windows (systemd, shell, chmod, paru) are automatically skipped via platform detection.

## Git Configuration

The repository contains symlinks (e.g., `symlinks/config/nvim` → `../vim`) that are tracked in Git. On Windows, creating actual symlinks during Git operations requires Developer Mode enabled or Administrator privileges.

The binary **automatically configures** Git to enable symlink support, since Developer Mode is enabled as a prior task:

```powershell
git config core.symlinks true
git config core.autocrlf false
git config credential.helper manager
```

This configuration:
- Enables proper symlink creation in the working directory
- Requires Developer Mode (enabled automatically by the first installation task)
- Is applied automatically on first run (idempotent—won't change if already set)

**Manual Workaround:** If you encounter symlink permission errors before running the script (e.g., during initial `git clone`), you can temporarily disable symlinks:
```powershell
git config core.symlinks false
```

## Automatic Repository Updates

The installation process includes an automatic repository update task that updates the dotfiles repository from the remote using a fast-forward-only merge:

```
git pull --ff-only
```

### Behavior

- **Clean, fast-forwardable**: Update proceeds normally
- **Already up to date**: Skips silently
- **Non-fast-forwardable**: The pull fails and the task reports an error — manual resolution is required
- **Dirty working tree**: Git will refuse the pull if there are conflicting changes

If the update fails, resolve the situation manually (e.g., commit or stash local changes, then re-run).

## Package Management

Package installation uses Windows Package Manager (winget) to install missing packages from `conf/packages.toml`.

**Requirements:**
- **winget**: Built into Windows 11 and modern Windows 10. If not available, install from: https://aka.ms/getwinget

Configuration lives in `conf/packages.toml` under the **`[windows]` section**:

```toml
[windows]
packages = [
  "Git.Git",
  "Microsoft.PowerShell",
  "Microsoft.VisualStudioCode",
]
```

Each entry is a **winget package ID** (case-sensitive). The binary:
- Checks if each package is already installed (idempotent)
- Installs only missing packages
- Uses silent installation with automatic acceptance of licenses
- Handles elevation automatically when packages require it

To find package IDs, use:
```powershell
winget search <package-name>
```

To add packages:
1. Add the winget package ID to the `[windows]` section in `conf/packages.toml`
2. Re-run `./dotfiles.ps1`

**Note:** Package installation respects the profile system. Only packages in sections matching your active categories (including auto-detected `windows`) will be installed.

## Registry Customization

Registry configuration lives in `conf/registry.toml`. Each section uses a logical name with a `path` key (the registry path) and a `[section.values]` subtable:

```toml
[console]
path = 'HKCU:\Console'

[console.values]
WindowSize = 0x00200078
FaceName = "Cascadia Mono"
QuickEdit = 1

[psreadline]
path = 'HKCU:\Console\PSReadLine'

[psreadline.values]
NormalForeground = 0xF
```

Section names are logical identifiers (not registry paths). The `path` field holds the actual registry path.

**Note:** Registry configuration doesn't use profile filtering since registry settings are Windows-only by nature.

To add custom registry settings:
1. Add entries to appropriate section in `conf/registry.toml` (or create a new section with a logical name and `path` key).
2. Re-run `./dotfiles.ps1`.

The script will create missing registry keys automatically.

## Symlinks

Symlink definitions live in `conf/symlinks.toml` under the **`[windows]` section**:

```toml
[windows]
symlinks = [
  "AppData/Roaming/Code/User/settings.json",
  "AppData/Roaming/Code - Insiders/User/settings.json",
  "AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
  "config/git/windows",
]
```

Each entry is a path relative to `$env:USERPROFILE`. The source file is located at `symlinks/<same-path>` in the repository. Forward slashes are automatically converted to backslashes for Windows.

### Smart Dot-Prefixing

Windows symlinks use intelligent dot-prefixing:
- **Well-known Windows folders** (AppData, Documents, etc.) remain as-is in the target path
- **Unix-style paths** (config, ssh, etc.) are automatically prefixed with a dot

Examples:
- `AppData/Roaming/Code/User/settings.json` → `%USERPROFILE%\AppData\Roaming\Code\User\settings.json`
- `config/git/config` → `%USERPROFILE%\.config\git\config`
- `ssh/config` → `%USERPROFILE%\.ssh\config`

This allows the same configuration repository to work across both Windows and Linux while respecting platform conventions.

**Note:** Windows symlinks use the same configuration file as Linux (`conf/symlinks.toml`) but use the `[windows]` section.

To add a new link:
1. Place the source file under `symlinks/<path>` (create directories as needed).
2. Add the path to the `[windows]` section in `conf/symlinks.toml`.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `conf/vscode-extensions.toml` contains extensions under category sections (`[desktop]`, `[windows]`, etc.). The `extensions` key holds an array of extension IDs. Remove an entry and re-run to keep new installs from occurring (does not uninstall). Add entries to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

## GitHub Copilot CLI Skills

The file `conf/copilot-skills.toml` contains GitHub Copilot CLI skill folder URLs organized by category sections (e.g., `[base]`, `[windows]`). Each URL points to a folder in a GitHub repository containing skill definition files.

**Format**:
```toml
[base]
skills = [
  "https://github.com/github/awesome-copilot/blob/main/skills/azure-devops-cli",
  "https://github.com/microsoft/skills/blob/main/.github/skills/azure-identity-dotnet",
]
```

**How it works**:
- Skills are downloaded to `$HOME/.copilot/skills/` directory
- The entire folder (including subdirectories) is downloaded recursively
- Each file is checked for changes before updating (idempotent)
- Skills extend GitHub Copilot CLI with additional context and functionality

**Requirements**:
- GitHub Copilot CLI (`gh copilot`) must be installed
- Skills are automatically downloaded during installation

## Updating

The dotfiles repository is automatically updated during installation. The binary handles local changes safely.

### Automatic Updates (Recommended)

Simply re-run the installer:

```powershell
.\dotfiles.ps1 install -p desktop
```

The binary automatically fetches and merges updates from remote using `git pull --ff-only`.

See the [Automatic Repository Updates](#automatic-repository-updates) section for details on conflict handling.

### Manual Updates

If you prefer to update manually:

```powershell
git pull
.\dotfiles.ps1 install -p desktop
```

Note: If the working tree has conflicting changes, commit or resolve them before pulling.

## Logging and Summary

The dotfiles installation includes a comprehensive logging system that tracks all operations and provides detailed summaries.

### Log File Location

All installation operations are logged to: `%LOCALAPPDATA%\dotfiles\install.log`

This persistent log file includes:
- Timestamp of installation
- Selected profile
- All operations performed
- Verbose details (even when not displayed on console)
- Summary statistics

The log file is useful for troubleshooting installation issues or reviewing what changes were made.

### Operation Counters

The installation tracks various operations and displays a summary at the end:

- **Packages installed**: Number of winget packages installed
- **Symlinks created**: Number of symlinks created in user profile
- **VS Code extensions installed**: Number of extensions installed
- **Copilot skills installed**: Number of GitHub Copilot CLI skills downloaded
- **Registry keys set**: Number of registry values modified

### Dry-Run Mode

When using `-d` (dry-run), the logging system:
- Shows what would be done without making changes
- Tracks counters for operations that would be performed
- Labels summary with "(would be)" suffix
- Still writes to the log file for review

Example summary output:
```
:: Installation Summary
Packages installed: 3
Symlinks created: 5
VS Code extensions installed: 2
Copilot skills installed: 2
Registry keys set: 12
Log file: C:\Users\YourName\AppData\Local\dotfiles\install.log
```

In dry-run mode:
```
:: Installation Summary
Packages installed (would be): 3
Symlinks created (would be): 5
Log file: C:\Users\YourName\AppData\Local\dotfiles\install.log
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No output / nothing changes | Ensure you are running an elevated PowerShell session. |
| Symlink not created | Entry present in `conf/symlinks.toml` under `[windows]` section? Does source file exist in `symlinks/`? Does a real file already exist at target path (preventing link)? Running as admin? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* **Cross-Edition Compatible**: Scripts work with both PowerShell Core and Windows PowerShell (5.1+)
* **Dry-Run Mode**: The `-d` (dry-run) flag allows previewing changes without administrator privileges
* The binary does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur
* Re-running is safe; tasks skip work that is already complete

## See Also

- [Configuration Reference](CONFIGURATION.md) - Details on `conf/packages.toml`, `conf/registry.toml`, `conf/symlinks.toml`
- [Usage Guide](USAGE.md) - General installation and usage
- [Troubleshooting](TROUBLESHOOTING.md) - Windows troubleshooting
- [Architecture](ARCHITECTURE.md) - Windows module architecture
- [Testing](TESTING.md) - Testing Windows changes
