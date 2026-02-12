# Usage Guide

Comprehensive guide to using the dotfiles installation and management system.

## Installation

### Linux

**Basic installation with profile selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile arch-desktop
```

**Interactive profile selection (first time):**
```bash
./dotfiles.sh -I
# You'll be prompted to select from available profiles
# Your selection is saved for future runs
```

**Subsequent runs (uses saved profile):**
```bash
./dotfiles.sh -I
# Automatically uses the profile you selected previously
```

### Windows

**Initial installation:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1
```

**Using the module command (after initial installation):**
```powershell
Install-Dotfiles
# Can be run from any directory
```

## Command Line Options

### Linux (`dotfiles.sh`)

**Synopsis:**
```
dotfiles.sh {-I | --install}   [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]
dotfiles.sh {-U | --uninstall} [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]
dotfiles.sh {-T | --test}      [-v]
dotfiles.sh {-h | --help}
```

**Options:**

- **`-I, --install`** - Install dotfiles
- **`-U, --uninstall`** - Uninstall dotfiles (removes managed symlinks)
- **`-T, --test`** - Run tests (static analysis and configuration validation)
- **`-h, --help`** - Show help message
- **`--profile PROFILE`** - Use specific profile (base, arch, arch-desktop, desktop, windows)
- **`-v, --verbose`** - Enable verbose logging
- **`--dry-run`** - Preview changes without modifying system (auto-enables verbose)
- **`--skip-os-detection`** - Skip automatic OS detection overrides (primarily for CI testing)

### Windows (`dotfiles.ps1`)

**Synopsis:**
```powershell
.\dotfiles.ps1 [-DryRun] [-Verbose]
Install-Dotfiles [-DryRun] [-Verbose]
```

**Parameters:**

- **`-DryRun`** - Preview changes without making modifications
- **`-Verbose`** - Show detailed operation logs

## Common Workflows

### First-Time Setup

**Linux with interactive selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I
# Select profile when prompted
# Installation proceeds with your selection
```

**Linux with explicit profile:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile arch-desktop
```

**Windows:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1
```

### Updating Dotfiles

**Linux (uses saved profile):**
```bash
cd ~/dotfiles
git pull
./dotfiles.sh -I
```

**Windows (automatic repository update):**
```powershell
# From anywhere
Install-Dotfiles
# Automatically updates repository and applies changes
```

### Previewing Changes

**Before running installation:**
```bash
./dotfiles.sh -I --profile arch-desktop --dry-run
# Shows what would be done without making changes
# Verbose mode is automatically enabled
```

**Windows:**
```powershell
.\dotfiles.ps1 -DryRun -Verbose
```

### Switching Profiles

**Change profile and apply changes:**
```bash
./dotfiles.sh -I --profile base
# Switches to base profile
# Removes desktop-specific symlinks and files
# New profile is saved for future runs
```

### Uninstalling

**Remove all managed symlinks:**
```bash
./dotfiles.sh -U
# Removes symlinks created by dotfiles
# Does not remove packages or other configuration
```

**Preview uninstallation:**
```bash
./dotfiles.sh -U --dry-run
```

### Testing

**Run all tests:**
```bash
./dotfiles.sh -T
# Runs configuration validation
# Runs shellcheck on all shell scripts
# Runs PSScriptAnalyzer on PowerShell scripts
```

**Verbose test output:**
```bash
./dotfiles.sh -T -v
```

## What Gets Installed

The installation process handles different components based on your profile:

### Linux Installation Steps

1. **Configure Sparse Checkout** - Excludes files based on profile
2. **Install Packages** - Installs packages from `conf/packages.ini` using pacman/paru
3. **Create Symlinks** - Links files from `symlinks/` to `$HOME`
4. **Enable Systemd Units** - Enables and starts user units from `conf/units.ini`
5. **Install Fonts** - Installs font families from `conf/fonts.ini`
6. **Set Permissions** - Applies file permissions from `conf/chmod.ini`
7. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.ini`
8. **Install PowerShell Modules** - Installs modules when pwsh is available
9. **Install Git Hooks** - Symlinks repository git hooks

### Windows Installation Steps

1. **Configure Git** - Sets `core.symlinks=false` for compatibility
2. **Update Repository** - Pulls latest changes with automatic stash handling
3. **Install Git Hooks** - Symlinks repository git hooks
4. **Install Module** - Installs dotfiles as PowerShell module
5. **Install Packages** - Installs packages using winget
6. **Apply Registry Settings** - Configures registry from `conf/registry.ini`
7. **Create Symlinks** - Links files from `symlinks/` to `%USERPROFILE%`
8. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.ini`

## Verbose Mode

Enable verbose logging to see detailed operation information:

```bash
./dotfiles.sh -I -v
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

## Dry-Run Mode

Preview what would be done without making changes:

```bash
./dotfiles.sh -I --dry-run
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

## Installation Summary

After installation, a summary is displayed:

**Linux example:**
```
:: Installation Summary
Packages installed: 15
AUR packages installed: 3
Symlinks created: 8
VS Code extensions installed: 5
Systemd units enabled: 2
Log file: /home/user/.cache/dotfiles/install.log
```

**Windows example:**
```
:: Installation Summary
Packages installed: 3
PowerShell modules installed: 1
Symlinks created: 5
VS Code extensions installed: 2
Registry keys set: 12
Log file: C:\Users\YourName\AppData\Local\dotfiles\install.log
```

**Dry-run summary:**
Counters show "(would be)" suffix:
```
:: Installation Summary
Packages installed (would be): 15
Symlinks created (would be): 8
Log file: /home/user/.cache/dotfiles/install.log
```

## Idempotency

All operations are idempotent - safe to run multiple times:

```bash
# First run
./dotfiles.sh -I --profile arch-desktop
# Installs packages, creates symlinks, etc.

# Second run
./dotfiles.sh -I
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
./dotfiles.sh -I --profile arch-desktop
# Profile is saved to .git/config

# Subsequent runs
./dotfiles.sh -I
# Uses arch-desktop automatically, no need to specify
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
./dotfiles.sh -I --profile arch
# Core configs + Arch packages, no desktop
```

### Full Desktop Workstation (Arch Linux)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile arch-desktop
# Everything including desktop environment
```

### Non-Arch Linux Desktop (Ubuntu, Fedora, etc.)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile desktop
# Desktop tools without Arch-specific packages
```

### Cross-Platform Development (Windows)
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1
# Windows configurations, registry, desktop tools
```

### CI/Testing Environment
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile base --dry-run --skip-os-detection
# Test configuration without system modifications
```

## See Also

- [Profile System](PROFILES.md) - Understanding profiles in detail
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Troubleshooting](TROUBLESHOOTING.md) - Common issues and solutions
- [Windows Usage](WINDOWS.md) - Windows-specific details
