# Architecture

Technical documentation covering the implementation and design of the dotfiles management system.

## Overview

This dotfiles project is a cross-platform, profile-based configuration management system built around a **Rust core engine** (`cli/`). Thin shell wrappers (`dotfiles.sh` on Linux, `dotfiles.ps1` on Windows) download or build the binary and forward all arguments to it. Configuration lives in declarative TOML files (`conf/`), and the binary handles parsing, profile resolution, platform detection, and task execution.

## Design Principles

### 1. Cross-Platform Compatibility

**Challenge**: Support both Linux (Arch, Debian, etc.) and Windows with a unified configuration approach.

**Solution**:
- Single Rust binary compiled for both platforms
- Thin platform-native entry points (`dotfiles.sh`, `dotfiles.ps1`) that download or build the binary
- Shared configuration format (TOML files in `conf/`)
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
- Profile definitions in `conf/profiles.toml` map to category exclusions
- Git sparse checkout excludes files by category
- TOML table names carry category tags; the binary filters them against the active profile
- Automatic OS detection provides safety overrides

### 4. Binary Distribution

**Challenge**: End users should not need a Rust toolchain installed.

**Solution**:
- GitHub Actions builds release binaries on every push to `master` that touches `cli/` or `conf/`
- The release workflow (`.github/workflows/release.yml`) publishes Linux and Windows binaries with SHA-256 checksums
- The shell wrappers download the latest release and cache the version for one hour (`bin/.dotfiles-version-cache`)
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
│  config/        — TOML loading &     │
│                   profile resolution │
│  tasks/         — Task trait impls   │
│  platform.rs    — OS detection       │
│  logging.rs     — structured logging │
│  exec.rs        — subprocess exec    │
└──────────────────────────────────────┘
       │
       ▼
┌────────────────────────────────────────────┐
│            conf/ (TOML files)              │
│  packages.toml      symlinks.toml          │
│  profiles.toml      manifest.toml          │
│  systemd-units.toml vscode-extensions.toml │
│  registry.toml      copilot-skills.toml    │
│  chmod.toml                                │
└────────────────────────────────────────────┘
```

## Component Architecture

### Shell Wrappers

#### `dotfiles.sh` (Linux)

POSIX shell script that:
- Checks for a `--build` flag; if set, runs `cargo build --release` in `cli/` and executes the resulting binary
- Otherwise, checks the version cache (`bin/.dotfiles-version-cache`, max age 3600 s)
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
dotfiles [-v] [-p PROFILE] [-d] [--no-parallel] [--root DIR] <COMMAND>

Commands:
  install     Install dotfiles and configure system
  uninstall   Remove installed dotfiles
  test        Run self-tests and validation
  version     Print version information

Install options:
  --skip TASK,...   Skip specific tasks
  --only TASK,...   Run only specific tasks
```

The wrapper scripts (`dotfiles.sh` / `dotfiles.ps1`) expose only `-p`, `-d`, `-v`,
and `--build`. Flags like `--skip`, `--only`, `--root`, and `--no-parallel` are
available only when invoking the binary directly.

#### Commands (`commands/`)

- **`install.rs`** — Uses `CommandRunner` to resolve the profile, load `Config`, build the task list, filter by `--skip`/`--only`, and execute each task via `tasks::execute()`
- **`uninstall.rs`** — Removes managed symlinks
- **`test.rs`** — Runs configuration validation

#### Config (`config/`)

`Config::load()` reads all TOML files from `conf/` and filters sections against the active profile's categories:

| Module | File | Description |
|---|---|---|
| `profiles.rs` | `profiles.toml` | Profile resolution and category computation |
| `toml_loader.rs` | (all) | Generic TOML loader |
| `packages.rs` | `packages.toml` | System packages (pacman, AUR, winget) |
| `symlinks.rs` | `symlinks.toml` | Symlink mappings |
| `systemd_units.rs` | `systemd-units.toml` | Systemd units (Linux only) |
| `chmod.rs` | `chmod.toml` | File permissions |
| `vscode_extensions.rs` | `vscode-extensions.toml` | VS Code extensions |
| `copilot_skills.rs` | `copilot-skills.toml` | GitHub Copilot CLI skills |
| `registry.rs` | `registry.toml` | Windows registry entries |
| `manifest.rs` | `manifest.toml` | Sparse checkout file mappings |

