---
name: rust-patterns
description: >
  Rust implementation patterns for the dotfiles core engine: Task trait, Resource
  trait, Executor, Context, Platform, and ProcessOpts. Use when creating or modifying
  Rust code in cli/src/. For doc comment conventions, see the rust-docs skill.
metadata:
  author: sneivandt
  version: "2.0"
---

# Rust Patterns

The core engine is a single Rust crate in `cli/` using `anyhow` for errors and `clap` (derive) for CLI.

## Project Layout

```
cli/src/
├── main.rs        # Entry: parse CLI, dispatch commands
├── cli.rs         # clap CLI definitions (Cli, Command, GlobalOpts)
├── platform.rs    # OS detection (Platform, Os enum)
├── exec.rs        # Command execution helpers
├── logging.rs     # Logger with leveled output + task summary
├── config/        # INI parsing and config loading
│   ├── mod.rs     # Config struct (aggregates all types)
│   ├── ini.rs     # Section/KvSection parsers, filter_sections()
│   ├── profiles.rs
│   └── *.rs       # Per-type loaders (packages, symlinks, etc.)
├── resources/     # Declarative resource abstraction
│   ├── mod.rs     # Resource trait, ResourceState, ResourceChange
│   └── *.rs       # Per-type resources (symlink, registry, chmod, etc.)
├── tasks/         # Task implementations
│   ├── mod.rs     # Task trait, Context, ProcessOpts, process_resources()
│   └── *.rs       # One file per task
└── commands/      # install.rs, uninstall.rs, test.rs
```

## The Task Trait

```rust
pub trait Task: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn task_id(&self) -> TypeId { TypeId::of::<Self>() }
    fn dependencies(&self) -> &[TypeId] { &[] }
    fn should_run(&self, ctx: &Context) -> bool;
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
pub enum TaskResult { Ok, Skipped(String), DryRun }
```

### Task Dependencies

Tasks declare dependencies via `dependencies()`, returning `TypeId`s of other
task structs. The scheduler runs tasks as soon as all their dependencies
complete — there are no fixed "levels" or ordering beyond the dependency
graph.

```rust
use std::any::TypeId;

impl Task for InstallSymlinks {
    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[
            TypeId::of::<UpdateRepository>(),
            TypeId::of::<EnableDeveloperMode>(),
        ];
        DEPS
    }
    // ...
}
```

**Rules for dependencies:**
- Use `const DEPS` for the slice (required for `TypeId::of` in const context)
- Only reference concrete task structs (`TypeId::of::<TaskStruct>()`)
- Missing dependencies (filtered by `--skip`/`--only`) are silently ignored
- Cycles are detected at runtime; the scheduler falls back to sequential execution

### New Resource-Based Task Template (preferred)

Most tasks process a list of `Resource` implementors. Use the generic
`process_resources()` or `process_resource_states()` helpers instead of
writing the state-match loop by hand:

```rust
use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::my_resource::MyResource;

pub struct MyTask;
impl Task for MyTask {
    fn name(&self) -> &str { "My task" }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_systemd() && !ctx.config_read().items.is_empty()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items = ctx.config_read().items.clone();
        let resources = items.iter()
            .map(|entry| MyResource::from_entry(entry, &*ctx.executor));
        process_resources(ctx, resources, &ProcessOpts::apply_all("install"))
    }
}
```

For tasks that batch-query state up front (packages, VS Code extensions,
registry), build `(Resource, ResourceState)` pairs and use
`process_resource_states()` instead.

Register in `tasks/mod.rs` by adding `Box::new(tasks::my_module::MyTask)` to
`all_install_tasks()`.

