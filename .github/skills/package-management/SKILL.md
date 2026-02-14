---
name: package-management
description: >
  Package installation patterns for the dotfiles project.
  Use when working with system packages on Linux (pacman/AUR) or Windows (winget).
metadata:
  author: sneivandt
  version: "1.0"
---

# Package Management

This skill documents package installation patterns and conventions used in the dotfiles project for both Linux and Windows platforms.

## Overview

The dotfiles project manages system packages declaratively through `conf/packages.ini`:
- **Linux**: Uses `pacman` (official Arch repos) and `paru` (AUR packages)
- **Windows**: Uses `winget` (Windows Package Manager)
- **Idempotent**: All package checks prevent redundant installations
- **Profile-Based**: Different package sets for different profiles (base, arch, arch-desktop, windows)

## Configuration File Format

### INI Structure
Package lists are organized by profile sections in `conf/packages.ini`:

```ini
# Linux packages - Official repositories
[arch]
git
neovim
zsh

# Linux packages - Desktop-specific
[arch,desktop]
alacritty
xmonad
chromium

# Linux packages - AUR (requires paru)
[arch,aur]
powershell-bin

# Linux packages - AUR desktop-specific
[arch,desktop,aur]
visual-studio-code-insiders-bin
spotify

# Windows packages
[windows]
Git.Git
Microsoft.PowerShell
Microsoft.VisualStudioCode
```

### Section Naming Convention
- `[arch]` - Base Arch Linux packages for all systems
- `[arch,desktop]` - Desktop-specific packages (excludes headless servers)
- `[arch,aur]` - AUR packages for all Arch systems
- `[arch,desktop,aur]` - AUR packages for desktop systems only
- `[windows]` - Windows packages

See the `profile-system` skill for details on profile filtering.

## Linux Package Management

### Package Managers

#### pacman (Official Repositories)
- **Command**: `sudo pacman -S --quiet --needed --noconfirm <packages>`
- **Detection**: Checks if package exists with `pacman -Qq <package>`
- **Flags**:
  - `--needed`: Skip already installed packages
  - `--noconfirm`: No prompts (for automation)
  - `--quiet`: Minimal output

#### paru (AUR Helper)
- **Command**: `paru -S --needed --noconfirm <packages>`
- **Installation**: Automatically built from AUR if needed (requires git, base-devel, rust)
- **Detection**: Checks if package exists with `pacman -Qq <package>` (same as pacman)
- **Note**: paru handles sudo internally - do NOT prefix with sudo

### Adding Linux Packages

1. Find the correct package name:
```bash
# Official repositories
pacman -Ss <search-term>

# AUR packages
paru -Ss <search-term>
```

2. Add to appropriate section in `conf/packages.ini`:
```ini
[arch]
# Official repo packages
my-package

[arch,aur]
# AUR packages
my-aur-package-bin
```

3. Install:
```bash
./dotfiles.sh -I
```

### Implementation Pattern (Shell)

```bash
# Check for package manager
if ! is_program_installed "sudo" || ! is_program_installed "pacman"; then
    log_verbose "Skipping package installation: sudo or pacman not installed"
    return
fi

# Build list of missing packages
packages=""
for package in $package_list; do
    if ! pacman -Qq "$package" >/dev/null 2>&1; then
        packages="$packages $package"
        log_verbose "Package needs installation: $package"
    else
        log_verbose "Skipping package $package: already installed"
    fi
done

# Install if any packages missing
if [ -n "$packages" ]; then
    if ! is_dry_run; then
        sudo pacman -S --quiet --needed --noconfirm $packages
        # Increment counter for each package
        for pkg in $packages; do
            increment_counter "packages_installed"
        done
    else
        log_dry_run "Would install packages: $packages"
    fi
fi
```

## Windows Package Management

### Package Manager

