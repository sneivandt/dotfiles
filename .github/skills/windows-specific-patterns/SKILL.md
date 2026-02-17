---
name: windows-specific-patterns
description: >
  Windows-specific implementation patterns and considerations beyond general PowerShell patterns.
  Use when working with Windows features, registry, admin privileges, or Windows-specific architecture.
metadata:
  author: sneivandt
  version: "1.0"
---

# Windows-Specific Patterns

This skill documents Windows-specific implementation patterns and architectural considerations in the dotfiles project.

## Overview

The Windows implementation provides equivalent functionality to the Linux version while accommodating platform differences:
- **Registry Configuration**: Windows registry management (no Linux equivalent)
- **Administrator Privileges**: Required for symlinks and registry (Linux uses sudo selectively)
- **PowerShell Modules**: Modular architecture matching Linux shell functions
- **Fixed Profile**: Windows always uses "windows" profile (no selection)

## Windows-Specific Features

### Registry Configuration

#### Configuration File: registry.ini

**Location**: `conf/registry.ini`

Registry configuration uses INI format with sections as registry paths:

```ini
# Section = Registry path
[HKCU:\Console]
WindowSize = 0x00200078                # Console window dimensions
ScreenBufferSize = 0x0BB80078          # Screen buffer size
FaceName = Cascadia Mono               # Console font

[HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer]
EnableAutoTray = 0                     # Disable auto-hide tray icons
```

**Format:**
- **Section headers**: Registry paths in PowerShell format (`HKCU:\`, `HKLM:\`)
- **Entries**: `ValueName = Value` (value can be hex, string, or numeric)
- **No profile filtering**: All registry settings applied when running on Windows

#### Registry Implementation

**Module**: `src/windows/Registry.psm1`

Key functions:

```powershell
# Parse registry path into hive and subkey
function Get-RegistryHiveAndKey {
    param ([string]$Path)

    if ($Path -match '^(HKEY_CURRENT_USER|HKCU)[:\\](.*)$') {
        return [PSCustomObject]@{
            Hive = [Microsoft.Win32.Registry]::CurrentUser
            SubKey = $matches[2]
        }
    }
    # ... other hives (HKLM, HKCR, HKU)
}

# Main registry sync function
function Sync-Registry {
    [CmdletBinding()]
    param ([switch]$DryRun)

    # Read conf/registry.ini
    # For each section (registry path):
    #   - Create path if missing
    #   - For each entry: compare and update if different
    #   - Increment counter only when values change
}
```

**Registry API Usage:**
- Uses `.NET Registry` APIs for PowerShell Core compatibility
- Compatible with both Windows PowerShell 5.1+ and PowerShell Core
- Handles all standard hives: HKCU, HKLM, HKCR, HKU

#### Registry Value Types

```powershell
# Supported registry value types
$registryValueTypes = @{
    "REG_SZ"        = [Microsoft.Win32.RegistryValueKind]::String
    "REG_DWORD"     = [Microsoft.Win32.RegistryValueKind]::DWord
    "REG_QWORD"     = [Microsoft.Win32.RegistryValueKind]::QWord
    "REG_BINARY"    = [Microsoft.Win32.RegistryValueKind]::Binary
    "REG_MULTI_SZ"  = [Microsoft.Win32.RegistryValueKind]::MultiString
    "REG_EXPAND_SZ" = [Microsoft.Win32.RegistryValueKind]::ExpandString
}

# Type inference from value
if ($value -match '^0x[0-9A-Fa-f]+$') {
    $type = [Microsoft.Win32.RegistryValueKind]::DWord  # Hex values
} else {
    $type = [Microsoft.Win32.RegistryValueKind]::String  # Default to string
}
```

### Administrator Privilege Requirements

#### When Admin is Required

**Operations requiring elevation:**
1. **Registry modification** (HKCU doesn't always require admin, but HKLM does)
2. **Symlink creation** (unless Developer Mode enabled)
3. **System-wide package installation** (winget to Program Files)

**Operations NOT requiring admin:**
- Dry-run mode (all operations)
- Reading configuration files
- Logging operations
- Git operations

#### Admin Check Pattern

```powershell
# Check if running as administrator
function Test-Administrator {
    $currentPrincipal = New-Object Security.Principal.WindowsPrincipal(
        [Security.Principal.WindowsIdentity]::GetCurrent()
    )
    return $currentPrincipal.IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator
    )
}