For **uninstall** tasks, use `process_resources_remove()`:

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    let items = ctx.config_read().items.clone();
    let resources = items.iter()
        .map(|entry| MyResource::from_entry(entry, &*ctx.executor));
    process_resources_remove(ctx, resources, "remove")
}
```

### ProcessOpts Fields

| Field | Purpose |
|---|---|
| `verb` | Verb for log messages ("install", "link", "chmod") |
| `fix_incorrect` | Apply when state is `Incorrect` (else skip) |
| `fix_missing` | Apply when state is `Missing` (else skip) |
| `bail_on_error` | `true`: propagate `apply()` errors. `false`: warn and count as skipped |

Use named constructors and modifier methods instead of building the struct directly:

```rust
ProcessOpts::apply_all("link")            // fix Missing+Incorrect, bail on errors (strict)
ProcessOpts::apply_all("install").no_bail() // fix Missing+Incorrect, warn on errors (lenient)
ProcessOpts::install_missing("enable")    // only fix Missing, warn on errors
ProcessOpts::apply_all("chmod").skip_missing() // only fix Incorrect, bail on errors
```

### Non-Resource Task Template

For tasks that don't use the `Resource` trait (e.g., git config, shell
setup), write the check→dry-run→mutate loop manually:

```rust
pub struct MyCustomTask;
impl Task for MyCustomTask {
    fn name(&self) -> &str { "My custom task" }
    fn should_run(&self, ctx: &Context) -> bool { true }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if already_correct() {
            return Ok(TaskResult::Ok);
        }
        if ctx.dry_run {
            ctx.log.dry_run("would do something");
            return Ok(TaskResult::DryRun);
        }
        do_something()?;
        Ok(TaskResult::Ok)
    }
}
```

## Platform Detection

Use capability-based methods when possible for more expressive code:

**Capability Methods** (preferred):
- `platform.supports_chmod()` — POSIX file permissions support
- `platform.supports_systemd()` — systemd support
- `platform.has_registry()` — Windows Registry support
- `platform.supports_aur()` — AUR package support
- `platform.uses_pacman()` — pacman package manager
- `platform.excludes_category(cat)` — whether a category should be excluded on this platform

**Direct OS Checks** (when capabilities don't apply):
- `platform.is_linux()` — Linux OS
- `platform.is_windows()` — Windows OS
- `platform.is_arch_linux()` — Arch Linux specifically

**Why use capability methods?** They make the *reason* for the platform check explicit:

```rust
// Less expressive - why does Linux matter?
ctx.platform.is_linux() && !ctx.config_read().units.is_empty()

// More expressive - clearly about systemd support
ctx.platform.supports_systemd() && !ctx.config_read().units.is_empty()
```

## Context Struct

```rust
pub struct Context {
    pub config: Arc<RwLock<Config>>,         // locked; use ctx.config_read()
    pub platform: Arc<Platform>,
    pub log: Arc<dyn Log>,
    pub dry_run: bool,
    pub home: PathBuf,
    pub executor: Arc<dyn Executor>,
    pub parallel: bool,
    pub repo_updated: Arc<AtomicBool>,       // set by UpdateRepository, read by ReloadConfig
    pub fs_ops: Arc<dyn FileSystemOps>,      // filesystem abstraction; inject MockFileSystemOps in tests
}
```

Helpers: `ctx.root()`, `ctx.symlinks_dir()`, `ctx.hooks_dir()`.

Config access uses an `RwLock` so the `ReloadConfig` task can atomically swap
the config after a git pull. Use `ctx.config_read()` to get a read guard; clone
the data out before a long-running operation so the lock is not held across
`await` points or parallel sections:

```rust
let items = ctx.config_read().items.clone();
```

`repo_updated` is an `Arc<AtomicBool>` shared across per-task contexts in the
parallel scheduler. `UpdateRepository` sets it to `true` when it actually pulls
new commits; `ReloadConfig` reads it to decide whether a reload is needed.

`fs_ops` is an `Arc<dyn FileSystemOps>` that wraps filesystem operations
(`exists`, `is_file`, `read_dir`). The production implementation is
`SystemFileSystemOps`. In tests, inject `MockFileSystemOps` via
`ctx.with_fs_ops(Arc::new(mock))` so tasks can be exercised without touching
the real filesystem.

## Executor Trait

All command execution goes through the `Executor` trait (`exec.rs`), which enables
dependency injection and test mocking:

```rust
pub trait Executor: std::fmt::Debug + Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult>;
    fn run_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult>;
    fn run_in_with_env(&self, dir: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<ExecResult>;
    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult>;
    fn which(&self, program: &str) -> bool;
}
```

The `Send + Sync` supertraits are required because the executor is shared via
`Arc<dyn Executor>` across tasks and resources that may run in parallel;
`Arc<T>: Send + Sync` requires `T: Send + Sync`.

`SystemExecutor` is the production implementation that delegates to real
process spawning. Free functions (`exec::run()`, `exec::run_unchecked()`, etc.)
still exist but are only called by `SystemExecutor` internally.

### Passing the Executor

The executor flows top-down through the system:

1. **Commands** create a `Context` via `CommandSetup::init()` + `setup.into_context()`
2. **Context** stores `executor: Arc<dyn Executor>`
3. **Tasks** pass `&*ctx.executor` to resource constructors and batch query functions
4. **Resources** store `executor: &'a dyn Executor` and call `self.executor.run()` etc.

```rust
// In commands/install.rs
let setup = super::CommandSetup::init(global, &**log)?;
let ctx = setup.into_context(global, log)?;
```

`CommandSetup::into_context()` creates the executor internally and constructs the `Context`.

Resources borrow the executor for the duration of the task. Pass `&*ctx.executor`
(deref coercion) when constructing resources:

```rust
let resource = PackageResource::new(name, manager, &*ctx.executor);
```

