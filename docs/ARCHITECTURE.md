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
- GitHub Actions builds release binaries on every push to `main` that touches `cli/` or `conf/`
- The release workflow (`.github/workflows/release.yml`) publishes Linux and Windows binaries with SHA-256 checksums
- The shell wrappers download the latest release and cache the version for one hour (`bin/.dotfiles-version-cache`)
- A `--build` flag builds from source for development

## High-Level Architecture

```
┌──────────────┐      ┌─────────────┐
│ dotfiles.sh  │      │ dotfiles.ps1│   Thin wrappers
│  (Linux)     │      │  (Windows)  │   download/build binary
└──────┬───────┘      └──────┬──────┘
       │                     │
       ▼                     ▼
┌──────────────────────────────────────┐
│         cli/ (Rust binary)           │
│                                      │
│  cli.rs         — clap argument      │
│                   parsing            │
│  commands/      — install, uninstall,│
│                   test, version      │
│  config/        — TOML loading &     │
│                   profile resolution │
│  engine/        — execution engine,  │
│                   context, graph,    │
│                   scheduler          │
│  resources/     — idempotent check   │
│                   + apply primitives │
│  phases/        — Phase task impls   │
│  platform.rs    — OS detection       │
│  logging/       — structured logging │
│  exec.rs        — subprocess exec    │
└──────────────────────────────────────┘
       │
       ▼
┌────────────────────────────────────────────┐
│            conf/ (TOML files)              │
│  packages.toml      symlinks.toml          │
│  profiles.toml      manifest.toml          │
│  systemd-units.toml vscode-extensions.toml │
│  registry.toml      copilot-plugins.toml   │
│  chmod.toml         git-config.toml        │
└────────────────────────────────────────────┘
```

## Component Architecture

### Shell Wrappers

#### `dotfiles.sh` (Linux)

POSIX shell script that:
- Checks for a `--build` flag; if set, runs `cargo build --profile dev-opt` in `cli/` and executes the resulting binary
- Otherwise, bootstraps the latest published binary when `bin/dotfiles` is missing
- Verifies the downloaded bootstrap binary with the published SHA-256 checksum
- Exports `DOTFILES_ROOT` and forwards the remaining arguments directly to the binary

#### `dotfiles.ps1` (Windows)

PowerShell script with identical logic:
- `--build` flag builds from source with `cargo build --profile dev-opt`
- Otherwise bootstraps `dotfiles-windows-x86_64.exe` from GitHub Releases when missing
- Verifies checksum, promotes any staged self-update before launch, and exports `DOTFILES_ROOT`
- Forwards all other arguments directly to the binary

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

- **`install.rs`** — Uses `CommandRunner` to resolve the profile, load `Config`, build the task list, filter by `--skip`/`--only`, and execute each task via `phases::execute()`
- **`uninstall.rs`** — Removes managed symlinks
- **`test.rs`** — Runs configuration validation

#### Config (`config/`)

`Config::load()` reads all TOML files from `conf/` and filters sections against the active profile's categories:

| Module | File | Description |
| --- | --- | --- |
| `profiles.rs` | `profiles.toml` | Profile resolution and category computation |
| `helpers/toml_loader.rs` | (all) | Generic TOML loader |
| `packages.rs` | `packages.toml` | System packages (pacman, AUR, winget) |
| `symlinks.rs` | `symlinks.toml` | Symlink mappings |
| `systemd_units.rs` | `systemd-units.toml` | Systemd units (Linux only) |
| `chmod.rs` | `chmod.toml` | File permissions |
| `vscode_extensions.rs` | `vscode-extensions.toml` | VS Code extensions |
| `copilot_plugins.rs` | `copilot-plugins.toml` | GitHub Copilot CLI plugins from marketplaces |
| `registry.rs` | `registry.toml` | Windows registry entries |
| `git_config.rs` | `git-config.toml` | Git configuration settings |
| `manifest.rs` | `manifest.toml` | Sparse checkout file mappings |
| `overlay.rs` | — | Overlay path resolution and persistence |
| `scripts.rs` | `scripts.toml` | Custom script entries from overlay repo |

#### Tasks (`phases/`)

Each task implements the `Task` trait:

```rust
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Which phase this task belongs to (Bootstrap, Repository, or Apply).
    fn phase(&self) -> TaskPhase;

    /// Stable TypeId for dependency matching.
    fn task_id(&self) -> TypeId { TypeId::of::<Self>() }

    /// TypeIds of tasks that must complete before this one starts.
    fn dependencies(&self) -> &[TypeId] { &[] }

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Combine applicability checks with execution.
    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>>;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
```

A shared `Context` struct (defined in `engine/context.rs`) carries the loaded `Config`, `Platform`, `Logger`, and flags (`dry_run`, `parallel`, `home` path). Task-specific dependencies are injected via constructors: `UpdateRepository` and `ReloadConfig` share an `UpdateSignal` (`engine/update_signal.rs`) to coordinate config reloading, and hook tasks (`InstallGitHooks`, `UninstallGitHooks`) hold an `Arc<dyn FileSystemOps>` for testable filesystem access.

