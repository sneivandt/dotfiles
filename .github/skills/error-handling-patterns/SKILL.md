---
name: error-handling-patterns
description: >
  Idempotency and error handling conventions, which are core design principles.
  Use when implementing tasks that modify system state, handle errors, or support dry-run mode.
metadata:
  author: sneivandt
  version: "1.0"
---

# Error Handling Patterns

This skill documents error handling and idempotency patterns used across both Linux shell scripts and Windows PowerShell modules in the dotfiles project.

## Overview

The dotfiles project follows strict idempotency and error handling principles:
- **Idempotent**: All operations can be safely re-run without unwanted side effects
- **Defensive**: Check existing state before making changes
- **Fail-Fast**: Errors stop execution immediately (unless explicitly handled)
- **Dry-Run**: Preview mode shows what would change without making modifications
- **Consistent**: Same patterns across both platforms (Shell and PowerShell)

## Idempotency Principles

### Core Concept

**Idempotency**: Re-running the same operation produces the same result without side effects.

**Benefits:**
- Safe to re-run after failures
- No duplicate resources created
- Predictable behavior
- Users can re-run to ensure everything is up-to-date

### Check Before Mutate Pattern

**Always check existing state before making changes:**

```bash
# Shell: Check before creating symlink
if [ ! -e "$HOME/.vimrc" ]; then
  ln -s "$DIR/symlinks/vimrc" "$HOME/.vimrc"
  log_verbose "Created symlink: ~/.vimrc"
else
  log_verbose "Symlink already exists: ~/.vimrc"
fi
```

```powershell
# PowerShell: Check before creating registry key
if (-not (Test-Path $registryPath)) {
    New-Item -Path $registryPath -Force | Out-Null
    Write-LogVerbose "Created registry key: $registryPath"
} else {
    Write-LogVerbose "Registry key already exists: $registryPath"
}
```

### Compare Before Update Pattern

For configuration values, compare existing value before updating:

```powershell
# PowerShell: Only update if value differs
$currentValue = Get-RegistryValue -Path $path -Name $valueName
if ($currentValue -ne $expectedValue) {
    Set-RegistryValue -Path $path -Name $valueName -Value $expectedValue
    Write-Counter "registry"
} else {
    Write-LogVerbose "Registry value unchanged: $valueName = $currentValue"
}
```

```bash
# Shell: Check if git config value needs updating
current="$(git config --global user.email 2>/dev/null || echo '')"
if [ "$current" != "$email" ]; then
  git config --global user.email "$email"
  log_verbose "Updated git user.email"
fi
```

### Skip Already-Installed Pattern

For package installation, verify package isn't already installed:

```bash
# Shell: Check package before installing
if ! is_program_installed "vim"; then
  sudo pacman -S --noconfirm vim
  increment_counter "packages"
else
  log_verbose "Package already installed: vim"
fi
```

```powershell
# PowerShell: Check winget package before installing
$installed = winget list --id $packageId 2>$null
if (-not $installed) {
    winget install --id $packageId --silent
    Write-Counter "packages"
} else {
    Write-LogVerbose "Package already installed: $packageId"
}
```

## Error Handling Conventions

### Shell Error Handling

#### Errexit and Nounset

All shell scripts start with strict error handling:

```bash
#!/bin/sh
set -o errexit   # Exit immediately if any command fails
set -o nounset   # Exit if undefined variable is referenced
```

**`errexit` (set -e):**
- Script exits immediately if any command returns non-zero exit code
- Prevents cascading failures
- Forces explicit error handling

**`nounset` (set -u):**
- Script exits if undefined variable is used
- Catches typos and missing variable initialization
- Makes scripts more robust

#### Exit Codes

Standard exit codes used throughout:

```bash
# Success
exit 0

# General error (via log_error)
exit 1

# Examples from dotfiles.sh
if [ "$(id -u)" = 0 ]; then
  log_error "$(basename "$0") can not be run as root."
fi
# log_error calls exit 1 automatically
```

