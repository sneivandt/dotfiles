# Architecture

This document describes the internal architecture and design of the dotfiles repository.

## Layer Hierarchy

The dotfiles system uses a layered architecture where environments can extend and build upon each other:

```
base ────────┐
             ├──> base-gui
             │
arch ────────┤
             ├──> arch-gui
             │
win ─────────┘
```

### Layer Descriptions

- **base**: Cross-platform core configuration (shell, git, vim/nvim, tooling)
- **base-gui**: GUI/editor extras (VS Code, JetBrains directories)
- **arch**: Arch Linux specific packages and pacman configuration
- **arch-gui**: Arch desktop environment (X, xmonad, picom, dunst, redshift, fonts)
- **win**: Windows/PowerShell/registry settings

### Layer Dependencies

The dependency system is encoded in `is_env_ignored()` within `src/utils.sh`:

- `arch-gui` depends on both `base-gui` and `arch`
- `base-gui` requires the `-g` flag to be active
- `arch` is only active on Arch Linux systems
- `win` is currently ignored on Linux systems

## Data Flow

### Installation Flow

```
dotfiles.sh (CLI parsing)
    ↓
commands.sh (do_install)
    ↓
tasks.sh (granular operations)
    ↓
utils.sh (predicates & helpers)
```

### Script Execution Sequence

1. **dotfiles.sh**: Entry point
   - Parses CLI flags using `getopt`
   - Prevents root execution
   - Sources `logger.sh` and `commands.sh`
   - Dispatches to appropriate command handler

2. **commands.sh**: High-level orchestration
   - `do_install`: Full environment provisioning
   - `do_test`: Static analysis and linting
   - `do_uninstall`: Remove managed symlinks
   - Ensures correct task ordering

3. **tasks.sh**: Task primitives
   - Each function is idempotent and self-guarding
   - Runs in subshells to isolate state
   - Categories: install_*, configure_*, update_*, test_*, uninstall_*

4. **utils.sh**: Helper functions
   - Predicates returning 0 (true) or 1 (false)
   - Environment detection (IS_ARCH)
   - Flag checking (is_flag_set)
   - Program detection (is_program_installed)

## Layer Configuration Files

Each layer directory (`env/<name>/`) may contain:

| File | Purpose |
|------|---------|
| `symlinks.conf` | List of files to symlink (relative paths) |
| `symlinks.json` | Alternative JSON format for symlink definitions |
| `packages.conf` | System packages to install (one per line) |
| `units.conf` | Systemd user units to enable |
| `chmod.conf` | File permission directives (mode path) |
| `submodules.conf` | Git submodules specific to this layer |
| `vscode-extensions.conf` | VS Code extensions to install |
| `fonts.conf` | Font families to ensure are installed |
| `symlinks/` | Directory containing actual files to link |

## Submodule Strategy

The repository uses Git submodules for:

1. **Third-party plugins**: Vim plugins under `env/base/symlinks/vim/pack/plugins/`
2. **Optional layers**: Environment layers can be submodules (e.g., `env/arch`)

Submodules are initialized and updated during:
- Installation (`do_install`)
- Testing (`do_test`)
- Uninstallation (`do_uninstall`)

The system handles:
- Recursive submodule initialization
- Selective submodule updates based on active layers
- Skipping uninitialized or modified submodules during updates

## Symlink Management

Symlinks follow a declarative pattern:

1. **Source**: `env/<layer>/symlinks/<path>`
2. **Target**: `~/.<path>` (always prefixed with dot)
3. **Example**: 
   - Config entry: `config/nvim`
   - Source: `env/base/symlinks/config/nvim`
   - Target: `~/.config/nvim`

### Idempotency

- `is_symlink_installed()` checks if symlink is already correct
- Existing files are replaced (with removal, not backup)
- Parent directories are created as needed
- Re-running creates no duplicate operations

## Extension Points

### Adding a New Layer

1. Create `env/<name>/` directory
2. Add layer-specific configuration files as needed
3. Update `is_env_ignored()` in `src/utils.sh` if layer has dependencies
4. Add layer documentation in `env/<name>/README.md`

### Adding New Task Types

1. Add task function to `src/tasks.sh`
2. Follow naming conventions: `install_*`, `configure_*`, etc.
3. Wrap implementation in subshell `( )`
4. Add idempotency guards at function start
5. Use `log_stage`, `log_verbose`, `log_error` for output
6. Integrate into appropriate command in `commands.sh`

### Adding New Utilities

1. Add predicate or helper to `src/utils.sh`
2. Return 0 for success/true, 1 for failure/false
3. Keep logic minimal and focused
4. Document parameters and return values in comments

## Windows Support

Windows support is provided through:

- **dotfiles.ps1**: PowerShell entry point
- **src/script.psm1**: Core PowerShell module
- **win/src/**: Windows-specific modules (Registry, VsCode, Symlinks)
- **env/win/**: Windows-specific configurations

The Windows implementation mirrors the Linux structure but uses PowerShell conventions.

## Testing Infrastructure

### Static Analysis

- **ShellCheck**: Validates POSIX sh compliance for all shell scripts
- **PSScriptAnalyzer**: Validates PowerShell scripts (when pwsh available)

### Test Mode

The `--test` flag runs:
1. Repository update
2. Submodule initialization
3. Static analysis with ShellCheck and PSScriptAnalyzer

### Docker Testing

A Docker image provides isolated testing:
- Based on Ubuntu Jammy
- Includes required tools (git, vim, zsh, shellcheck)
- Runs full installation in clean environment
- Published via GitHub Actions on master branch