# Enforce admin requirement (except dry-run)
if (-not $DryRun -and -not (Test-Administrator)) {
    Write-LogError "Administrator privileges required (use -DryRun to preview changes)"
    throw "Administrator privileges required"
}
```

#### Developer Mode Alternative

Windows 10+ Developer Mode allows symlink creation without admin:

```powershell
# Check Developer Mode status
$devModeKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock"
$devModeEnabled = (Get-ItemProperty -Path $devModeKey -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense -eq 1

if (-not $devModeEnabled -and -not (Test-Administrator)) {
    Write-Warning "Symlink creation requires Administrator privileges or Developer Mode"
}
```

## Module Structure and Organization

### Module Layout

**Directory**: `src/windows/`

All PowerShell modules follow consistent naming:

```
src/windows/
├── CopilotSkills.psm1      # GitHub Copilot CLI skill management
├── Dotfiles.psm1           # Main orchestration module
├── Git.psm1                # Git configuration and repository updates
├── GitHooks.psm1           # Git hooks installation
├── Logging.psm1            # Logging and counter utilities
├── Module.psm1             # PowerShell module installation
├── Packages.psm1           # Package management
├── Profile.psm1            # Profile configuration utilities
├── Registry.psm1           # Windows registry management
├── Symlinks.psm1           # Symlink creation
└── VsCodeExtensions.psm1   # VS Code extension management
```

### Module Naming Conventions

**File Names:**
- PascalCase with `.psm1` extension
- Descriptive noun representing the module's domain
- Examples: `Logging.psm1`, `Registry.psm1`, `Symlinks.psm1`

**Function Names:**
- Use PowerShell approved verbs: `Get-`, `Set-`, `Install-`, `Sync-`, etc.
- PascalCase: `Install-Symlinks`, `Sync-Registry`, `Write-LogStage`
- Avoid unapproved verbs like `Download-` (use `Get-` instead)

### Export-ModuleMember Pattern

All modules explicitly export their public functions:

```powershell
# At end of each module file
Export-ModuleMember -Function @(
    'Install-Symlinks',
    'Uninstall-Symlinks'
)
```

**Guidelines:**
- **Explicit exports**: Only export public API functions
- **Helper functions**: Private helpers not exported (no `Export-ModuleMember`)
- **Consistency**: All modules follow this pattern

### Module Import Pattern

**Entry Point**: `dotfiles.ps1`

```powershell
# Set DOTFILES_ROOT environment variable
$env:DOTFILES_ROOT = $PSScriptRoot

# Import main module
$modulePath = Join-Path $PSScriptRoot "src\windows\Dotfiles.psm1"
Import-Module $modulePath -Force -ErrorAction Stop

# Call main installation function
Install-Dotfiles -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')
```

**Module Orchestration**: `Dotfiles.psm1`

```powershell
# Import all dependent modules
. "$PSScriptRoot\Logging.psm1"
. "$PSScriptRoot\Profile.psm1"
. "$PSScriptRoot\Git.psm1"
# ... other modules

# Main installation function
function Install-Dotfiles {
    [CmdletBinding()]
    param ([switch]$DryRun)

    Initialize-Logging -ProfileName "windows"
    Initialize-GitConfig -DryRun:$DryRun
    Install-Packages -DryRun:$DryRun
    Sync-Registry -DryRun:$DryRun
    # ... other operations
}

Export-ModuleMember -Function 'Install-Dotfiles'
```

## Cross-Platform Feature Parity

### Equivalent Operations

| Feature | Linux Implementation | Windows Implementation |
|---------|---------------------|------------------------|
| **Package Management** | `pacman`/`yay` (packages.ini) | `winget` (packages.ini) |
| **Configuration Files** | System packages handle configs | Registry + configuration files |
| **Symlinks** | `ln -s` | `New-Item -ItemType SymbolicLink` |
| **Logging** | `logger.sh` functions | `Logging.psm1` functions |
| **Profile Selection** | Interactive/CLI selection | Fixed "windows" profile |
| **Sparse Checkout** | Profile-based filtering | Fixed "windows" exclusions |
| **Dry-Run** | `--dry-run` flag | `-DryRun` parameter |

### Windows-Only Features

Features that exist only on Windows:

1. **Registry Configuration** (`conf/registry.ini`)
   - No Linux equivalent
   - Essential for Windows customization
   - Console colors, Explorer settings, regional formats

2. **Developer Mode Check**
   - Windows-specific symlink alternative
   - No Linux equivalent (Linux always allows symlinks)

### Linux-Only Features

Features that exist only on Linux:

1. **Package Manager Selection** (pacman/yay)
   - Arch-specific package management
   - Windows uses winget universally

2. **Systemd Unit Files** (`conf/units.ini`)
   - Linux service management
   - No direct Windows equivalent (uses Task Scheduler/Services)

3. **File Mode Bits** (`conf/chmod.ini`)
   - Unix permission model
   - Windows uses ACLs (not managed by dotfiles)

## Windows Path Conventions

### Special Paths

**PowerShell Automatic Variables:**
```powershell
$HOME                       # User home directory (C:\Users\<username>)
$PROFILE                    # PowerShell profile path
$env:USERPROFILE           # Same as $HOME
$env:LOCALAPPDATA          # C:\Users\<username>\AppData\Local
$env:APPDATA               # C:\Users\<username>\AppData\Roaming
$env:ProgramFiles          # C:\Program Files
$env:ProgramFiles(x86)     # C:\Program Files (x86)
```

**Dotfiles-Specific:**
```powershell
$env:DOTFILES_ROOT         # Repository root (set by dotfiles.ps1)
```

### Path Handling

```powershell
# Always use Join-Path for cross-platform compatibility
$configPath = Join-Path $env:DOTFILES_ROOT "conf\registry.ini"

# Handle both / and \ in paths
$normalizedPath = $path -replace '/', '\'

# Test path existence (works for files and directories)
if (Test-Path $path) {
    # Path exists
}

# Get absolute path
$absolutePath = Resolve-Path $relativePath
```

### Symlink Targets on Windows

**Important**: Symlinks on Windows must use absolute paths:

```powershell
# Correct: Absolute target path
$target = Join-Path $env:DOTFILES_ROOT "symlinks\vimrc"
New-Item -ItemType SymbolicLink -Path "$HOME\.vimrc" -Target $target

# Incorrect: Relative paths don't work reliably on Windows
New-Item -ItemType SymbolicLink -Path "$HOME\.vimrc" -Target "..\symlinks\vimrc"
```

## Windows-Specific Profile Handling

### Fixed "windows" Profile

**Key Difference from Linux:**
- **Linux**: Interactive profile selection (base, arch, arch-desktop, desktop)
- **Windows**: Always uses "windows" profile (no selection)

**Implementation:**

```powershell
# In Dotfiles.psm1
function Install-Dotfiles {
    # Profile is always "windows"
    Initialize-Logging -ProfileName "windows"

    # No profile selection logic
    # No sparse checkout configuration (not needed - Windows has no Linux files)
}
```

**Rationale:**
- Windows only runs on Windows (obvious!)
- No need for profile-based sparse checkout
- Simplifies Windows experience

### Configuration File Filtering

**No Profile Filtering Needed:**

Most Windows configuration files don't use profile sections:

```ini
# registry.ini - No profile sections
[HKCU:\Console]
WindowSize = 0x00200078

# All settings applied on Windows
```

## Common Windows Patterns

### Pattern: Registry Key Creation

```powershell
function Ensure-RegistryPath {
    param (
        [string]$Path,
        [switch]$DryRun
    )

    if (-not (Test-Path $Path)) {
        if ($DryRun) {
            Write-LogDryRun "Would create registry key: $Path"
        } else {
            New-Item -Path $Path -Force | Out-Null
            Write-LogVerbose "Created registry key: $Path"
        }
    }
}
```

### Pattern: Registry Value Comparison

```powershell
function Sync-RegistryValue {
    param (
        [string]$Path,
        [string]$Name,
        [object]$Value,
        [switch]$DryRun
    )

    $current = Get-ItemProperty -Path $Path -Name $Name -ErrorAction SilentlyContinue
    if ($null -eq $current -or $current.$Name -ne $Value) {
        if ($DryRun) {
            Write-LogDryRun "Would set $Path\$Name = $Value"
        } else {
            Set-ItemProperty -Path $Path -Name $Name -Value $Value
            Write-Counter "registry"
        }
    }
}
```

### Pattern: Symlink with Admin Check

```powershell
function New-DotfileSymlink {
    param (
        [string]$LinkPath,
        [string]$TargetPath,
        [switch]$DryRun
    )

    if (Test-Path $LinkPath) {
        Write-LogVerbose "Symlink already exists: $LinkPath"
        return
    }

    if ($DryRun) {
        Write-LogDryRun "Would create symlink: $LinkPath -> $TargetPath"
        return
    }

    # Requires admin unless Developer Mode enabled
    try {
        New-Item -ItemType SymbolicLink -Path $LinkPath -Target $TargetPath -Force
        Write-Counter "symlinks"
    } catch {
        Write-LogError "Failed to create symlink: $_"
        throw
    }
}
```

### Pattern: Windows Package Installation

```powershell
function Install-Package {
    param (
        [string]$PackageId,
        [switch]$DryRun
    )

    # Check if already installed
    $installed = winget list --id $PackageId 2>$null | Select-String $PackageId
    if ($installed) {
        Write-LogVerbose "Package already installed: $PackageId"
        return
    }

    if ($DryRun) {
        Write-LogDryRun "Would install package: $PackageId"
        return
    }

    Write-LogVerbose "Installing package: $PackageId"
    winget install --id $PackageId --silent --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -eq 0) {
        Write-Counter "packages"
    } else {
        Write-LogError "Package installation failed: $PackageId"
    }
}
```

## Architectural Differences

### Execution Model

**Linux (Shell):**
- Scripts execute in subshells for isolation
- Each task function wrapped in `()`
- Variables and cd don't leak to parent

**Windows (PowerShell):**
- Functions execute in current scope
- No automatic isolation
- Careful scope management required

### Error Handling Model

**Linux (Shell):**
- `set -o errexit` - Exit on any error
- `log_error` automatically exits
- Strict fail-fast model

**Windows (PowerShell):**
- `$ErrorActionPreference = 'Continue'` - Show errors but continue
- `Write-LogError` logs but doesn't exit
- More graceful degradation

### Configuration Processing

**Linux:**
- Profile selection determines sparse checkout
- Configuration files filtered by active profile
- Multi-level profile system

**Windows:**
- Fixed "windows" profile
- All Windows configs applied
- No sparse checkout needed

## Rules for Agents

When working with Windows-specific features:

1. **Use .NET Registry APIs** for PowerShell Core compatibility
2. **Always export functions** explicitly with `Export-ModuleMember`
3. **Check admin privileges** before registry/symlink operations (except dry-run)
4. **Use absolute paths** for symlink targets on Windows
5. **Use approved PowerShell verbs** (Get-, Set-, Install-, Sync-, etc.)
6. **Join-Path for all paths** - never hardcode backslashes
7. **Test both PowerShell Core and Windows PowerShell** compatibility
8. **Document admin requirements** in function documentation
9. **Support -DryRun parameter** in all state-changing functions
10. **No profile selection** - Windows always uses "windows" profile
11. **Registry.ini has no profile sections** - all settings applied on Windows
12. **Propagate -DryRun** to all called functions

## Related Skills and Documentation

- **`powershell-patterns`** skill - General PowerShell conventions
- **`error-handling-patterns`** skill - Error handling and idempotency
- **`logging-patterns`** skill - Logging conventions
- **`ini-configuration`** skill - Configuration file formats
- **`docs/WINDOWS.md`** - Windows usage guide
- **`docs/CONFIGURATION.md`** - Configuration reference

## Key Files

- **`dotfiles.ps1`** - Windows entry point
- **`src/windows/Dotfiles.psm1`** - Main orchestration module
- **`src/windows/Registry.psm1`** - Registry management
- **`src/windows/Symlinks.psm1`** - Symlink creation
- **`conf/registry.ini`** - Registry configuration