The `execute()` function first checks `should_run()`, then calls `run_if_applicable()`, recording `Ok`, `NotApplicable`, `Skipped`, `DryRun`, or `Failed` in the logger.

#### Engine (`engine/`)

The execution engine provides the generic resource processing loop, dependency graph, and shared context used by all tasks. Key components:

- **`context.rs`** — `Context` and `ContextOpts`: shared state (config, platform, logger, flags) threaded through every task
- **`apply.rs`** — single-resource processing: check state → dry-run → apply/remove
- **`orchestrate.rs`** — top-level resource orchestration with `process_resources()`, `process_resource_states()`, and `process_resources_remove()`
- **`mode.rs`** — `ProcessMode` enum (`Strict`, `Lenient`, `InstallMissing`, `FixExisting`) and `ProcessOpts` that control which states are fixable and whether errors bail or warn
- **`parallel.rs`** — Rayon-based parallel dispatch when `ctx.parallel` is true
- **`graph.rs`** — dependency graph cycle detection (Kahn's algorithm)
- **`scheduler.rs`** — dependency-driven parallel task scheduling using OS threads and `mpsc` channels
- **`stats.rs`** — `TaskResult` and `TaskStats` types
- **`update_signal.rs`** — `Arc<AtomicBool>` signalling between `UpdateRepository` and `ReloadConfig`

**Implemented tasks** (executed as soon as dependencies allow):

Bootstrap phase (`cli/src/phases/bootstrap/`):
- `self_update` — Update the dotfiles binary from latest GitHub release
- `developer_mode` — Enable Windows developer mode (required for symlinks)
- `wrapper` — Install platform-specific CLI wrapper to `~/.local/bin/` for running dotfiles from anywhere
- `path` — Ensure `~/.local/bin` is on the user's `PATH` (`~/.profile` on Unix, registry on Windows)

Repository phase (`cli/src/phases/repository/`):
- `update` — Update repository (`git pull --ff-only`)
- `sparse_checkout` — Configure git sparse checkout
- `reload_config` — Reload config from disk after `update` pulls new commits
- `hooks` — Install git hooks (copies `hooks/*` into `.git/hooks/`)
- `completions` — Generate the zsh completion script into `symlinks/config/zsh/completions/`
- `overlay_scripts` — Discover overlay script definitions and log script count

Apply phase (`cli/src/phases/apply/`):
- `packages` — Install system packages (pacman or winget)
- `paru` — Bootstrap paru AUR helper (Arch Linux only)
- `aur_packages` — Install AUR packages via paru (Arch Linux only)
- `symlinks` — Create symlinks
- `chmod` — Apply file permissions
- `git_config` — Configure git settings (Windows: autocrlf, symlinks, credential helper)
- `shell` — Configure default shell
- `systemd` — Enable systemd units
- `registry` — Apply Windows registry settings
- `vscode` — Install VS Code extensions
- `copilot_plugins` — Download Copilot CLI plugins
- `pam` — Install custom PAM service files (Arch Linux + desktop, uses sudo)
- `wsl_conf` — Write `/etc/wsl.conf` with `generateResolvConf = true` (Linux/WSL only, uses sudo)

#### Overlay System

An overlay repository provides private configuration extensions that are merged
with the main dotfiles config.  The overlay path is resolved from (in order):
`--overlay` CLI flag → `DOTFILES_OVERLAY` env var → `dotfiles.overlay` git
config.  When an overlay is set:

1. `Config::load()` reads any `conf/*.toml` files from the overlay directory
   and appends their entries to the main config lists
2. `scripts.toml` in the overlay defines custom script tasks
3. Each script entry produces a dynamic `OverlayScriptTask` that appears in
   the task output like any built-in task
4. Scripts follow a convention-based interface: no args (apply), `--check`
   (verify state, exit 0 = correct), `--remove` (undo)

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

#### Logging (`logging/`)

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
- Uses AND logic for exclusions — consistent with all other config files
- `[arch-desktop]` means "exclude only if both arch AND desktop are excluded"

### Error Handling

The binary uses `anyhow::Result` throughout. Each config loader and task adds context via `.context()`:

```rust
packages::load(&conf.join("packages.toml"), active_categories)
    .context("loading packages.toml")?;
```

Task failures are caught by `phases::execute()` and recorded as `TaskStatus::Failed` — the binary continues executing remaining tasks and reports all failures in the summary.

## Testing Architecture

### Rust Tests

- **Unit tests**: Inline `#[cfg(test)]` modules in source files (e.g. `platform.rs`, `cli.rs`, `config/toml_loader.rs`, `phases/bootstrap/*.rs`, `phases/repository/*.rs`, `phases/apply/*.rs`)
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
| --- | --- |
| `rust` | Formatting, Clippy linting, and unit/integration tests (matrix) |
| `lint` | ShellCheck and PSScriptAnalyzer (matrix) |
| `build` | Release build + smoke test on Linux and Windows (matrix) |
| `integration` | Dry-run install per profile and platform (matrix) |
| `test-applications` | Git, zsh, vim, nvim behavior (matrix) |
| `test-docker` | Docker image build + smoke test |
| `test-git-hooks` | Pre-commit sensitive data detection and Rust formatting/clippy linting |

### Release Pipeline

GitHub Actions release (`.github/workflows/release.yml`) triggers on push to `main` when `cli/` or `conf/` change:
1. Builds Linux and Windows release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with version tag `v0.1.<run_number>`

## Extension Points

### Adding New Tasks

1. Create a new file in `cli/src/phases/bootstrap/`, `cli/src/phases/repository/` (for bootstrap/repository-phase tasks), or `cli/src/phases/apply/` (for apply-phase tasks) implementing the `Task` trait
2. Add the module to `cli/src/phases/bootstrap/mod.rs`, `cli/src/phases/repository/mod.rs`, or `cli/src/phases/apply/mod.rs`
3. Add the task to `all_install_tasks()` in `cli/src/phases/catalog.rs`

### Adding New Configuration Types

1. Create TOML file in `conf/`
2. Add a config parser in `cli/src/config/`
3. Add the field to the `Config` struct and load it in `Config::load()`
4. Create a task in `cli/src/phases/apply/` that consumes the config
5. Document in CONFIGURATION.md

### Adding Overlay Scripts

1. Create a script in the overlay repository's `scripts/` directory
2. Implement the convention interface: no args (apply), `--check` (exit 0 if correct), `--remove` (undo)
3. Add the entry to `conf/scripts.toml` in the overlay repository
4. Use `--overlay /path/to/overlay` to activate

### Adding Custom Profiles

1. Define in `conf/profiles.toml`
2. Add sections to configuration files
3. Map files in `conf/manifest.toml`

## Performance Considerations

### Parallel Task Execution

Execution is split into three phases: **Bootstrap** (prepare the tool
itself), **Repository** (synchronise the dotfiles repository), then
**Apply** (apply declared state).  `run_tasks_to_completion()` loops
over `[TaskPhase::Bootstrap, TaskPhase::Repository, TaskPhase::Apply]`,
completing all tasks in one phase before starting the next.  Within each
phase, tasks are executed in parallel using a dependency-graph scheduler.

Each task declares its dependencies using the `task_deps!` macro (defined in
`phases/macros.rs`, re-exported from `phases/mod.rs`), which implements `Task::dependencies()` returning `TypeId`s of
prerequisite task structs.  The scheduler uses `std::thread::scope` to spawn
one OS thread per task and `mpsc` channels to block each task until its
dependencies complete.  For each task, a channel is created — dependent tasks
wait by calling `recv()` on the receiving end, and each dependency sends a
notification when it finishes.  OS threads are used deliberately — blocking on
`mpsc::Receiver::recv()` inside a Rayon worker would exhaust Rayon's
fixed-size thread pool and deadlock when the pool is smaller than the number
of tasks with unsatisfied dependencies (common on 2-vCPU CI runners).

**How it works:**

- Each task is spawned into an OS thread via `std::thread::scope`
- Tasks wait for their dependencies by calling `recv()` on an `mpsc::Receiver`,
  receiving one message per dependency
- When a task completes, it sends a notification to all tasks that depend on it
  via their `mpsc::Sender`s
- Tasks with no dependencies (or whose dependencies were filtered out) start
  immediately
- Each task's console output is captured in a per-task `BufferedLog`; when the
  task completes, the buffer is flushed atomically under a `flush_lock` so
  output from different tasks never interleaves
- A dim status line (`▹ task1, task2, ...`) shows which tasks are currently
  running, updated on every task start and completion
- Cycle detection (Kahn's algorithm) runs before scheduling; if a cycle is
  found, the run is aborted with an error
- Dependencies that reference `TypeId`s not present in the task list (e.g.
  filtered out by `--skip`/`--only`) are silently ignored

### Parallel Resource Processing

Within each task, resource operations (symlinks, packages, registry entries,
etc.) are also processed in parallel using Rayon's `into_par_iter()`.

- `process_resources()` and `process_resource_states()` in `engine/` dispatch
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

`process_resources_remove()` (used by uninstall tasks) also dispatches to parallel
processing when `ctx.parallel` is `true` and there is more than one resource,
matching the behaviour of `process_resources()` and `process_resource_states()`.

### Binary Distribution

- Pre-compiled binaries eliminate the need for a Rust toolchain on end-user machines
- The Rust binary owns the version cache (`bin/.dotfiles-version-cache`, 1 hour TTL) and self-update checks after bootstrap
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