#### Explicit Error Handling

Use `|| true` or conditional checks when a command failure is acceptable:

```bash
# Allow command to fail without exiting script
git stash pop 2>/dev/null || true

# Conditional check instead of relying on errexit
if git pull origin main; then
  log_verbose "Repository updated"
else
  log_error "Failed to update repository"
fi
```

#### Subshell Isolation

Tasks run in subshells to isolate state changes:

```bash
# From tasks.sh - each task in subshell
configure_file_mode_bits()
{(
  # Subshell: cd, variable changes don't leak
  cd "$DIR" || return
  local mode="755"
  chmod "$mode" file
)}
```

**Benefits:**
- Directory changes don't affect caller
- Local variables don't pollute parent scope
- Failures in subshell don't affect parent (when not using errexit)

### PowerShell Error Handling

#### Error Action Preference

PowerShell uses `ErrorActionPreference` to control error behavior:

```powershell
# In dotfiles.ps1 entry point
$ErrorActionPreference = 'Continue'  # Show errors but continue

# In functions - use explicit error actions
Get-RegistryValue -Path $path -ErrorAction SilentlyContinue
New-Item -Path $path -ErrorAction Stop  # Throw terminating error
```

**Error Actions:**
- **`Stop`**: Throw terminating error (like errexit)
- **`Continue`**: Write error but continue (default)
- **`SilentlyContinue`**: Suppress error, continue silently
- **`Ignore`**: Completely ignore error

#### Try-Catch-Finally

Use try-catch for operations that might fail:

```powershell
try {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($subKey, $true)
    if ($null -eq $key) {
        throw "Failed to open registry key: $subKey"
    }
    $key.SetValue($valueName, $value)
} catch {
    Write-LogError "Registry operation failed: $_"
    throw
} finally {
    if ($null -ne $key) {
        $key.Close()
    }
}
```

**Structure:**
- **`try`**: Code that might throw errors
- **`catch`**: Handle errors (log, cleanup, re-throw)
- **`finally`**: Cleanup code (always runs, even after errors)

#### Terminating vs Non-Terminating Errors

```powershell
# Non-terminating error (cmdlet-based)
Get-Item "nonexistent" -ErrorAction Continue  # Writes error, continues

# Convert to terminating error
Get-Item "nonexistent" -ErrorAction Stop  # Throws exception

# Throw custom errors
if (-not $condition) {
    throw "Validation failed: condition not met"
}
```

## Dry-Run Implementation

### Shell Dry-Run Pattern

#### Global Flag

```bash
# From dotfiles.sh - dry-run flag stored in OPT
OPT="$@"  # Store all arguments

# Check if dry-run mode is active
is_dry_run()
{
  case "$OPT" in
    *--dry-run*) return 0 ;;  # True: in dry-run mode
    *) return 1 ;;             # False: normal mode
  esac
}
```

#### Conditional Execution

```bash
# Pattern 1: Skip mutation entirely
if is_dry_run; then
  log_dry_run "Would create symlink: ~/.vimrc -> $DIR/symlinks/vimrc"
  return 0
fi
ln -s "$DIR/symlinks/vimrc" "$HOME/.vimrc"

# Pattern 2: Show what would happen, then skip
log_verbose "Processing package: $package"
if is_dry_run; then
  log_dry_run "Would install package: $package"
else
  sudo pacman -S --noconfirm "$package"
  increment_counter "packages"
fi
```

#### Dry-Run Logging

```bash
# From logger.sh
log_dry_run()
{
  printf "%s[DRY-RUN]%s %s\n" "$BLUE" "$NC" "$*"
  _log_to_file "DRY" "$@"
}
```

**Characteristics:**
- Blue color for visibility
- `[DRY-RUN]` prefix
- Logged to file with `DRY` level
- No counter increment in dry-run mode

### PowerShell Dry-Run Pattern

#### Parameter Declaration

All functions support `-DryRun` parameter:

```powershell
function Install-Symlinks {
    [CmdletBinding(SupportsShouldProcess)]
    param (
        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )
    
    # Function implementation
}
```

#### Conditional Execution

```powershell
# Pattern 1: WhatIf support (preferred for simple cases)
if ($PSCmdlet.ShouldProcess($target, "Create symlink")) {
    New-Item -ItemType SymbolicLink -Path $linkPath -Target $targetPath
}

# Pattern 2: Explicit DryRun check
if ($DryRun) {
    Write-LogDryRun "Would create symlink: $linkPath -> $targetPath"
    return
}
New-Item -ItemType SymbolicLink -Path $linkPath -Target $targetPath

# Pattern 3: Combined (most common)
if ($DryRun) {
    Write-LogDryRun "Would set registry value: $valueName = $value"
} else {
    Set-RegistryValue -Path $path -Name $valueName -Value $value
    Write-Counter "registry"
}
```

#### Parameter Propagation

Pass `DryRun` to called functions:

```powershell
function Install-Dotfiles {
    [CmdletBinding()]
    param (
        [switch]$DryRun
    )
    
    # Propagate DryRun to all called functions
    Install-Packages -DryRun:$DryRun
    Sync-Registry -DryRun:$DryRun
    Install-Symlinks -DryRun:$DryRun
}
```

#### Dry-Run Logging

```powershell
function Write-LogDryRun {
    param ([string]$Message)
    
    Write-Host "[DRY-RUN] $Message" -ForegroundColor Cyan
    Write-LogMessage -Level "DRY" -Message $Message
}
```

## Logging Errors vs Warnings vs Info

### Shell Logging Levels

```bash
# Stage: Major operation grouping (:: prefix)
log_stage "Installing packages"
# Output: :: Installing packages
# File:   2026-02-14 10:30:00 STG Installing packages

# Info: General progress messages
log_verbose "Processing package: vim"
# Output: Processing package: vim
# File:   2026-02-14 10:30:01 VRB Processing package: vim

# Error: Fatal errors (exits script)
log_error "Failed to install package: vim"
# Output: Error: Failed to install package: vim (red)
# File:   2026-02-14 10:30:02 ERR Failed to install package: vim
# Then: exit 1

# Dry-Run: Preview messages
log_dry_run "Would install package: vim"
# Output: [DRY-RUN] Would install package: vim (blue)
# File:   2026-02-14 10:30:03 DRY Would install package: vim
```

### PowerShell Logging Levels

```powershell
# Stage: Major operation grouping
Write-LogStage "Installing packages"
# Output: :: Installing packages
# File:   2026-02-14 10:30:00 STG Installing packages

# Info: General progress  
Write-LogVerbose "Processing package: Git"
# Output: Processing package: Git (only with -Verbose)
# File:   2026-02-14 10:30:01 VRB Processing package: Git

# Error: Non-fatal errors (logged, doesn't exit)
Write-LogError "Failed to install package: Git"
# Output: Error: Failed to install package: Git (red)
# File:   2026-02-14 10:30:02 ERR Failed to install package: Git

# Dry-Run: Preview messages
Write-LogDryRun "Would install package: Git"
# Output: [DRY-RUN] Would install package: Git (cyan)
# File:   2026-02-14 10:30:03 DRY Would install package: Git
```

**Key Difference:**
- **Shell**: `log_error` exits immediately (because of errexit + explicit exit in function)
- **PowerShell**: `Write-LogError` logs but doesn't exit (allows graceful degradation)

## Transaction-Like Operations

### Pattern: Save State, Modify, Restore on Error

```bash
# Shell: Git stash/pop pattern
if ! git diff --quiet; then
  log_verbose "Stashing local changes"
  git stash push -u
  stashed=1
fi

# Perform operation that might fail
if ! git pull origin main; then
  if [ "$stashed" -eq 1 ]; then
    git stash pop
  fi
  log_error "Failed to update repository"
fi

# Restore stash on success
if [ "$stashed" -eq 1 ]; then
  git stash pop || true  # Don't fail if stash pop fails
fi
```