#### Tasks (`tasks/`)

Each task implements the `Task` trait:

```rust
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Stable TypeId for dependency matching.
    fn task_id(&self) -> TypeId { TypeId::of::<Self>() }

    /// TypeIds of tasks that must complete before this one starts.
    fn dependencies(&self) -> &[TypeId] { &[] }

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
```

A shared `Context` struct carries the loaded `Config`, `Platform`, `Logger`, and flags (`dry_run`, `parallel`, `home` path). Task-specific dependencies are injected via constructors: `UpdateRepository` and `ReloadConfig` share an `Arc<AtomicBool>` (`repo_updated`) to coordinate config reloading, and hook tasks (`InstallGitHooks`, `UninstallGitHooks`) hold an `Arc<dyn FileSystemOps>` for testable filesystem access.

The `execute()` function runs a task, recording the result (`Ok`, `Skipped`, `DryRun`, `Failed`) in the logger.

**Implemented tasks** (`cli/src/tasks/`, executed as soon as dependencies allow):
- `developer_mode` — Enable Windows developer mode (required for symlinks)
- `sparse_checkout` — Configure git sparse checkout
- `update` — Update repository (`git pull --ff-only`)
- `reload_config` — Reload config from disk after `update` pulls new commits
- `git_config` — Configure git settings (Windows: autocrlf, symlinks, credential helper)
- `hooks` — Install git hooks
- `packages` — Install system packages (pacman or winget)
- `paru` — Bootstrap paru AUR helper (Arch Linux only)
- `aur_packages` — Install AUR packages via paru (Arch Linux only)
- `symlinks` — Create symlinks
- `chmod` — Apply file permissions
- `shell` — Configure default shell
- `systemd` — Enable systemd units
- `registry` — Apply Windows registry settings
- `vscode` — Install VS Code extensions
- `copilot_skills` — Download Copilot CLI skills

#### Platform Detection (`platform.rs`)

The `Platform` struct detects the OS at compile time (`cfg!(target_os)`) and checks for Arch Linux at runtime (`/etc/arch-release`).

**Basic Platform Queries:**
- `is_linux()` — returns true if running on Linux
- `is_windows()` — returns true if running on Windows
- `is_arch` — public field, true if running on Arch Linux

**Capability-Based Methods** (more expressive platform checks):
- `supports_chmod()` — returns true if platform supports POSIX file permissions
- `supports_systemd()` — returns true if platform uses systemd
- `has_registry()` — returns true if platform uses Windows Registry
- `is_arch_linux()` — returns true if running on Arch Linux
- `uses_pacman()` — returns true if platform uses pacman package manager
- `supports_aur()` — returns true if platform supports AUR packages

**Display Methods:**
- `description()` — returns "Arch Linux", "Linux", or "Windows"
- `to_string()` / `Display` — same as `description()`

**Profile Integration:**
- `excludes_category(category)` — returns true if the given category is incompatible with this platform

Tasks use these methods in their `should_run()` implementation to determine platform compatibility. For example:

```rust
fn should_run(&self, ctx: &Context) -> bool {
    ctx.platform.supports_systemd() && !ctx.config_read().units.is_empty()
}
```

This is more expressive than `ctx.platform.is_linux()` because it clearly states *why* the platform matters (systemd support) rather than just checking the OS type.

#### Logging (`logging.rs`)

Structured logger that:
- Prints stage headers for each task
- Records task outcomes (Ok, Skipped, DryRun, Failed)
- Tracks operation counters
- Prints a summary at the end of execution

