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
- GitHub Actions builds release binaries whenever a CI run on `main` completes successfully
- The release workflow (`.github/workflows/release.yml`) publishes Linux (x86_64, aarch64) and Windows binaries with SHA-256 checksums
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
│  commands/      — install, update,   │
│                   uninstall, test,   │
│                   version            │
│  config/        — TOML loading &     │
│                   profile resolution │
│  engine/        — execution engine,  │
│                   context, graph,    │
│                   scheduler          │
│  resources/     — idempotent check   │
│                   + apply primitives │
│  tasks/         — Task impls,        │
│                   grouped by domain  │
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
│  registry.toml      chmod.toml             │
│  git-config.toml    copilot.toml           │
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
  update      Install and advance pinned dependency versions
  uninstall   Remove installed dotfiles
  test        Run self-tests and validation
  version     Print version information

Install/update options:
  --skip TASK,...   Skip specific tasks
  --only TASK,...   Run only specific tasks
```

The wrapper scripts (`dotfiles.sh` / `dotfiles.ps1`) handle only the `--build` flag and then forward all remaining arguments to the binary unchanged. All binary flags — including `-p`, `-d`, `-v`, `--skip`, `--only`, `--root`, and `--no-parallel` — are available when invoking via the wrappers.

#### Commands (`commands/`)

- **`install.rs`** — Uses `CommandRunner` to resolve the profile, load `Config`, build the task list, filter by `--skip`/`--only`, and execute each task via `tasks::execute()`. Before the task graph, it may self-update the binary and re-exec so the rest of the run uses the latest engine. It also attempts safe fast-forward-only repository synchronization in the task graph but leaves pinned dependency versions untouched. Exposes `run_pipeline(advance_versions)`, the shared implementation behind both `install` and `update`
- **`update.rs`** — Delegates to `install::run_pipeline` with `advance_versions = true`, so it runs the identical task graph as `install` but additionally schedules the final Update phase to advance pinned dependency versions (currently the APM dependency refresh)
- **`uninstall.rs`** — Conservatively removes detachable managed state: symlinks, installed Git hooks, and the wrapper entry point. It intentionally does not remove packages or roll back registry, systemd, shell, editor, Copilot/APM, PAM/WSL, or overlay-script changes
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
| `registry.rs` | `registry.toml` | Windows registry entries |
| `git_config.rs` | `git-config.toml` | Git configuration settings |
| `copilot.rs` | `copilot.toml` | Copilot CLI settings (`~/.copilot/settings.json`) |
| `manifest.rs` | `manifest.toml` | Sparse checkout file mappings |
| `overlay.rs` | — | Overlay path resolution and persistence |
| `scripts.rs` | `scripts.toml` | Custom script entries from overlay repo |

#### Tasks (`tasks/`)

Each task implements the `Task` trait:

```rust
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Which phase this task belongs to (Bootstrap, Repository, Configure, or Update).
    ///
    /// This is per-task metadata returned by the task itself — it is **not**
    /// derived from the folder the task lives in. A task can therefore live in
    /// any module yet declare any phase, which is how a single domain (e.g.
    /// the overlay system) can span more than one phase.
    fn phase(&self) -> TaskPhase;

    /// Stable identifier for dependency matching.
    fn task_id(&self) -> TaskId { TaskId::Type(TypeId::of::<Self>()) }

    /// TaskIds of tasks that must complete before this one starts.
    fn dependencies(&self) -> &[TaskId] { &[] }

    /// Declarative rules enforced before the task runs.
    fn execution_policies(&self) -> &[ExecutionPolicy] { ALWAYS_POLICY }

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Combine applicability checks with execution.
    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>>;

    /// Predict whether an applicable task will need elevation.
    fn needs_elevation(&self, ctx: &Context) -> bool { false }

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
```

A shared `Context` struct (defined in `engine/context.rs`) carries the loaded `Config`, `Platform`, `Logger`, and flags (`dry_run`, `parallel`, `home` path). Task-specific dependencies are injected via constructors: `UpdateRepository` and `ReloadConfig` share an `UpdateSignal` (`engine/update_signal.rs`) to coordinate config reloading, and hook tasks (`InstallGitHooks`, `UninstallGitHooks`) hold an `Arc<dyn FileSystemOps>` for testable filesystem access.

The `execute()` function first evaluates `execution_policies()` (platform support, dry-run skip rules, and elevation declarations), then checks `should_run()` and calls `run_if_applicable()`, recording `Ok`, `NotApplicable`, `Skipped`, `DryRun`, or `Failed` in the logger. Before parallel phase dispatch, `run_tasks_to_completion()` calls `requires_elevation()` only for tasks whose policies and `should_run()` pass, then primes sudo for the tasks that predict a privileged mutation.

#### Engine (`engine/`)

The execution engine provides the generic resource processing loop, dependency graph, and shared context used by all tasks. Key components:

- **`context.rs`** — `Context` and `ContextOpts`: shared state (config, platform, logger, flags) threaded through every task
- **`plan.rs`** — pure resource plan/diff construction from `ResourceState` + `ProcessOpts`
- **`apply.rs`** — single-resource plan execution: log/dry-run → apply/remove → stats
- **`orchestrate.rs`** — top-level resource orchestration with `process_resources()`, `process_resources_with_provider()`, and `process_resources_remove()`
- **`mode.rs`** — `ProcessMode` enum (`Strict`, `Lenient`, `InstallMissing`, `FixExisting`) and `ProcessOpts` that control which states are fixable and whether errors bail or warn
- **`parallel.rs`** — Rayon-based parallel dispatch when `ctx.parallel` is true
- **`graph.rs`** — dependency graph cycle detection (Kahn's algorithm)
- **`scheduler.rs`** — dependency-driven parallel task scheduling using OS threads and `mpsc` channels
- **`stats.rs`** — `TaskResult` and `TaskStats` types
- **`update_signal.rs`** — `Arc<AtomicBool>` signalling between `UpdateRepository` and `ReloadConfig`

**Two axes: domain and phase.** Task files are organized by **domain** (what a
task is about) under `cli/src/tasks/`, while each task independently declares its
**phase** (when it runs) via `phase()`. The axes are orthogonal: a domain folder
can hold tasks from different phases, and a single domain can span phases. Domain
folders:

- `core/` — self-update, CLI wrapper install, `PATH` setup
- `repository/` — git pull, sparse checkout, config reload
- `git/` — git config, git hooks
- `files/` — symlinks, file permissions
- `shell/` — login shell, zsh completions
- `system/` — developer mode, systemd units, registry, PAM, wsl.conf
- `ai/` — APM plugin manifests, Copilot settings
- `editors/` — VS Code/editor extensions
- `packages/` — system and AUR packages
- `overlay/` — overlay script discovery and execution
- `validation/` — configuration checks

Every domain is a folder, so `tasks/` reads uniformly. A domain folder takes one
of two shapes: a thin `mod.rs` with per-task submodules (as in `system/` and
`git/`), or a production `mod.rs` paired with a sibling `tests.rs` when the code
is one cohesive unit but its tests are large (as in `editors/`, `overlay/`,
`packages/`, `validation/`, `ai/apm/`, and `repository/sparse_checkout/`). The
framework itself — the `Task` trait, `TaskPhase`, `Domain`, the
`resource_task!`/`task_deps!` macros, the task catalog, and the `--skip`/`--only`
filter — lives in `mod.rs`, `macros.rs`, `catalog.rs`, and `filter.rs`.

**Implemented tasks** (the engine schedules by **phase**, completing each before
the next; within a phase, tasks run as soon as dependencies allow). Each task is
annotated with its domain folder:

Bootstrap phase — prepares the tool itself, before the main task graph:
- `self_update` (core) — Updates the dotfiles binary from the latest GitHub release. Runs **before** the task graph (directly from `install.rs`) so all subsequent tasks use the latest code. If the binary is replaced, the process re-execs itself with a guard variable (`DOTFILES_REEXEC_GUARD`) to prevent an infinite loop.
- `developer_mode` (system) — Enable Windows developer mode (required for symlinks)
- `wrapper` (core) — Install platform-specific CLI wrapper to `~/.local/bin/` for running dotfiles from anywhere
- `path` (core) — Ensure `~/.local/bin` is on the user's `PATH` (`~/.profile` on Unix, registry on Windows)

Sync phase — synchronize the dotfiles repository:
- `update` (repository) — Update repository (`git pull --ff-only`)
- `sparse_checkout` (repository) — Configure git sparse checkout
- `reload_config` (repository) — Reload config from disk after `update` pulls new commits
- `hooks` (git) — Install git hooks (copies `hooks/*` into `.git/hooks/`)
- `completions` (shell) — Generate the zsh completion script into `symlinks/config/zsh/completions/`
- `overlay_scripts` (overlay) — Discover overlay script definitions and log script count. The overlay *domain* spans two phases: this discovery task runs in the Sync phase, while the generated `OverlayScriptTask`s run in the Provision phase. Both live in `tasks/overlay/mod.rs` because phase is per-task metadata (see `phase()` above), not folder-derived.

Provision phase — converge declared configuration to its target state:
- `packages` (packages) — Install system packages (pacman or winget)
- `paru` (packages) — Bootstrap paru AUR helper (Arch Linux only)
- `aur_packages` (packages) — Install AUR packages via paru (Arch Linux only)
- `symlinks` (files) — Create symlinks
- `chmod` (files) — Configure file permissions
- `git_config` (git) — Configure git settings (Windows: autocrlf, symlinks, credential helper)
- `shell` (shell) — Configure default shell
- `systemd` (system) — Enable systemd units
- `registry` (system) — Apply Windows registry settings
- `vscode` (editors) — Install VS Code extensions
- `apm` (ai) — `InstallApmPackages`: converge AI plugin manifests via Microsoft APM (reads `~/.apm/apm.yml` generated by merging every `~/.apm/config/*.yml` fragment; runs `apm install -g --target copilot,codex` and adds `copilot-app` only when `~/.copilot/data.db` exists). Convergence only — it never advances locked refs
- `pam` (system) — Install custom PAM service files (Arch Linux + desktop, uses sudo)
- `wsl_conf` (system) — Write `/etc/wsl.conf` with `generateResolvConf = true` (Linux/WSL only, uses sudo)

Update phase — advance pinned/locked dependency versions (the `update` command only; absent from `install`):
- `apm` (ai) — `UpdateApmPackages`: runs `apm outdated -g` and, when stale, `apm update -g --yes` to advance locked dependency refs. Self-guards on the install success marker so it only advances after a successful convergence

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
   (exit 0 = correct, exit 1 = apply needed), `--dryrun` (preview), and
   `--remove` (undo)

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
event immediately with sequence numbers, microsecond-resolution wall-clock
timestamps, and task context, providing the true chronological view of parallel execution.
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

Task failures are caught by `tasks::execute()` and recorded as `TaskStatus::Failed` — the binary continues executing remaining tasks and reports all failures in the summary.

## Testing Architecture

### Rust Tests

- **Unit tests**: Inline `#[cfg(test)]` modules in source files (e.g. `platform.rs`, `cli.rs`, `config/toml_loader.rs`, `tasks/core/*.rs`, `tasks/repository/*.rs`, `tasks/files/*.rs`)
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
| `rust-fmt` | Rust format check (`cargo fmt --check`) |
| `lint` | ShellCheck and PSScriptAnalyzer (matrix: ShellCheck, PSScriptAnalyzer) |
| `validate-config` | 6 config checks: TOML syntax, file references, category consistency, empty sections |
| `audit` | Cargo security audit (vulnerability scan) |
| `deny` | Cargo deny: license and advisory policy |
| `build-linux` | Linux build + Clippy + unit/integration tests |
| `build-windows` | Windows build + Clippy + unit/integration tests |
| `integration-linux` | Dry-run install and validation per profile on Linux (matrix: base, desktop) |
| `integration-windows` | Dry-run install and validation per profile on Windows (matrix: base, desktop) |
| `test-install-uninstall` | Install/uninstall round-trip (Linux) |
| `test-install-uninstall-windows` | Install/uninstall round-trip (Windows) |
| `test-applications` | Git, zsh, vim, nvim behavior (matrix) |
| `test-git-hooks` | Pre-commit sensitive data detection |
| `test-shell-wrapper-linux` | Linux wrapper script (`dotfiles.sh`) validation |
| `test-shell-wrapper-windows` | Windows wrapper script (`dotfiles.ps1`) validation |

### Release Pipeline

GitHub Actions release (`.github/workflows/release.yml`) triggers automatically when the CI workflow completes successfully on `main`:
1. Builds Linux (x86_64, aarch64) and Windows (x86_64) release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with version tag `v0.1.<run_number>`

## Extension Points

### Adding New Tasks

1. Create a new file in the relevant domain folder under `cli/src/tasks/<domain>/` (e.g. `core/`, `repository/`, `git/`, `files/`, `shell/`, `system/`, `ai/`), implementing the `Task` trait and declaring its `phase()` (Bootstrap, Repository, Configure, or Update)
2. Add the module to that domain's `cli/src/tasks/<domain>/mod.rs`
3. Add the task to `all_install_tasks()` in `cli/src/tasks/catalog.rs`

### Adding New Configuration Types

1. Create TOML file in `conf/`
2. Add a config parser in `cli/src/config/`
3. Add the field to the `Config` struct and a single `SectionLoader` call in
   `Config::load()` (e.g. `sections.collect_filtered(...)`). The same call
   loads the main config and merges the overlay, so there is no separate
   overlay-merge step to keep in sync.
4. Create a task in the relevant domain folder under `cli/src/tasks/` (declaring the Provision phase) that consumes the config
5. Document in CONFIGURATION.md

### Adding Overlay Scripts

1. Create a script in the overlay repository's `scripts/` directory
2. Implement the convention interface: no args (apply), `--check` (exit 0 if correct, exit 1 if apply is needed), `--dryrun` (preview), `--remove` (undo)
3. Add the entry to `conf/scripts.toml` in the overlay repository
4. Use `--overlay /path/to/overlay` to activate

### Adding Custom Profiles

1. Define in `conf/profiles.toml`
2. Add sections to configuration files
3. Map files in `conf/manifest.toml`

## Performance Considerations

### Parallel Task Execution

Execution is split into four phases: **Bootstrap** (prepare the tool
itself), **Repository** (synchronise the dotfiles repository),
**Configure** (apply declared state), then **Update** (advance pinned/locked
dependency versions — `update` command only).  `run_tasks_to_completion()`
loops over
`[TaskPhase::Bootstrap, TaskPhase::Sync, TaskPhase::Provision, TaskPhase::Update]`,
completing all tasks in one phase before starting the next (an empty phase,
such as Update under `install`, is skipped with no header).  Within each
phase, tasks are executed in parallel using a dependency-graph scheduler.

Each task declares its dependencies using the `task_deps!` macro (defined in
`tasks/macros.rs`, re-exported from `tasks/mod.rs`), which implements `Task::dependencies()` returning `TypeId`s of
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

- `process_resources()` and `process_resources_with_provider()` in `engine/`
  dispatch to Rayon's `into_par_iter()` when `ctx.parallel` is `true` and there
  is more than one resource to process
- A `Mutex<TaskStats>` accumulates changed/skipped counters across threads
- The `Executor` trait requires `Sync` so resources holding `&dyn Executor` are safe
  to share across threads
- The `Logger` uses `Mutex<Vec<TaskEntry>>` internally for thread-safe task recording

**To disable** both task-level and resource-level parallelism (e.g. for
debugging), pass `--no-parallel` to the wrapper scripts or the binary directly (see
[Advanced Binary Options](USAGE.md#advanced-binary-options)).

`process_resources_remove()` (used by uninstall tasks) also dispatches to parallel
processing when `ctx.parallel` is `true` and there is more than one resource,
matching the behaviour of `process_resources()` and `process_resources_with_provider()`.

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

The pre-commit hook runs targeted checks via dedicated scripts in `hooks/`:

`check-sensitive.sh` scans staged files for sensitive data:
- API keys, tokens, passwords
- Private keys
- Cloud provider credentials
- Generic high-entropy secrets

`check-rust.sh` runs Rust and PowerShell checks for staged files:
- `cargo fmt --check` — format verification
- `cargo clippy --profile ci -- -D warnings` — lint enforcement (same policy as CI)
- PSScriptAnalyzer for staged PowerShell files when available

`DOTFILES_HOOKS_FULL=1` enables slower CI-parity checks: Windows-target clippy,
`cargo test --profile ci`, config drift tests, cargo-deny, and Linux shell
wrapper argument-forwarding tests. `check-ci-guards.sh` keeps default
pre-commit checks fast by running cheap guards first: config reference checks,
wildcard dependency detection, and ShellCheck on staged shell files.

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
