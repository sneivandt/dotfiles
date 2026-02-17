# Architecture

Technical documentation covering the implementation and design of the dotfiles management system.

## Overview

This dotfiles project is a cross-platform, profile-based configuration management system built around a **Rust core engine** (`cli/`). Thin shell wrappers (`dotfiles.sh` on Linux, `dotfiles.ps1` on Windows) download or build the binary and forward all arguments to it. Configuration lives in declarative INI files (`conf/`), and the binary handles parsing, profile resolution, platform detection, and task execution.

## Design Principles

### 1. Cross-Platform Compatibility

**Challenge**: Support both Linux (Arch, Debian, etc.) and Windows with a unified configuration approach.

**Solution**:
- Single Rust binary compiled for both platforms
- Thin platform-native entry points (`dotfiles.sh`, `dotfiles.ps1`) that download or build the binary
- Shared configuration format (INI files in `conf/`)
- Compile-time platform detection via `cfg!(target_os)` plus runtime checks (e.g. `/etc/arch-release`)
- Profile system to exclude platform-specific files and configuration

### 2. Idempotency

**Challenge**: Allow the tool to be run multiple times safely.

**Solution**:
- Every task checks existence/state before acting
- Operations that are already complete are skipped
- Skipped operations are logged in verbose mode
- No side effects on re-runs

### 3. Profile-Based Configuration

**Challenge**: Support multiple environments (headless server, desktop, Windows) from one repository.

**Solution**:
- Profile definitions in `conf/profiles.ini` map to category exclusions
- Git sparse checkout excludes files by category
- INI section names carry category tags; the binary filters them against the active profile
- Automatic OS detection provides safety overrides

### 4. Binary Distribution

**Challenge**: End users should not need a Rust toolchain installed.

**Solution**:
- GitHub Actions builds release binaries on every push to `master` that touches `cli/` or `conf/`
- The release workflow (`.github/workflows/release.yml`) publishes Linux and Windows binaries with SHA-256 checksums
- The shell wrappers download the latest release and cache the version for one hour (`.dotfiles-version-cache`)
- A `--build` flag builds from source for development

## High-Level Architecture

```
┌─────────────┐      ┌─────────────┐
│ dotfiles.sh  │      │ dotfiles.ps1│   Thin wrappers
│  (Linux)     │      │  (Windows)  │   download/build binary
└──────┬───────┘      └──────┬──────┘
       │                     │
       ▼                     ▼
┌──────────────────────────────────────┐
│         cli/ (Rust binary)          │
│                                      │
│  cli.rs         — clap argument      │
│                   parsing            │
│  commands/      — install, uninstall,│
│                   test, version      │
│  config/        — INI loading &      │
│                   profile resolution │
│  tasks/         — Task trait impls   │
│  platform.rs    — OS detection       │
│  logging.rs     — structured logging │
│  exec.rs        — subprocess exec    │
└──────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────┐
│            conf/ (INI files)         │
│  packages.ini  symlinks.ini          │
│  profiles.ini  manifest.ini          │
│  units.ini     fonts.ini             │
│  registry.ini  vscode-extensions.ini │
│  chmod.ini     copilot-skills.ini    │
└──────────────────────────────────────┘
```

## Component Architecture

### Shell Wrappers

#### `dotfiles.sh` (Linux)

POSIX shell script that:
- Checks for a `--build` flag; if set, runs `cargo build --release` in `cli/` and executes the resulting binary
- Otherwise, checks the version cache (`.dotfiles-version-cache`, max age 3600 s)
- If stale or missing, queries the GitHub Releases API for the latest tag, downloads the binary to `bin/dotfiles`, and verifies its SHA-256 checksum
- Forwards all remaining arguments to the binary with `--root`

#### `dotfiles.ps1` (Windows)

PowerShell script with identical logic:
- `-Build` switch builds from source with `cargo build --release`
- Otherwise downloads `dotfiles-windows-x86_64.exe` from GitHub Releases
- Verifies checksum and caches version
- Forwards arguments to the binary

