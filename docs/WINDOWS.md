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
.\dotfiles.ps1 --build install -p desktop
```

Re‑run the script at any time; operations are skipped when already satisfied (extensions installed, registry values unchanged, symlinks existing).

## What the Binary Does

`dotfiles.ps1` downloads (or builds) the Rust binary and forwards all arguments to it. The binary runs the following tasks on Windows:

| Phase | Step | Task | Description | Idempotency Cue |
|-------|------|------|-------------|-----------------|
| System | 1 | Self-Update | Updates the dotfiles binary from latest GitHub release. | Skips if already up to date. |
| System | 2 | Developer Mode | Enables Windows developer mode (required for symlink creation). | Skips if already enabled. |
| System | 3 | Sparse Checkout | Configures git sparse checkout based on profile. | Skips if already configured. |
| System | 4 | Update Repository | Updates the repository from remote (`git pull --ff-only`). | Skips if already up to date. |
| System | 5 | Git Hooks | Installs repository git hooks. | Skips if hooks already installed. |
| System | 6 | Configure PATH | Ensures dotfiles bin directory is on PATH. | Skips if already on PATH. |
| User | 7 | Packages | Installs missing packages from `conf/packages.toml` using winget. | Skips already-installed packages. |
| User | 8 | Symlinks | Creates Windows user profile symlinks from `conf/symlinks.toml`. | Only creates links whose targets do not already exist. |
| User | 9 | Git Config | Configures git settings (e.g., `core.symlinks=true`, `core.autocrlf=false`). | Skips if already configured. |
| User | 10 | Registry | Applies registry values from `conf/registry.toml`. | Each value compared to existing; paths created only if missing. |
| User | 11 | VS Code Extensions | Installs VS Code extensions from `conf/vscode-extensions.toml`. | Checks against `code --list-extensions`. |
| User | 12 | Copilot Plugins | Registers configured Copilot marketplaces and installs plugins from `conf/copilot-plugins.toml`. | Skips if the plugin is already installed. |

Tasks that don't apply to Windows (systemd, shell, chmod, paru) are automatically skipped via platform detection.

## Git Configuration

The repository contains symlinks (e.g., `symlinks/config/nvim` → `../vim`) that are tracked in Git. On Windows, creating actual symlinks during Git operations requires Developer Mode enabled or Administrator privileges.

The binary **automatically configures** Git to enable symlink support, since Developer Mode is enabled during the System phase:

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
  { source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" },
  { source = "AppData/Roaming/Code - Insiders/User/settings.json", target = "AppData/Roaming/Code - Insiders/User/settings.json" },
  { source = "AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json", target = "AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json" },
  "config/git/windows",
]
```

Each entry is a path relative to `$env:USERPROFILE`. The source file is located at `symlinks/<same-path>` in the repository. Forward slashes are automatically converted to backslashes for Windows.

### Target Path Handling

By default, a symlink entry `"foo/bar"` maps to `%USERPROFILE%\.foo\bar` (a dot is prepended). For Windows paths that must **not** receive a dot prefix — such as `AppData\` or `Documents\` — specify an explicit `target` field:

```toml
{ source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" }
```

This maps `symlinks/AppData/Roaming/Code/User/settings.json` →
`%USERPROFILE%\AppData\Roaming\Code\User\settings.json`.

Unix-style paths (no explicit target) continue to receive the dot prefix automatically:
- `"config/git/windows"` → `%USERPROFILE%\.config\git\windows`
- `"ssh/config"` → `%USERPROFILE%\.ssh\config`

**Note:** Windows symlinks use the same configuration file as Linux (`conf/symlinks.toml`) but use the `[windows]` section.

To add a new link:
1. Place the source file under `symlinks/<path>` (create directories as needed).
2. Add the path to the `[windows]` section in `conf/symlinks.toml`. Use a plain string for Unix-style paths (dot prefix applied automatically) or `{ source, target }` for Windows paths that need no dot prefix.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `conf/vscode-extensions.toml` contains extensions under category sections (`[desktop]`, `[windows]`, etc.). The `extensions` key holds an array of extension IDs. Remove an entry and re-run to keep new installs from occurring (does not uninstall). Add entries to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

## GitHub Copilot CLI Plugins

The file `conf/copilot-plugins.toml` contains GitHub Copilot CLI plugins organized by category sections (e.g., `[base]`, `[windows]`). Each entry specifies the marketplace repository, the marketplace name used by the CLI, and the plugin to install.

**Format**:
```toml
[base]
plugins = [
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-diag" },
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-msbuild" },
]
```

**How it works**:
- The task runs `gh copilot plugin marketplace add <marketplace>` when the marketplace is not already registered
- Plugins are installed with `gh copilot plugin install <plugin>@<marketplace_name>`
- Installed plugins are detected via `gh copilot plugin list`
- Copilot skips reinstalling plugins that are already present

**Requirements**:
- GitHub CLI with the Copilot extension (`gh copilot`) must be installed
- Plugins are automatically installed during installation

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

### Task Summary

The installation displays a summary of all tasks grouped by phase:

- `✓` — task completed successfully (green)
- `·` — not applicable on this platform/profile (dim)
- `○` — deliberately skipped (yellow)
- `~` — dry-run preview (white)
- `✗` — task failed (red)

### Dry-Run Mode

When using `-d` (dry-run), the logging system:
- Shows what would be done without making changes
- Marks tasks with `~` (dry-run) in the summary
- Still writes to the log file for review

Example summary output:
```
:: Summary
   System
     ✓ Self-update
     ✓ Enable developer mode
     ✓ Configure sparse checkout
     ✓ Update repository
     ✓ Install git hooks
   User
     ✓ Install packages
     ✓ Install symlinks
     ✓ Configure Git
     ✓ Apply registry settings
     ✓ Install VS Code extensions
     ✓ Install Copilot plugins

   11 tasks: 11 ok, 0 n/a, 0 skipped, 0 dry-run, 0 failed
   log: C:\Users\YourName\AppData\Local\dotfiles\install.log
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
