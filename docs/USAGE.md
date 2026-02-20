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
dotfiles.sh [--build] install   [-p PROFILE] [-d] [-v] [--skip TASKS] [--only TASKS]
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
- **`--skip TASKS`** - Skip specific tasks (comma-separated)
- **`--only TASKS`** - Run only specific tasks (comma-separated)
- **`--root DIR`** - Override dotfiles root directory
- **`--no-parallel`** - Disable parallel resource processing (parallel is on by default)

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
- All other options are the same as Linux (forwarded to the binary)

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
# Validates INI file syntax
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

1. **Configure Sparse Checkout** - Excludes files based on profile
2. **Update Repository** - Pulls latest changes (`git pull --ff-only`)
3. **Configure Git** - Applies git configuration
4. **Install Git Hooks** - Symlinks repository git hooks
5. **Install Packages** - Installs packages from `conf/packages.ini` using pacman
6. **Install Paru** - Bootstraps paru AUR helper (Arch Linux only)
7. **Install AUR Packages** - Installs AUR packages via paru (Arch Linux only)
8. **Create Symlinks** - Links files from `symlinks/` to `$HOME`
9. **Set Permissions** - Applies file permissions from `conf/chmod.ini`
10. **Configure Shell** - Sets default shell
11. **Enable Systemd Units** - Enables and starts user units from `conf/systemd-units.ini`
12. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.ini`
13. **Install Copilot Skills** - Downloads GitHub Copilot CLI skills from `conf/copilot-skills.ini`

### Windows Installation Steps

1. **Enable Developer Mode** - Enables Windows developer mode (required for symlinks)
2. **Configure Sparse Checkout** - Excludes files based on profile
3. **Update Repository** - Pulls latest changes (`git pull --ff-only`)
4. **Configure Git** - Sets `core.symlinks=true`, `core.autocrlf=false`, credential helper
5. **Install Git Hooks** - Symlinks repository git hooks
6. **Install Packages** - Installs packages using winget
7. **Create Symlinks** - Links files from `symlinks/` to `%USERPROFILE%`
8. **Apply Registry Settings** - Configures registry from `conf/registry.ini`
9. **Install VS Code Extensions** - Installs extensions from `conf/vscode-extensions.ini`
10. **Install Copilot Skills** - Downloads GitHub Copilot CLI skills from `conf/copilot-skills.ini`

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

**To disable parallel execution:**

```bash
./dotfiles.sh install --no-parallel
```

**When parallel execution runs:**
- Multiple symlinks are created concurrently
- Package state checks overlap
- Registry entries are applied in parallel

**Parallel execution is safe** — each resource is checked and applied independently,
and the results accumulator uses a mutex for thread-safe counting.

**Note:** The wrapper scripts (`dotfiles.sh`, `dotfiles.ps1`) do not expose
`--no-parallel` as a named parameter. Pass it directly if you need sequential mode:

```bash
./dotfiles.sh install --no-parallel
```

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

## Installation Summary

After installation, a summary is displayed:

**Linux example:**
```
:: Installation Summary
Packages installed: 15
AUR packages installed: 3
Symlinks created: 8
VS Code extensions installed: 5
Copilot skills installed: 2
Systemd units enabled: 2
Log file: /home/user/.cache/dotfiles/install.log
```

**Windows example:**
```
:: Installation Summary
Packages installed: 3
Symlinks created: 5
VS Code extensions installed: 2
Copilot skills installed: 2
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

## See Also

- [Profile System](PROFILES.md) - Understanding profiles in detail
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Troubleshooting](TROUBLESHOOTING.md) - Common issues and solutions
- [Windows Usage](WINDOWS.md) - Windows-specific details