```powershell
# PowerShell: Try-Finally pattern
$tempFile = $null
try {
    $tempFile = New-TemporaryFile
    # Perform operations with temp file
    Copy-Item $tempFile $destination
} catch {
    Write-LogError "Operation failed: $_"
    throw
} finally {
    if ($null -ne $tempFile -and (Test-Path $tempFile)) {
        Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
    }
}
```

### Pattern: Verify Prerequisites

```bash
# Shell: Check prerequisites before starting
if [ ! -d "$DIR"/.git ]; then
  log_error "Not a git repository: $DIR"
fi

if ! command -v git >/dev/null 2>&1; then
  log_error "Git is not installed"
fi

# Git version check
git_version="$(git --version | grep -oE '[0-9]+\.[0-9]+')"
if [ "$(printf '%s\n' "2.25" "$git_version" | sort -V | head -n1)" != "2.25" ]; then
  log_error "Git 2.25+ required for sparse checkout (found: $git_version)"
fi
```

```powershell
# PowerShell: Verify prerequisites
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "Git is not installed"
}

if (-not (Test-Path $env:DOTFILES_ROOT)) {
    throw "DOTFILES_ROOT environment variable not set"
}

# Version check
$gitVersion = (git --version) -replace '.*?(\d+\.\d+\.\d+).*', '$1'
if ([version]$gitVersion -lt [version]"2.25.0") {
    throw "Git 2.25+ required for sparse checkout (found: $gitVersion)"
}
```

## Conditional Execution Patterns

### Pattern: Skip on Condition

```bash
# Skip if already done
if [ -f "$HOME/.gitconfig" ]; then
  log_verbose "Git already configured"
  return 0
fi

# Skip if dry-run
if is_dry_run; then
  log_dry_run "Would configure git"
  return 0
fi

# Skip if profile doesn't match
if ! should_include_profile_tag "$section"; then
  log_verbose "Skipping section [$section]: profile not included"
  continue
fi
```

```powershell
# Skip if already done
if (Test-Path $gitConfigPath) {
    Write-LogVerbose "Git already configured"
    return
}

# Skip if dry-run
if ($DryRun) {
    Write-LogDryRun "Would configure git"
    return
}

# Skip if condition not met
if (-not $isWindows) {
    Write-LogVerbose "Skipping Windows-specific operation"
    return
}
```

## Rules for Agents

When implementing error handling and idempotency:

1. **Always use `set -o errexit` and `set -o nounset`** at the start of shell scripts
2. **Check existing state** before making changes (files, registry keys, packages)
3. **Compare values** before updating (git config, registry values)
4. **Support dry-run mode** in all functions that modify system state
5. **Log appropriately**: Use stages for major operations, verbose for details, errors for failures
6. **Don't increment counters** in dry-run mode
7. **Use subshells** in shell scripts to isolate state changes
8. **Propagate DryRun parameter** in PowerShell function chains
9. **Use try-catch-finally** in PowerShell for cleanup operations
10. **Verify prerequisites** before starting operations
11. **Allow safe re-execution** - operations should be idempotent
12. **Document dry-run behavior** in function comments

## Related Skills and Documentation

- **`logging-patterns`** skill - Logging conventions and functions
- **`shell-patterns`** skill - Shell scripting conventions
- **`powershell-patterns`** skill - PowerShell scripting conventions
- **`testing-patterns`** skill - Testing and validation
- **`docs/ARCHITECTURE.md`** - System design principles

## Key Files

- **`dotfiles.sh`** - Entry point with errexit/nounset example
- **`src/linux/logger.sh`** - Shell logging functions including log_error
- **`src/linux/tasks.sh`** - Idempotent task implementations
- **`src/windows/Logging.psm1`** - PowerShell logging functions
- **`src/windows/*.psm1`** - All modules with DryRun parameter patterns