#### winget (Windows Package Manager)
- **Built-in**: Included in Windows 11 and modern Windows 10
- **Command**: `winget install --id <PackageId> --silent --accept-package-agreements --accept-source-agreements`
- **Detection**: Use `winget list --id <PackageId>` to check if installed
- **Flags**:
  - `--silent`: No UI, minimal output
  - `--accept-package-agreements`: Auto-accept licenses
  - `--accept-source-agreements`: Auto-accept source agreements

### Adding Windows Packages

1. Find the correct package ID:
```powershell
winget search <search-term>
# Note the exact Package ID (e.g., Microsoft.PowerShell)
```

2. Add to `conf/packages.ini`:
```ini
[windows]
Microsoft.PowerShell
Git.Git
Microsoft.VisualStudioCode
```

3. Install:
```powershell
.\dotfiles.ps1
# Or from anywhere:
Install-Dotfiles
```

### Implementation Pattern (PowerShell)

```powershell
function Test-PackageInstalled {
    param ([string]$PackageId)

    try {
        $result = winget list --id $PackageId 2>&1
        # Check if output contains the package ID
        return ($result -match [regex]::Escape($PackageId))
    }
    catch {
        return $false
    }
}

# Check each package
foreach ($packageId in $packages) {
    if (-not (Test-PackageInstalled -PackageId $packageId)) {
        Write-VerboseMessage "Package needs installation: $packageId"

        if (-not $DryRun) {
            winget install --id $packageId --silent `
                --accept-package-agreements `
                --accept-source-agreements
            Add-Counter -CounterName "packages_installed"
        } else {
            Write-DryRunMessage "Would install package: $packageId"
        }
    } else {
        Write-VerboseMessage "Package already installed: $packageId"
    }
}
```

## Rules for Package Management

1. **Always check before installing**: Use package manager's query commands to check if package is already installed

2. **Use batch installations when possible**: Collect all missing packages and install in a single command for efficiency

3. **Use --needed flag**: Prevents unnecessary reinstallation on Linux (`pacman` and `paru`)

4. **No sudo for paru**: The paru AUR helper manages elevation internally

5. **Handle missing package managers gracefully**: Skip package installation with verbose message if package manager not found

6. **Increment counters**: Track each installed package with `increment_counter` or `Add-Counter` for summary statistics

7. **Support dry-run mode**: Show what would be installed without actually installing

8. **Use exact package IDs on Windows**: Package IDs are case-sensitive and must match exactly (e.g., `Microsoft.PowerShell`, not `powershell`)

9. **Separate AUR packages**: Use dedicated `[arch,aur]` sections for AUR packages to keep them separate from official repo packages

10. **Prerequisites for paru**: Ensure git, base-devel, and rust are installed before building paru from AUR

## Common Patterns

### Installing paru (AUR Helper)
```bash
install_paru() {
    if is_program_installed "paru"; then
        log_verbose "Skipping paru installation: already installed"
        return
    fi

    # Check prerequisites
    if ! is_program_installed "git" || ! is_program_installed "makepkg" || ! is_program_installed "cargo"; then
        log_verbose "Skipping paru installation: missing prerequisites"
        return
    fi

    log_stage "Installing paru (AUR helper)"

    if is_dry_run; then
        log_dry_run "Would clone and build paru from AUR"
        return
    fi

    # Create temp directory
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    # Clone and build
    cd "$tmp_dir"
    git clone https://aur.archlinux.org/paru-git.git .
    makepkg -si --noconfirm
}
```

### Batch Package Installation
```bash
# Build space-separated list of missing packages
packages=""
for package in $package_list; do
    if ! pacman -Qq "$package" >/dev/null 2>&1; then
        packages="$packages $package"
    fi
done

# Install all at once (more efficient than individual installs)
if [ -n "$packages" ]; then
    sudo pacman -S --quiet --needed --noconfirm $packages
fi
```

## Cross-References

- See the `profile-system` skill for profile filtering and section naming
- See the `ini-configuration` skill for INI file format details
- See the `logging-patterns` skill for logging and counter tracking
- See the `shell-patterns` skill for shell script patterns
- See the `powershell-patterns` skill for PowerShell patterns