### Rust Core (`cli/`)

The binary is built with `cargo` from `cli/Cargo.toml`. Key dependencies:

- **clap** — CLI argument parsing with derive macros
- **anyhow** — error handling and context propagation

#### Entry Point (`main.rs`)

Parses CLI arguments via `cli::Cli`, creates a `Logger`, and dispatches to the matching command handler.

#### CLI (`cli.rs`)

Defines the command structure using clap derive:

```
dotfiles [-v] [-p PROFILE] [-d] [--root DIR] <COMMAND>

Commands:
  install     Install dotfiles and configure system
  uninstall   Remove installed dotfiles
  test        Run self-tests and validation
  version     Print version information

Install options:
  --skip TASK,...   Skip specific tasks
  --only TASK,...   Run only specific tasks
```

#### Commands (`commands/`)

- **`install.rs`** — Resolves profile, loads `Config`, builds the task list, filters by `--skip`/`--only`, and executes each task via `tasks::execute()`
- **`uninstall.rs`** — Removes managed symlinks
- **`test.rs`** — Runs configuration validation

#### Config (`config/`)

`Config::load()` reads all INI files from `conf/` and filters sections against the active profile's categories:

| Module | File | Description |
|---|---|---|
| `profiles.rs` | `profiles.ini` | Profile resolution and category computation |
| `ini.rs` | (all) | Generic INI parser |
| `packages.rs` | `packages.ini` | System packages (pacman, AUR, winget) |
| `symlinks.rs` | `symlinks.ini` | Symlink mappings |
| `units.rs` | `units.ini` | Systemd units (Linux only) |
| `fonts.rs` | `fonts.ini` | Font families |
| `chmod.rs` | `chmod.ini` | File permissions |
| `vscode.rs` | `vscode-extensions.ini` | VS Code extensions |
| `copilot_skills.rs` | `copilot-skills.ini` | GitHub Copilot CLI skills |
| `registry.rs` | `registry.ini` | Windows registry entries |
| `manifest.rs` | `manifest.ini` | Sparse checkout file mappings |

#### Tasks (`tasks/`)

Each task implements the `Task` trait:

```rust
pub trait Task {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
```

A shared `Context` struct carries the loaded `Config`, `Platform`, `Logger`, and flags (`dry_run`, `verbose`, `home` path).

The `execute()` function runs a task, recording the result (`Ok`, `Skipped`, `DryRun`, `Failed`) in the logger.

**Implemented tasks** (`cli/src/tasks/`):
- `sparse_checkout` — Configure git sparse checkout
- `update` — Update repository
- `hooks` — Install git hooks
- `packages` — Install system packages (pacman, paru, AUR)
- `symlinks` — Create symlinks
- `vscode` — Install VS Code extensions
- `copilot_skills` — Download Copilot CLI skills
- `chmod` — Apply file permissions
- `shell` — Configure default shell
- `fonts` — Check/install fonts
- `systemd` — Enable systemd units
- `registry` — Apply Windows registry settings
- `git_config` — Configure git settings

#### Platform Detection (`platform.rs`)

The `Platform` struct detects the OS at compile time (`cfg!(target_os)`) and checks for Arch Linux at runtime (`/etc/arch-release`). It exposes helpers like `is_linux()`, `is_windows()`, `is_arch`, and `excludes_category()` which tasks use to decide whether to run.

#### Logging (`logging.rs`)

Structured logger that:
- Prints stage headers for each task
- Records task outcomes (Ok, Skipped, DryRun, Failed)
- Tracks operation counters
- Prints a summary at the end of execution

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

1. `Config::load()` reads each INI file from `conf/`
2. Each config module parses sections and entries
3. Sections are filtered against the active profile's `active_categories`
4. Platform-specific configs (e.g. registry on Windows, units on Linux) are loaded conditionally

### Sparse Checkout System

