# Usage Guide

Comprehensive guide to using the dotfiles installation and management system.

## Installation

### Linux

**Basic installation with profile selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
```

**Interactive profile selection (first time):**
```bash
./dotfiles.sh install
# You'll be prompted to select from available profiles
# Your selection is saved for future runs
```

**Subsequent runs (uses saved profile):**
```bash
./dotfiles.sh install
# Automatically uses the profile you selected previously
```

### Windows

**Initial installation:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

## Command Line Options

### Linux (`dotfiles.sh`)

**Synopsis:**
```
dotfiles.sh [--build] install   [-p PROFILE] [-d] [-v]
dotfiles.sh [--build] uninstall [-p PROFILE] [-d] [-v]
dotfiles.sh [--build] test      [-p PROFILE] [-v]
dotfiles.sh [--build] version
```

**Options:**

- **`install`** - Install dotfiles and configure system
- **`uninstall`** - Remove installed dotfiles (managed symlinks)
- **`test`** - Run configuration validation
- **`version`** - Print version information
- **`--build`** - Build and run from source (requires `cargo`)
- **`-p, --profile PROFILE`** - Use specific profile (base, desktop)
- **`-v, --verbose`** - Enable verbose logging
- **`-d, --dry-run`** - Preview changes without modifying system (auto-enables verbose)

### Windows (`dotfiles.ps1`)

**Synopsis:**
```powershell
.\dotfiles.ps1 [-Build] install -p desktop [-d] [-v]
.\dotfiles.ps1 [-Build] uninstall [-d]
.\dotfiles.ps1 [-Build] test
.\dotfiles.ps1 [-Build] version
```

**Parameters:**

- **`-Build`** - Build and run from source (requires `cargo`)
- **`-p PROFILE`** - Use specific profile (base, desktop)
- **`-d`** - Preview changes without applying (dry-run)
- **`-Verbose`** - Enable verbose logging

## Common Workflows

### First-Time Setup

**Linux with interactive selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install
# Select profile when prompted
# Installation proceeds with your selection
```

**Linux with explicit profile:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
```

**Windows:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

### Updating Dotfiles

**Linux (uses saved profile):**
```bash
cd ~/dotfiles
git pull
./dotfiles.sh install
```

**Windows:**
```powershell
cd ~\dotfiles
git pull
.\dotfiles.ps1 install -p desktop
```

### Previewing Changes

**Before running installation:**
```bash
./dotfiles.sh install -p desktop -d
# Shows what would be done without making changes
# Verbose mode is automatically enabled
```

**Windows:**
```powershell
.\dotfiles.ps1 install -p desktop -d
```

### Switching Profiles

**Change profile and apply changes:**
```bash
./dotfiles.sh install -p base
# Switches to base profile
# Removes desktop-specific symlinks and files
# New profile is saved for future runs
```

### Uninstalling

**Remove all managed symlinks:**
```bash
./dotfiles.sh uninstall
# Removes symlinks created by dotfiles
# Does not remove packages or other configuration
```

**Preview uninstallation:**
```bash
./dotfiles.sh uninstall -d
```

### Testing

**Run configuration validation:**
```bash
./dotfiles.sh test
# Validates TOML file syntax
# Checks profile definitions
# Verifies configuration references
```

### Checking Version

```bash
./dotfiles.sh version
# Prints the installed binary version
```

## What Gets Installed

The installation process handles different components based on your profile:

### Linux Installation Steps

**Bootstrap** (prepare the environment):

1. **Self-Update** - Updates the dotfiles binary from latest GitHub release
2. **Configure Sparse Checkout** - Excludes files based on profile
3. **Update Repository** - Pulls latest changes (`git pull --ff-only`)
4. **Install Git Hooks** - Symlinks repository git hooks
5. **Install Wrapper** - Installs `dotfiles` wrapper to `~/.local/bin/`
6. **Configure PATH** - Ensures `~/.local/bin` is on PATH

**Configure** (apply declared state):

7. **Install Packages** - Installs packages from `conf/packages.toml` using pacman
8. **Install Paru** - Bootstraps paru AUR helper (Arch Linux only)
9. **Install AUR Packages** - Installs AUR packages via paru (Arch Linux only)
10. **Create Symlinks** - Links files from `symlinks/` to `$HOME`
11. **Set Permissions** - Applies file permissions from `conf/chmod.toml`
12. **Configure Git** - Applies git configuration
13. **Configure Shell** - Sets default shell
14. **Enable Systemd Units** - Enables and starts user units from `conf/systemd-units.toml`
15. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.toml`
16. **Install Copilot Plugins** - Registers configured marketplaces and installs GitHub Copilot CLI plugins from `conf/copilot-plugins.toml`
17. **Write wsl.conf** - Writes `/etc/wsl.conf` with `generateResolvConf = true` under `[network]` (WSL only, via sudo when not root)

