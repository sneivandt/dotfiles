---
name: logging-patterns
description: >
  Logging conventions and patterns for the dotfiles project.
  Use when working with console output, persistent logging, counters, or summary reporting.
metadata:
  author: sneivandt
  version: "1.0"
---

# Logging Patterns

This skill documents logging conventions and patterns used across both Linux shell scripts and Windows PowerShell modules in the dotfiles project.

## Overview

The dotfiles project uses a consistent logging system across platforms:
- **Persistent Logs**: All operations write to log files for troubleshooting
- **Counter Tracking**: Operations counted for summary statistics
- **Consistent Formatting**: Same log levels and formats on both platforms
- **Dry-Run Support**: Preview mode shows what would happen without making changes

## Log File Locations

### Linux
```bash
$XDG_CACHE_HOME/dotfiles/install.log  # Default: ~/.cache/dotfiles/install.log
$XDG_CACHE_HOME/dotfiles/counters/    # Counter files
```

### Windows
```powershell
$env:LOCALAPPDATA\dotfiles\install.log  # Usually: C:\Users\<user>\AppData\Local\dotfiles\install.log
$env:LOCALAPPDATA\dotfiles\counters\    # Counter files
```

## Log Levels

Both platforms use identical 3-character log level codes for consistent formatting:

| Level | Code | Usage |
|-------|------|-------|
| Info | `INF` | General progress messages |
| Verbose | `VRB` | Detailed diagnostic output (only shown with -v flag) |
| Error | `ERR` | Error messages |
| Stage | `STG` | Major stage headers (prefixed with ::) |
| Dry-Run | `DRY` | Dry-run preview messages |

## Log Format

All log files use the same format on both platforms:
```
YYYY-MM-DD HH:MM:SS LVL message
2026-02-14 10:30:45 INF Checking packages
2026-02-14 10:30:46 VRB Package vim already installed
2026-02-14 10:30:47 STG Installing symlinks
```

## Shell Logging Functions (Linux)

### init_logging
Initialize the logging system at the start of installation:
```bash
init_logging  # Creates log directory and file, resets counters
```

### log_stage
Print a stage heading with `::` prefix:
```bash
log_stage "Installing packages"
# Output: :: Installing packages
```

### log_verbose
Print verbose diagnostic information (only shown with `-v` flag):
```bash
log_verbose "Package already installed: vim"
# Output only if verbose mode enabled
```

### log_dry_run
Print dry-run preview message:
```bash
log_dry_run "Would install package: vim"
# Output: Would install package: vim
```

### log_error
Print error message and exit immediately:
```bash
log_error "Package manager not found"
# Output: Error: Package manager not found
# Script exits with code 1
```

### increment_counter / get_counter
Track operation counts:
```bash
increment_counter "packages_installed"
count=$(get_counter "packages_installed")
```

### log_summary
Print installation summary at the end:
```bash
log_summary
# Output:
# :: Installation Summary
# Packages installed: 5
# Symlinks created: 12
# Log file: ~/.cache/dotfiles/install.log
```

## PowerShell Logging Functions (Windows)

### Initialize-Logging
Initialize the logging system:
```powershell
Initialize-Logging -Profile "windows"
```

### Write-Stage
Print a stage heading:
```powershell
Write-Stage "Installing packages"
# Output: :: Installing packages
```

### Write-VerboseMessage
Print verbose diagnostic (only shown with `-Verbose` flag):
```powershell
Write-VerboseMessage "Package already installed: Git.Git"
# Output only if -Verbose specified
```

### Write-DryRunMessage
Print dry-run preview:
```powershell
Write-DryRunMessage "Would install package: Git.Git"
```

### Write-ProgressMessage
Print general progress information:
```powershell
Write-ProgressMessage "Checking packages"
```

### Add-Counter / Get-Counter
Track operation counts:
```powershell
Add-Counter -CounterName "packages_installed"
$count = Get-Counter -CounterName "packages_installed"
```

### Write-InstallationSummary
Print installation summary:
```powershell
Write-InstallationSummary -DryRun:$false
```

## Counter Names

Standard counter names used across both platforms:

| Counter Name | Description |
|--------------|-------------|
| `packages_installed` | System packages installed |
| `aur_packages_installed` | AUR packages installed (Linux only) |
| `symlinks_created` | Symlinks created |
| `symlinks_removed` | Symlinks removed |
| `vscode_extensions_installed` | VS Code extensions installed |
| `powershell_modules_installed` | PowerShell modules installed |
| `systemd_units_enabled` | Systemd units enabled (Linux only) |
| `fonts_cache_updated` | Font cache updates |
| `chmod_applied` | File permissions set |
| `registry_values_set` | Registry values set (Windows only) |

## Rules for Logging

1. **Always initialize logging**: Call `init_logging` (shell) or `Initialize-Logging` (PowerShell) at the start of install/uninstall operations

2. **Use stage messages for major sections**: Group related operations under stage headings with `log_stage` or `Write-Stage`

3. **Use verbose for details**: Diagnostic information should use `log_verbose` or `Write-VerboseMessage` so it's hidden by default

4. **Track all operations**: Increment counters for all installation actions so the summary is accurate

5. **Always show summary**: Call `log_summary` or `Write-InstallationSummary` at the end of operations

6. **Clean log files**: Strip ANSI color codes before writing to log files (done automatically)

7. **Idempotent counting**: Only increment counters when actual work is performed, not when items are already configured

8. **Dry-run clarity**: In dry-run mode, use appropriate labels ("would be") and dry-run logging functions

## Example: Shell Script with Logging

```bash
# Initialize at start
init_logging

# Stage heading
log_stage "Installing packages"

# Operation with verbose output
if ! is_program_installed "vim"; then
    log_verbose "Installing package: vim"
    if ! is_dry_run; then
        sudo pacman -S --noconfirm vim
        increment_counter "packages_installed"
    else
        log_dry_run "Would install package: vim"
        increment_counter "packages_installed"
    fi
else
    log_verbose "Package already installed: vim"
fi

# Summary at end
log_summary
```

## Example: PowerShell Script with Logging

```powershell
# Initialize at start
Initialize-Logging -Profile "windows"

# Stage heading
Write-Stage "Installing packages"

# Operation with verbose output
if (-not (Test-PackageInstalled -PackageId "Git.Git")) {
    Write-VerboseMessage "Installing package: Git.Git"
    if (-not $DryRun) {
        winget install --id Git.Git --silent
        Add-Counter -CounterName "packages_installed"
    } else {
        Write-DryRunMessage "Would install package: Git.Git"
        Add-Counter -CounterName "packages_installed"
    }
} else {
    Write-VerboseMessage "Package already installed: Git.Git"
}

# Summary at end
Write-InstallationSummary -DryRun:$DryRun
```

## Cross-References

- See the `shell-patterns` skill for shell script conventions
- See the `powershell-patterns` skill for PowerShell conventions
- See the `testing-patterns` skill for testing logging output