Git's sparse checkout feature controls which files are checked out.

**Implementation flow**:
1. Resolve profile from `profiles.ini`
2. Compute excluded categories from profile definition plus platform detection
3. Load file mappings from `manifest.ini`
4. Build exclusion patterns
5. Configure `git sparse-checkout set`

**Pattern logic** (manifest.ini):
- Uses OR logic for exclusions
- `[arch,desktop]` means "exclude if arch OR desktop is excluded"
- Ensures files common to multiple categories are excluded appropriately

### Error Handling

The binary uses `anyhow::Result` throughout. Each config loader and task adds context via `.context()`:

```rust
packages::load(&conf.join("packages.ini"), active_categories)
    .context("loading packages.ini")?;
```

Task failures are caught by `tasks::execute()` and recorded as `TaskStatus::Failed` — the binary continues executing remaining tasks and reports all failures in the summary.

## Testing Architecture

### Rust Tests

- **Unit tests**: Inline `#[cfg(test)]` modules in source files (e.g. `platform.rs`, `cli.rs`, `config/ini.rs`)

The project uses `assert_cmd` and `predicates` as dev-dependencies for CLI-level testing.

### Configuration Validation

The `test` command validates:
- INI file syntax
- Section format
- Profile definitions
- File references

### CI Pipeline

GitHub Actions CI (`.github/workflows/ci.yml`) runs on pull requests:

| Job | What it checks |
|---|---|
| `rust-fmt` | `cargo fmt --check` |
| `rust-clippy` | `cargo clippy -- -D warnings` |
| `rust-test` | `cargo test` |
| `build-linux` | Release build + binary smoke test |
| `build-windows` | Release build + binary smoke test |
| `script-lint` | shellcheck on `dotfiles.sh` and `install.sh` |
| `integration-linux` | Dry-run install for `base` and `desktop` profiles |
| `integration-windows` | Dry-run install for `windows` profile |
| `test-docker` | Docker image build + smoke test |
| `test-git-hooks` | Pre-commit hook tests |

### Release Pipeline

GitHub Actions release (`.github/workflows/release.yml`) triggers on push to `master` when `cli/` or `conf/` change:
1. Builds Linux and Windows release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with version tag `v0.1.<run_number>`

## Extension Points

### Adding New Tasks

1. Create a new file in `cli/src/tasks/` implementing the `Task` trait
2. Add the module to `cli/src/tasks/mod.rs`
3. Add the task to the task list in `commands/install.rs`

### Adding New Configuration Types

1. Create INI file in `conf/`
2. Add a config parser in `cli/src/config/`
3. Add the field to the `Config` struct and load it in `Config::load()`
4. Create a task in `cli/src/tasks/` that consumes the config
5. Document in CONFIGURATION.md

### Adding Custom Profiles

1. Define in `conf/profiles.ini`
2. Add sections to configuration files
3. Map files in `conf/manifest.ini`

## Performance Considerations

### Binary Distribution

- Pre-compiled binaries eliminate the need for a Rust toolchain on end-user machines
- The version cache (`.dotfiles-version-cache`, 1 hour TTL) avoids GitHub API calls on every invocation
- Offline fallback: if GitHub is unreachable and a local binary exists, it is used as-is

### Compiled Binary

- Release builds use LTO, single codegen unit, and size optimization (`opt-level = "z"`)
- Binary is stripped in CI for minimal size
- Startup and execution are significantly faster than interpreted shell scripts

### Sparse Checkout Benefits

- Reduces disk usage (only relevant files checked out)
- Faster git operations (fewer files to track)
- Cleaner workspace (no irrelevant files)

## Security Considerations

### Git Hooks

Pre-commit hook scans for sensitive data:
- API keys, tokens, passwords
- Private keys
- Cloud provider credentials
- Generic high-entropy secrets

### Binary Verification

- Release binaries include SHA-256 checksums (`checksums.sha256`)
- Both shell wrappers verify the checksum after download

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