### Windows Installation Steps

**Bootstrap** (prepare the environment):

1. **Self-Update** - Updates the dotfiles binary from latest GitHub release
2. **Enable Developer Mode** - Enables Windows developer mode (required for symlinks)
3. **Configure Sparse Checkout** - Excludes files based on profile
4. **Update Repository** - Pulls latest changes (`git pull --ff-only`)
5. **Install Git Hooks** - Symlinks repository git hooks
6. **Configure PATH** - Ensures dotfiles bin directory is on PATH

**Configure** (apply declared state):

7. **Install Packages** - Installs packages using winget
8. **Create Symlinks** - Links files from `symlinks/` to `%USERPROFILE%`
9. **Configure Git** - Sets `core.symlinks=true`, `core.autocrlf=false`, credential helper
10. **Apply Registry Settings** - Configures registry from `conf/registry.toml`
11. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.toml`
12. **Install Copilot Plugins** - Registers configured marketplaces and installs GitHub Copilot CLI plugins from `conf/copilot-plugins.toml`

## Verbose Mode

Enable verbose logging to see detailed operation information:

```bash
./dotfiles.sh install -v
```

**Verbose output includes:**
- Files being processed
- Operations being skipped (with reasons)
- Detailed package installation progress
- Symlink creation details
- All configuration processing

**Example verbose output:**
```
:: Installing packages
   Skipping git: already installed
   Skipping base-devel: already installed
   Installing alacritty...
   Package installed: alacritty
