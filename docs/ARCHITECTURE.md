# Architecture

Technical documentation covering the implementation and design of the dotfiles management system.

## Overview

This dotfiles project is designed as a cross-platform, profile-based configuration management system with the following architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                     Entry Points                             │
│  dotfiles.sh (Linux)          dotfiles.ps1 (Windows)        │
└────────────────┬──────────────────────────┬─────────────────┘
                 │                          │
     ┌───────────▼──────────┐   ┌──────────▼─────────┐
     │  src/linux/          │   │  src/windows/      │
     │  ├─ commands.sh      │   │  ├─ Dotfiles.psm1  │
     │  ├─ tasks.sh         │   │  ├─ Packages.psm1  │
     │  ├─ utils.sh         │   │  ├─ Symlinks.psm1  │
     │  └─ logger.sh        │   │  └─ ...            │
     └──────────┬───────────┘   └────────┬───────────┘
                │                        │
                └────────┬───────────────┘
                         │
            ┌────────────▼────────────┐
            │   Configuration Files   │
            │   conf/*.ini            │
            │   ├─ profiles.ini       │
            │   ├─ manifest.ini       │
            │   ├─ symlinks.ini       │
            │   ├─ packages.ini       │
            │   └─ ...                │
            └────────────┬────────────┘
                         │
            ┌────────────▼────────────┐
            │   Source Files          │
            │   symlinks/             │
            └─────────────────────────┘
```

## Design Principles

### 1. Cross-Platform Compatibility

**Challenge**: Support both Linux (Arch, Debian, etc.) and Windows with a unified configuration approach.

**Solution**:
- Separate entry points (`dotfiles.sh` for Linux, `dotfiles.ps1` for Windows)
- Shared configuration format (INI files)
- Platform-specific logic isolated in separate modules
- Profile system to exclude platform-specific files

### 2. Idempotency

**Challenge**: Allow script to be run multiple times safely.

**Solution**:
- Check existence/state before every operation
- Skip operations that are already complete
- Log skipped operations in verbose mode
- No side effects on re-runs

**Implementation examples**:
```bash
# Check before creating symlink
if [ -L "$target" ] && [ "$(readlink "$target")" = "$source" ]; then
  log_verbose "Skipping: already correct"
  return
fi
```

```powershell
# Check before installing package
if (Test-PackageInstalled $packageId) {
  Write-Verbose "Skipping $packageId: already installed"
  continue
}
```

### 3. Profile-Based Configuration

**Challenge**: Support multiple environments (headless server, desktop, Windows) from one repository.

**Solution**:
- Profile definitions map to category exclusions
- Git sparse checkout excludes files by category
- Configuration sections filtered by active categories
- Automatic OS detection provides safety overrides

### 4. POSIX Shell Compatibility

**Challenge**: Work across different Linux distributions with varying shell implementations.

**Solution**:
- Use `#!/bin/sh` for maximum compatibility
- Avoid Bash-specific features (arrays, process substitution)
- Test with shellcheck for POSIX compliance
- Document any Bash requirements explicitly

## Component Architecture

### Linux Components

#### Entry Point (`dotfiles.sh`)

Main script that:
- Parses command-line arguments
- Sources supporting modules
- Dispatches to command handlers
- Handles errors and exits

#### Commands Module (`src/linux/commands.sh`)

High-level orchestration functions:
- `do_install` - Coordinates installation steps
- `do_uninstall` - Removes managed symlinks
- `do_test` - Runs validation tests

Each command function delegates to task primitives.

#### Tasks Module (`src/linux/tasks.sh`)

Granular, idempotent task functions:
- `install_sparse_checkout` - Configures git sparse checkout
- `install_packages` - Installs system packages
- `install_symlinks` - Creates symlinks
- `install_systemd_units` - Enables systemd units
- `install_fonts` - Installs fonts
- `install_vscode_extensions` - Installs VS Code extensions
- `install_pwsh_modules` - Installs PowerShell modules
- `install_git_hooks` - Installs repository hooks
- And more...

**Task Function Pattern**:
```bash
task_name()
{(
  # Wrapped in subshell for isolation
  # Check prerequisites
  if ! is_program_installed "tool"; then
    log_verbose "Skipping: tool not installed"
    return
  fi

  # Do work
  log_stage "Task Name"

  # Idempotent operations
  if is_dry_run; then
    log_dry_run "Would perform action"
  else
    log_verbose "Performing action"
    # actual work
  fi
)}
```

#### Utils Module (`src/linux/utils.sh`)

Helper functions and predicates:
- `read_ini_section` - Parse INI configuration files
- `should_include_profile_tag` - Check if section matches active profile
- `is_program_installed` - Check if program is available
- `is_dry_run` - Check if dry-run mode is active
- Profile selection and persistence functions
- Sparse checkout configuration

#### Logger Module (`src/linux/logger.sh`)

Logging abstraction:
- `init_logging` - Initialize log file and counters
- `log_stage` - Print stage headers (once per subshell)
- `log_verbose` - Print verbose messages
- `log_error` - Print errors and exit
- `log_dry_run` - Print dry-run actions
- Operation counters and summary

### Windows Components

#### Entry Point (`dotfiles.ps1`)

Thin wrapper that:
- Imports `Dotfiles.psm1`
- Calls `Install-Dotfiles` with parameters
- Handles errors

#### Dotfiles Module (`src/windows/Dotfiles.psm1`)

Main module that orchestrates installation:
- Loads supporting modules
- Executes installation steps in order
- Exports `Install-Dotfiles` command

#### Supporting Modules

- **`Git.psm1`** - Git configuration and repository updates
- **`GitHooks.psm1`** - Git hooks installation
- **`Module.psm1`** - PowerShell module installation
- **`Packages.psm1`** - Package management (winget)
- **`Registry.psm1`** - Registry configuration
- **`Symlinks.psm1`** - Symlink creation
- **`VsCodeExtensions.psm1`** - VS Code extension installation
- **`Profile.psm1`** - Profile filtering and INI parsing
- **`Logging.psm1`** - Logging and operation counters

### Configuration System

#### INI File Format

All configuration files use standard INI format:

```ini
[section-name]
entry-one
entry-two
```

**Profile name distinction**:
- `profiles.ini`: Profile names use hyphens: `[arch-desktop]`
- Other files: Section names use comma-separated categories: `[arch,desktop]`

**Exception**: `registry.ini` uses `key = value` format.

#### Configuration Processing

1. Read configuration file
2. Parse sections
3. Filter sections by active profile
4. Process entries in matching sections

**Implementation**:
```bash
# Get all sections
sections="$(grep -E '^\[.+\]$' "$config" | tr -d '[]')"

# Process matching sections
for section in $sections; do
  if ! should_include_profile_tag "$section"; then
    log_verbose "Skipping section [$section]"
    continue
  fi

  # Process entries
  read_ini_section "$config" "$section" | while IFS='' read -r item; do
    # Handle item
  done
done
```

### Sparse Checkout System

Git's sparse checkout feature controls which files are checked out.

**Implementation flow**:
1. Read profile from `profiles.ini`
2. Parse excluded categories
3. Apply OS detection overrides
4. Read file mappings from `manifest.ini`
5. Build exclusion patterns
6. Configure `git sparse-checkout set`

**Pattern logic** (manifest.ini):
- Uses OR logic for exclusions
- `[arch,desktop]` means "exclude if arch OR desktop is excluded"
- Ensures files common to multiple categories are excluded appropriately

### Logging System

#### Linux

Log file: `${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.log`

**Initialization**:
```bash
init_logging() {
  # Create log directory
  # Initialize log file with timestamp
  # Initialize operation counters
}
```

**Stage logging**:
```bash
log_stage() {
  # Print stage header once per subshell
  # Uses _work flag to track if already printed
  # Resets in new subshell (task function pattern)
}
```

**Operation counters**:
- Global variables track counts
- Incremented in both dry-run and real modes
- Displayed in summary

#### Windows

Log file: `%LOCALAPPDATA%\dotfiles\install.log`

**Similar structure** but using PowerShell:
- `Initialize-Logging` - Set up log file
- `Write-VerboseMessage` - Log verbose details
- Stage headers with `$act` flag
- Operation counters in module-scoped variables

### Error Handling

#### Linux

```bash
set -o errexit  # Exit on error
set -o nounset  # Exit on undefined variable
```

**Controlled errors**:
```bash
if ! command; then
  log_error "Error message"
  # Exits with status 1
fi
```

#### Windows

```powershell
try {
  Import-Module -ErrorAction Stop
  # operations
} catch {
  Write-Error "Error: $_"
  exit 1
}
```

## Testing Architecture

### Static Analysis

**Linux**:
- shellcheck for shell scripts
- PSScriptAnalyzer for PowerShell (when pwsh available)
- Configuration validation

**Windows**:
- PSScriptAnalyzer for PowerShell scripts

### Configuration Validation

Validates:
- INI file syntax
- Section format
- Profile definitions
- File references

### Idempotency Tests

Runs installation twice and verifies:
- No errors on second run
- Consistent state
- Proper skip messages

### CI Testing

GitHub Actions workflows:
- Test all profiles
- Test both platforms (Linux, Windows)
- Run static analysis
- Run configuration validation
- Test Docker build

## Extension Points

### Adding New Configuration Types

1. Create INI file in `conf/`
2. Add task function to process it
3. Add to installation sequence
4. Document in CONFIGURATION.md

### Adding New Platforms

1. Create platform-specific modules
2. Add profile and category for platform
3. Update manifest.ini with platform files
4. Create platform-specific entry point

### Adding Custom Profiles

1. Define in `profiles.ini`
2. Add sections to configuration files
3. Map files in `manifest.ini`

## Performance Considerations

### Sparse Checkout Benefits

- Reduces disk usage (only relevant files checked out)
- Faster git operations (fewer files to track)
- Cleaner workspace (no irrelevant files)

### Parallel Operations

Most operations are sequential for simplicity and reliability. Potential parallelization opportunities:
- Package installation (requires dependency analysis)
- Symlink creation (safe to parallelize)
- VS Code extension installation (already handled by `code` CLI)

### Caching

- Package manager caches (pacman, winget) handle package caching
- Git sparse checkout caches file exclusions
- No application-level caching needed

## Security Considerations

### Git Hooks

Pre-commit hook scans for sensitive data:
- API keys, tokens, passwords
- Private keys
- Cloud provider credentials
- Generic high-entropy secrets

### Symlink Safety

- No automatic backup of existing files
- User must manually handle existing files
- Prevents accidental data loss

### Registry Safety (Windows)

- Only modifies HKCU (user scope)
- No HKLM (system scope) modifications
- Dry-run mode available for preview

### Package Installation

- Uses official package managers (pacman, winget)
- No automatic execution of arbitrary scripts
- User reviews `packages.ini` before installation

## See Also

- [Profile System](PROFILES.md) - Profile implementation details
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Contributing Guide](CONTRIBUTING.md) - Development guidelines
- [Testing Documentation](TESTING.md) - Testing procedures