Some free-standing query functions also take the executor:
```rust
let installed = get_installed_packages(manager, &*ctx.executor)?;
let extensions = get_installed_extensions(&cmd, &*ctx.executor)?;
```

Others use native crates and need no executor:
```rust
let cached = batch_check_values(&resources)?;
```

## Config Loader Pattern

Each `config/*.rs` module: `ini::parse_sections(path)` → `ini::filter_sections(sections, categories, MatchMode::All)` → parse items.

For simple flat lists, use the convenience wrapper `ini::load_flat_items(path, active_categories)` which combines all three steps.

## Error Handling

Use `anyhow::Result`, `?`, `.context("msg")?`, `bail!("msg")`. No `unwrap()` in non-test code.

## Resource Abstraction

The `resources/` module provides a declarative layer for checking and applying system state:

```rust
pub trait Resource {
    fn description(&self) -> String;
    fn current_state(&self) -> Result<ResourceState>;
    fn needs_change(&self) -> Result<bool>;
    fn apply(&self) -> Result<ResourceChange>;
    fn remove(&self) -> Result<ResourceChange>; // default: unimplemented
}
pub enum ResourceState { Missing, Correct, Incorrect { current: String }, Invalid { reason: String } }
pub enum ResourceChange { Applied, AlreadyCorrect, Skipped { reason: String } }
```

Tasks use `Resource` implementors (e.g., `SymlinkResource`, `RegistryResource`) to check state and apply changes. New declarative resources go in `resources/*.rs`.

### Resource Struct Pattern

Resources that shell out take a borrowed executor, giving them a lifetime parameter.
The `Executor: Debug` supertrait allows `#[derive(Debug)]` on all resources.
Resources are not `Clone` (trait objects are not cloneable).

```rust
#[derive(Debug)]
pub struct MyResource<'a> {
    pub name: String,
    executor: &'a dyn Executor,
}

impl<'a> MyResource<'a> {
    #[must_use]
    pub const fn new(name: String, executor: &'a dyn Executor) -> Self {
        Self { name, executor }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(entry: &config::MyEntry, executor: &'a dyn Executor) -> Self {
        Self::new(entry.name.clone(), executor)
    }
}

impl Resource for MyResource<'_> {
    // ...
    fn current_state(&self) -> Result<ResourceState> {
        let result = self.executor.run_unchecked("tool", &["check", &self.name])?;
        // ...
    }
}
```

Resources that use native crates or only filesystem operations (e.g., `SymlinkResource`,
`ChmodResource`, `GitConfigResource`, `RegistryResource`) do not need an executor
and have no lifetime parameter.

### Generic Resource Loop

`tasks/mod.rs` provides two helpers that handle the full check→dry-run→apply
loop so individual tasks don't repeat it (implemented in `tasks/processing.rs`,
re-exported from `tasks/mod.rs`):

- **`process_resources(ctx, resources, opts)`** — calls `current_state()` per resource.
- **`process_resource_states(ctx, resource_states, opts)`** — takes pre-computed `(Resource, ResourceState)` pairs for batch-checked resources.
- **`process_resources_remove(ctx, resources, verb)`** — for uninstall: removes resources in `Correct` state, skips others.

Both `process_resources` and `process_resource_states` are implemented in `tasks/processing.rs`
and re-exported from `tasks/mod.rs`. They accept a `ProcessOpts` value
that controls which states are fixable and whether errors bail or warn.
Use these helpers for **all** new resource-based tasks.

**Parallel execution:** When `ctx.parallel` is `true` and there is more than one
resource, both helpers automatically dispatch to Rayon's parallel iterator.
Task-level parallelism uses OS threads (via `std::thread::scope`) so blocking
on `Condvar` does not exhaust the Rayon thread pool. Resources
must implement `Send`; because `Executor: Sync`, all resources holding `&dyn Executor`
satisfy this automatically. Tests set `parallel: false` in their `Context` to keep
execution deterministic.

## Rules

- All task logic in `cli/src/tasks/*.rs` — never in shell scripts
- Every task: `name`, `should_run`, `run`; check `ctx.dry_run` before side effects
- **Use `process_resources()` / `process_resource_states()`** for resource-based tasks — do not duplicate the state-match loop
- Use `Resource` trait for declarative state checks where applicable
- Guard tools with `executor.which()`; return `TaskResult::Skipped(reason)` when not applicable
- Pass `&*ctx.executor` (deref coercion) to resource constructors and batch query functions — never call `exec::*` free functions directly from tasks or resources
- Add `#[cfg(test)] mod tests` to every module; use `Platform::new()` in tests
- See the **rust-docs** skill for documentation conventions (`///`, `# Errors`, `#[must_use]`)