A **diagnostic log** is written alongside the main log to
`$XDG_CACHE_HOME/dotfiles/<command>.diag.log`.  Unlike the main log (which
replays buffered parallel output per-task), the diagnostic log captures every
event immediately with microsecond-resolution wall-clock timestamps and thread
identification, providing the true chronological view of parallel execution.
Event tags cover the full lifecycle: logger messages (`STAGE`, `INFO`, `DEBUG`,
`WARN`, `ERROR`, `DRYRUN`), task scheduling (`TASK_WAIT`, `TASK_START`,
`TASK_DONE`, `TASK_SKIP`), and resource processing (`RES_CHECK`, `RES_APPLY`,
`RES_RESULT`, `RES_REMOVE`).

### Configuration System

#### TOML File Format

All configuration files use TOML format. Items are declared as typed arrays under section headers:

```toml
[section-name]
items = [
  "entry-one",
  "entry-two",
]
```

**Section name conventions**:
- `profiles.toml`: Profile names: `[base]`, `[desktop]`
- Other files: Section names use hyphen-separated categories: `[arch-desktop]`
  - This indicates the section requires ALL listed categories to be active (AND logic)
  - Example: `[arch-desktop]` is only processed when both `arch` AND `desktop` are not excluded

`registry.toml` uses a different structure — logical section names with a `path` key and a nested `[section.values]` subtable.

#### Configuration Processing

1. `Config::load()` reads each TOML file from `conf/`
2. Each config module parses sections and entries
3. Sections are filtered against the active profile's `active_categories`
4. Platform-specific configs (e.g. registry on Windows, units on Linux) are loaded conditionally

### Sparse Checkout System

Git's sparse checkout feature controls which files are checked out.

**Implementation flow**:
1. Resolve profile from `profiles.toml`
2. Compute excluded categories from profile definition plus platform detection
3. Load file mappings from `manifest.toml`
4. Build exclusion patterns
5. Configure `git sparse-checkout set`

**Pattern logic** (manifest.toml):
- Uses OR logic for exclusions
- `[arch-desktop]` means "exclude if arch OR desktop is excluded"
- Ensures files common to multiple categories are excluded appropriately

### Error Handling

The binary uses `anyhow::Result` throughout. Each config loader and task adds context via `.context()`:

```rust
packages::load(&conf.join("packages.toml"), active_categories)
    .context("loading packages.toml")?;
```

Task failures are caught by `tasks::execute()` and recorded as `TaskStatus::Failed` — the binary continues executing remaining tasks and reports all failures in the summary.

## Testing Architecture

### Rust Tests

- **Unit tests**: Inline `#[cfg(test)]` modules in source files (e.g. `platform.rs`, `cli.rs`, `config/toml_loader.rs`, `tasks/*.rs`)
- **Integration tests**: Separate test binaries in `cli/tests/` (`install_command.rs`, `uninstall_command.rs`, `test_command.rs`), using `IntegrationTestContext` and `TestContextBuilder` helpers from `cli/tests/common/mod.rs`
- **Snapshot tests**: Task list snapshots via the `insta` crate (`cli/tests/snapshots/`). Update with `INSTA_UPDATE=unseen cargo test` or `cargo insta review`

The project uses `tempfile` as a dev-dependency for tests that need temporary directories.

### Configuration Validation

The `test` command validates:
- TOML file syntax
- Section format
- Profile definitions
- File references

### CI Pipeline

GitHub Actions CI (`.github/workflows/ci.yml`) runs on pull requests:

| Job | What it checks |
|---|---|
| `rust` | Formatting, Clippy linting, and unit/integration tests (matrix) |
| `lint` | ShellCheck and PSScriptAnalyzer (matrix) |
| `build` | Release build + smoke test on Linux and Windows (matrix) |
| `integration` | Dry-run install per profile and platform (matrix) |
| `test-applications` | Git, zsh, vim, nvim behavior (matrix) |
| `test-docker` | Docker image build + smoke test |
| `test-git-hooks` | Pre-commit sensitive data detection and Rust formatting/clippy linting |

### Release Pipeline

GitHub Actions release (`.github/workflows/release.yml`) triggers on push to `master` when `cli/` or `conf/` change:
1. Builds Linux and Windows release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with version tag `v0.1.<run_number>`

## Extension Points

### Adding New Tasks