```

## Parallel Execution

Resource operations (symlinks, packages, registry entries, etc.) run in parallel
by default using Rayon's thread pool. This significantly speeds up installation
when many items need to be processed.

**When parallel execution runs:**
- Multiple symlinks are created concurrently
- Package state checks overlap
- Registry entries are applied in parallel

**Parallel execution is safe** — each resource is checked and applied independently,
and the results accumulator uses a mutex for thread-safe counting.

To disable parallel execution, see [Advanced Binary Options](#advanced-binary-options).

## Dry-Run Mode

Preview what would be done without making changes:

```bash
./dotfiles.sh install -d
```

**Dry-run mode:**
- Shows all operations that would be performed
- Doesn't modify system state
- Automatically enables verbose mode
- Useful for testing configuration changes
- Safe to run without privileges

**Example dry-run output:**
```
Would install package: alacritty
Would create symlink: /home/user/.config/alacritty
Would enable systemd unit: picom.service
```

## Logging

All operations are logged to persistent log files:

**Linux:**
- Location: `${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.log`
- Includes: Timestamps, operations, verbose details, summary

**Windows:**
- Location: `%LOCALAPPDATA%\dotfiles\install.log`
- Includes: Timestamps, operations, verbose details, summary

**Log contents:**
- Installation timestamp
- Selected profile
- All operations performed
- Verbose details (even when not shown on console)
- Summary statistics
- Error messages and warnings

A **diagnostic log** is also written alongside the main log:

**Linux:**
- Location: `${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.diag.log`

**Windows:**
- Location: `%LOCALAPPDATA%\dotfiles\install.diag.log`

The diagnostic log captures every event with microsecond-precision timestamps
and thread identification, preserving the true chronological order of parallel
execution.  See [Troubleshooting](TROUBLESHOOTING.md#using-diagnostic-logs) for
details on reading the diagnostic log.

## Installation Summary

After installation, a summary is displayed showing each task grouped by phase:

**Example:**
```
:: Summary
   Bootstrap
     ✓ Self-update
     ✓ Configure sparse checkout
     ✓ Update repository
     ✓ Install git hooks
     ✓ Install wrapper
     ✓ Configure PATH
   Configure
     ✓ Install symlinks
     ✓ Configure Git
     ✓ Install packages
     · Install AUR packages
     ○ Configure shell (skipped: not running on Arch)
     ✓ Enable systemd units

   12 tasks: 8 ok, 1 n/a, 1 skipped, 0 dry-run, 0 failed
   log: /home/user/.cache/dotfiles/install.log
```

**Status icons:**
- `✓` — task completed successfully (green)
- `·` — not applicable on this platform/profile (dim)
- `○` — deliberately skipped (yellow)
- `~` — dry-run preview (white)
- `✗` — task failed (red)

**Dry-run summary:**
Status icons show `~` for tasks that would have run:
```
:: Summary
   Bootstrap
     ~ Self-update
     ~ Configure sparse checkout
   Configure
     ~ Install symlinks
     ~ Configure Git

   4 tasks: 0 ok, 0 n/a, 0 skipped, 4 dry-run, 0 failed
   log: /home/user/.cache/dotfiles/install.log
```

## Idempotency

All operations are idempotent - safe to run multiple times:

```bash
# First run
./dotfiles.sh install -p desktop
# Installs packages, creates symlinks, etc.

# Second run
./dotfiles.sh install
# Skips already-installed packages
# Skips existing symlinks
# Only performs necessary work
```

**Expected behavior:**
- No errors on repeated runs
- Operations logged as "Skipping: already correct"
- No duplicate installations
- System state remains consistent

## Profile Persistence

Your profile selection is automatically saved:

```bash
# First run with explicit profile
./dotfiles.sh install -p desktop
# Profile is saved to .git/config

# Subsequent runs
./dotfiles.sh install
# Uses saved profile automatically, no need to specify
```

**Manual profile management:**
```bash
# Check saved profile
git config --local --get dotfiles.profile

# Change saved profile
git config --local dotfiles.profile base
```

## Examples by Use Case

### Minimal Server (Arch Linux)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p base
# Core configs + Arch packages (auto-detected), no desktop
```

### Full Desktop Workstation (Arch Linux)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Everything including desktop environment
```

### Non-Arch Linux Desktop (Ubuntu, Fedora, etc.)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Desktop tools without Arch-specific packages
```

### Cross-Platform Development (Windows)
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
# Windows configurations, registry, desktop tools
```

### CI/Testing Environment
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p base -d
# Test configuration without system modifications
```

### Development (building from source)
```bash
./dotfiles.sh --build install -p base -d
# Builds the Rust binary from source, then runs it
```

## Advanced Binary Options

The following flags are available when invoking the binary directly (`bin/dotfiles`)
and are intended for development and debugging. They are **not** exposed by the
wrapper scripts (`dotfiles.sh` / `dotfiles.ps1`).

```bash
./bin/dotfiles --root /path/to/dotfiles install --skip packages,fonts
./bin/dotfiles --root /path/to/dotfiles install --only symlinks
./bin/dotfiles --root /path/to/dotfiles --no-parallel install
```

- **`--skip TASKS`** - Skip specific tasks (comma-separated)
- **`--only TASKS`** - Run only specific tasks (comma-separated)
- **`--root DIR`** - Override dotfiles root directory (set automatically by wrapper scripts)
- **`--no-parallel`** - Disable parallel execution of resource operations

## See Also

- [Profile System](PROFILES.md) - Understanding profiles in detail
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Troubleshooting](TROUBLESHOOTING.md) - Common issues and solutions
- [Windows Usage](WINDOWS.md) - Windows-specific details