1. Create a new file in `cli/src/tasks/` implementing the `Task` trait
2. Add the module to `cli/src/tasks/mod.rs`
3. Add the task to `all_install_tasks()` in `cli/src/tasks/mod.rs`

### Adding New Configuration Types

1. Create TOML file in `conf/`
2. Add a config parser in `cli/src/config/`
3. Add the field to the `Config` struct and load it in `Config::load()`
4. Create a task in `cli/src/tasks/` that consumes the config
5. Document in CONFIGURATION.md

### Adding Custom Profiles

1. Define in `conf/profiles.toml`
2. Add sections to configuration files
3. Map files in `conf/manifest.toml`

## Performance Considerations

### Parallel Task Execution

Tasks are executed in parallel using a dependency-graph scheduler.  Each task
declares its dependencies using the `task_deps!` macro (exported from
`tasks/mod.rs`), which implements `Task::dependencies()` returning `TypeId`s of
prerequisite task structs.  The scheduler uses `std::thread::scope` to spawn
one OS thread per task and a `Condvar`-based `TaskGraph` to block each task
until its dependencies are marked complete.  OS threads are used deliberately
— blocking on a `Condvar` inside a Rayon worker would exhaust Rayon's
fixed-size thread pool and deadlock when the pool is smaller than the number
of tasks with unsatisfied dependencies (common on 2-vCPU CI runners).

**How it works:**

- Each task is spawned into an OS thread via `std::thread::scope`
- `TaskGraph::wait_for_deps()` blocks the task until all declared dependencies
  have called `TaskGraph::mark_complete()`
- Tasks with no dependencies (or whose dependencies were filtered out) start
  immediately
- Each task's console output is captured in a per-task `BufferedLog`; when the
  task completes, the buffer is flushed atomically under a `flush_lock` so
  output from different tasks never interleaves
- A dim status line (`▹ task1, task2, ...`) shows which tasks are currently
  running, updated on every task start and completion
- Cycle detection (Kahn's algorithm) runs before scheduling; if a cycle is
  found, the scheduler falls back to sequential execution with a warning
- Dependencies that reference `TypeId`s not present in the task list (e.g.
  filtered out by `--skip`/`--only`) are silently ignored

### Parallel Resource Processing

Within each task, resource operations (symlinks, packages, registry entries,
etc.) are also processed in parallel using Rayon's `into_par_iter()`.

- `process_resources()` and `process_resource_states()` in `tasks/processing.rs` dispatch
  to Rayon's `into_par_iter()` when `ctx.parallel` is `true` and there is more than
  one resource to process
- A `Mutex<TaskStats>` accumulates changed/skipped counters across threads
- The `Executor` trait requires `Sync` so resources holding `&dyn Executor` are safe
  to share across threads
- The `Logger` uses `Mutex<Vec<TaskEntry>>` internally for thread-safe task recording

**To disable** both task-level and resource-level parallelism (e.g. for
debugging), pass `--no-parallel` directly to the binary — this flag is not
exposed by the wrapper scripts (see
[Advanced Binary Options](USAGE.md#advanced-binary-options)).

`process_resources_remove()` (used by uninstall tasks) is always sequential because
removal operations are rare and order may matter.

### Binary Distribution

- Pre-compiled binaries eliminate the need for a Rust toolchain on end-user machines
- The version cache (`bin/.dotfiles-version-cache`, 1 hour TTL) avoids GitHub API calls on every invocation
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

The pre-commit hook runs two checks via dedicated scripts in `hooks/`:

`check-sensitive.sh` scans staged files for sensitive data:
- API keys, tokens, passwords
- Private keys
- Cloud provider credentials
- Generic high-entropy secrets

`check-rust.sh` runs two checks when any `.rs` files are staged:
- `cargo fmt --check` — format verification
- `cargo clippy -- -D warnings` — lint enforcement (same policy as CI)

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
- User reviews `packages.toml` before installation

## See Also

- [Profile System](PROFILES.md) - Profile implementation details
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Contributing Guide](CONTRIBUTING.md) - Development guidelines
- [Testing Documentation](TESTING.md) - Testing procedures
