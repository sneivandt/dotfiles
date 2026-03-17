---
name: rust-patterns
description: >
  Rust implementation patterns for the dotfiles core engine: Task trait, Resource
  trait, Executor, Context, Platform, ProcessOpts, and documentation conventions.
  Use when creating or modifying Rust code in cli/src/.
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
├── logging/       # Logging subsystem
│   ├── mod.rs     # Re-exports, init_subscriber()
│   ├── logger.rs  # Logger (sequential output)
│   ├── buffered.rs # BufferedLog (parallel task output)
│   ├── diagnostic.rs # High-precision diagnostic event log
│   ├── types.rs   # Log trait, TaskEntry, TaskStatus
│   ├── subscriber.rs # DotfilesFormatter tracing subscriber
│   └── utils.rs   # ANSI stripping, formatting helpers
├── config/        # TOML config loading and deserialization
│   ├── mod.rs     # Config struct (aggregates all types)
│   ├── helpers/   # Shared loader utilities
│   │   ├── mod.rs       # Re-exports
│   │   ├── toml_loader.rs  # filter_by_categories(), TOML deserialization helpers
│   │   ├── category_matcher.rs  # Category matching logic
│   │   └── validation.rs   # Config validation helpers
│   ├── profiles.rs
│   └── *.rs       # Per-type loaders (packages, symlinks, etc.)
├── resources/     # Declarative resource abstraction
│   ├── mod.rs     # Applicable + Resource traits, ResourceState, ResourceChange
│   └── *.rs       # Per-type resources (symlink, registry, chmod, etc.)
├── engine/        # Generic resource processing engine
│   ├── mod.rs     # process_resources(), process_resource_states()
│   ├── mode.rs    # ProcessMode, ProcessOpts, ResourceAction
│   ├── stats.rs   # TaskResult, TaskStats
│   ├── apply.rs   # Apply/remove logic
│   ├── context.rs # Context, ContextOpts
│   ├── graph.rs   # Dependency graph cycle detection
│   ├── scheduler.rs # Parallel task scheduler (OS threads + mpsc)
│   ├── parallel.rs # Parallel execution helpers
│   ├── update_signal.rs # Arc<AtomicBool> signalling
│   └── tests/     # Engine unit tests
├── tasks/         # Task implementations
│   ├── mod.rs     # Task trait, TaskPhase, re-exports from engine/ and helpers/
│   ├── helpers/   # Macros (task_deps!, resource_task!, batch_resource_task!) and catalog
│   ├── system/ # System-phase tasks (self_update, update, sparse_checkout, hooks, etc.)
│   ├── user/ # User-phase tasks (packages, symlinks, chmod, git_config, etc.)
│   └── validation.rs  # Validation task (shared, not phase-specific)
└── commands/      # install.rs, uninstall.rs, test.rs
```

## The Task Trait

```rust
pub trait Task: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn phase(&self) -> TaskPhase;
    fn task_id(&self) -> TypeId { TypeId::of::<Self>() }
    fn dependencies(&self) -> &[TypeId] { &[] }
    fn should_run(&self, ctx: &Context) -> bool;
    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.run(ctx).map(Some)  // default: delegates to run()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
pub enum TaskPhase { System, User }
pub enum TaskResult { Ok, NotApplicable(String), Skipped(String), Failed(String), DryRun }
```

`run_if_applicable()` combines the `should_run` check and `run` call into a single step,
returning `Ok(None)` when the task is not applicable. The `resource_task!` and `batch_task!`
macros override the default to evaluate the config items exactly once, avoiding a second
config lock acquisition. The executor calls `run_if_applicable()` — never `run()` directly.

### Task Dependencies

Tasks declare dependencies via `dependencies()`, returning `TypeId`s of other
task structs. The scheduler runs tasks as soon as all their dependencies
complete — there are no fixed "levels" or ordering beyond the dependency
graph.

Use the `task_deps!` macro to implement `fn dependencies()` — it eliminates
the manual `const DEPS` boilerplate:

```rust
use crate::tasks::{Context, Task, TaskPhase, TaskResult, task_deps};

impl Task for InstallSymlinks {
    task_deps![crate::tasks::repository::update::UpdateRepository, crate::tasks::bootstrap::developer_mode::EnableDeveloperMode];
    // ...
}
```

Import `task_deps` from `crate::tasks::` alongside the other task helpers.  The macro
expands to the `fn dependencies(&self) -> &[std::any::TypeId]` method body, so
it must appear inside `impl Task for …`.

**Rules for dependencies:**
- Use `task_deps![…]` to declare dependencies — do not write `const DEPS` by hand
- Only reference concrete task structs
- Missing dependencies (filtered by `--skip`/`--only`) are silently ignored
- Cycles are detected at runtime; the scheduler bails with an error

### New Resource-Based Task Template (preferred)

Most tasks process a list of `Resource` implementors. Use the generic
`process_resources()` or `process_resource_states()` helpers instead of
writing the state-match loop by hand.

#### `resource_task!` macro (simplest — use for config→resource tasks)

For tasks that read config items, map each to a resource, and process them,
the `resource_task!` macro eliminates all boilerplate:

```rust
use crate::tasks::{ProcessOpts, TaskPhase, resource_task};
use crate::resources::my_resource::MyResource;

resource_task! {
    /// Install my resources from config.
    pub MyTask {
        name: "My task",
        phase: TaskPhase::Apply,
        deps: [crate::tasks::repository::some_dependency::SomeDependency],  // optional
        guard: |ctx| ctx.platform.supports_systemd(),     // optional
        items: |ctx| ctx.config_read().items.clone(),
        build: |item, ctx| MyResource::from_entry(&item, &ctx.home),
        opts: ProcessOpts::strict("install"),
    }
}
```

The macro generates a `Debug` struct and a full `Task` implementation:
- `should_run` returns `false` when the `guard` fails or `items` is empty
- `run` clones the config items, maps each to a resource via `build`, and
  delegates to `process_resources`

Import `resource_task` and `TaskPhase` from `crate::tasks::` alongside `ProcessOpts`. Tests that call
`should_run` or `run` must also import `crate::tasks::Task` to bring the
trait into scope.

See `tasks/apply/git_config.rs` (no deps, no guard) and `tasks/apply/chmod.rs` (deps + guard)
for real examples.

#### Manual `Task` impl (for complex or non-standard tasks)

When a task needs custom logic beyond the macro (e.g., batch-querying state,
conditional skipping, or non-resource work), write the impl manually:

```rust
use crate::tasks::{Context, ProcessOpts, Task, TaskPhase, TaskResult, process_resources, task_deps};
use crate::resources::my_resource::MyResource;

pub struct MyTask;
impl Task for MyTask {
    fn name(&self) -> &str { "My task" }
    fn phase(&self) -> TaskPhase { TaskPhase::Apply }
    task_deps![crate::tasks::repository::some_dependency::SomeDependency]; // omit if no dependencies
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_systemd() && !ctx.config_read().items.is_empty()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items = ctx.config_read().items.clone();
        let resources = items.iter()
            .map(|entry| MyResource::from_entry(entry, &*ctx.executor));
        process_resources(ctx, resources, &ProcessOpts::strict("install"))
    }
}
```

For tasks that batch-query state up front (packages, VS Code extensions,
registry), build `(Resource, ResourceState)` pairs and use
`process_resource_states()` instead.

Register in `tasks/helpers/catalog.rs` by adding `Box::new(crate::tasks::apply::my_module::MyTask)` to
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

### ProcessMode, ResourceAction, and ProcessOpts

`ProcessMode` is an enum that makes the intent of each processing strategy
explicit:

| Mode | `fix_incorrect()` | `fix_missing()` | `bail_on_error()` | Typical use |
|---|---|---|---|---|
| `Strict` | yes | yes | yes | symlinks, hooks, git config |
| `Lenient` | yes | yes | no | packages, registry, developer mode |
| `InstallMissing` | no | yes | no | VS Code extensions, systemd units |
| `FixExisting` | yes | no | yes | chmod (files may not exist yet) |

#### ResourceAction (lifecycle state machine)

`ProcessMode::action_for(&ResourceState)` returns a `ResourceAction` — the
explicit decision of what to do with a resource:

```rust
pub enum ResourceAction {
    Apply,          // Create or update the resource
    Noop,           // Already correct, nothing to do
    Skip(String),   // Skip with a reason (invalid, mode disallows, etc.)
}
```

The processing loop (`process_single`) matches on `ResourceAction` instead of
nesting matches on `ResourceState` and mode flags:

```rust
match opts.mode.action_for(&resource_state) {
    ResourceAction::Noop  => { /* already ok */ },
    ResourceAction::Skip(reason) => { /* log skip */ },
    ResourceAction::Apply => { /* apply change */ },
}
```

This makes the lifecycle state machine explicit and independently testable.

#### ProcessOpts

`ProcessOpts` pairs a `ProcessMode` with a human-readable `verb` for log messages.
Use named constructors:

```rust
ProcessOpts::strict("link")           // fix Missing+Incorrect, bail on errors
ProcessOpts::lenient("install")       // fix Missing+Incorrect, warn on errors
ProcessOpts::install_missing("enable") // only fix Missing, warn on errors
ProcessOpts::fix_existing("chmod")    // only fix Incorrect, bail on errors
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
    pub config: Arc<RwLock<Arc<Config>>>,    // RCU pattern; use ctx.config_read()
    pub platform: Platform,
    pub log: Arc<dyn Log>,
    pub dry_run: bool,
    pub home: PathBuf,
    pub executor: Arc<dyn Executor>,
    pub parallel: bool,
}
```

Helpers: `ctx.root()`, `ctx.symlinks_dir()`, `ctx.hooks_dir()`.

Builder methods for creating modified copies (used extensively in tests):
- `ctx.with_log(log)` — different logger (used by parallel scheduler)
- `ctx.with_dry_run(true)` — enable dry-run mode
- `ctx.with_parallel(true)` — enable parallel processing
- `ctx.with_home(path)` — override home directory
- `ctx.config_swap(new_config)` — atomically replace the shared config (used by `ReloadConfig`)

### ContextOpts

`Context::new()` takes a `ContextOpts` struct to avoid positional `bool` confusion:

```rust
pub struct ContextOpts {
    pub dry_run: bool,
    pub parallel: bool,
}
```

Config access uses an `RwLock<Arc<Config>>` (read-copy-update pattern) so the
`ReloadConfig` task can atomically swap the inner `Arc<Config>` after a git pull.
`ctx.config_read()` returns an `Arc<Config>` snapshot — the read lock is held
only for the duration of the `Arc::clone`, so callers can hold the snapshot as
long as needed without blocking writers. Clone data out before long-running
operations if you only need a subset:
`await` points or parallel sections:

```rust
let items = ctx.config_read().items.clone();
```

### Task-Specific Dependency Injection

Some tasks require dependencies that are not shared across all tasks and are
therefore injected via constructors rather than stored on `Context`:

- **`repo_updated: Arc<AtomicBool>`** — shared between `UpdateRepository` and
  `ReloadConfig`. `UpdateRepository` sets it to `true` when it pulls new
  commits; `ReloadConfig` reads it to decide whether a reload is needed. Both
  receive the same `Arc` from `all_install_tasks()`.

- **`fs_ops: Arc<dyn FileSystemOps>`** — held by `InstallGitHooks` and
  `UninstallGitHooks`. The production implementation is `SystemFileSystemOps`.
  In tests, use the `with_fs_ops` constructor to inject a mockall-generated
  `MockFileSystemOps` without touching the real filesystem:

```rust
let mut mock = MockFileSystemOps::new();
mock.expect_exists().returning(|_| true);
let task = InstallGitHooks::with_fs_ops(Arc::new(mock));
```

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
    fn which_path(&self, program: &str) -> Result<std::path::PathBuf>;
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

1. **Commands** create a `Context` via `CommandRunner::new()`, which detects the platform, resolves the profile, loads config, and builds the `Context` directly
2. **Context** stores `executor: Arc<dyn Executor>`
3. **Tasks** clone `ctx.executor` into resource constructors and pass `&*ctx.executor` to helper functions that only need a borrowed executor
4. **Resources** that shell out store `executor: Arc<dyn Executor>` and call `self.executor.run()` etc.

```rust
// In commands/install.rs
let runner = super::CommandRunner::new(global, log)?;
runner.run(tasks.iter().map(Box::as_ref))
```

`CommandRunner::new()` detects the platform, resolves the profile, loads config, and constructs the `Context` directly, then stores the resulting `Context` and `Arc<Logger>`. `CommandRunner::run()` delegates to `run_tasks_to_completion()`.

Resources that shell out own an `Arc<dyn Executor>`. Clone the context executor
when constructing them:

```rust
let resource = PackageResource::new(name, manager, Arc::clone(&ctx.executor));
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

Each `config/*.rs` module: `toml_loader::load_section_items(path, extract)` → `toml_loader::filter_by_categories(sections, categories)` → map items.

For simple flat lists, use the convenience wrapper `toml_loader::load_section::<S>(path, active_categories)` which combines all three steps via the `ConfigSection` trait.

## Error Handling

Use `anyhow::Result`, `?`, `.context("msg")?`, `bail!("msg")`. No `unwrap()` in non-test code.

## Resource Abstraction

The `resources/` module provides a two-level trait hierarchy for checking and applying system state:

```rust
/// Base trait — implement for resources whose state is determined by a bulk
/// external query (e.g. VS Code extensions, packages).
pub trait Applicable {
    fn description(&self) -> String;
    fn apply(&self) -> Result<ResourceChange>;
    fn remove(&self) -> Result<ResourceChange>; // default: unimplemented
}

/// Extended trait — implement for resources that can independently check
/// their own state (e.g. symlinks, registry entries, file permissions).
pub trait Resource: Applicable {
    fn current_state(&self) -> Result<ResourceState>;
    fn needs_change(&self) -> Result<bool>; // default: Missing|Incorrect → true
}

pub enum ResourceState { Missing, Correct, Incorrect { current: String }, Invalid { reason: String } }
pub enum ResourceChange { Applied, AlreadyCorrect, Skipped { reason: String } }
```

**Which trait to implement:**
- **`Resource`** (implies `Applicable`) — when the resource can check its own state individually (symlinks, chmod, registry, git config, hooks).
- **`Applicable` only** — when state is determined via a single external bulk query shared across all instances (VS Code extensions, packages). These use `process_resource_states()` with pre-computed `(impl Applicable, ResourceState)` pairs.

Tasks use `Resource` / `Applicable` implementors (e.g., `SymlinkResource`, `RegistryResource`) to check state and apply changes. New declarative resources go in `resources/*.rs`.

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

`engine/orchestrate.rs` provides helpers that handle the full check→dry-run→apply
loop so individual tasks don't repeat it (re-exported from `tasks/mod.rs`):

- **`process_resources(ctx, resources, opts)`** — calls `current_state()` per resource.
- **`process_resource_states(ctx, resource_states, opts)`** — takes pre-computed `(Resource, ResourceState)` pairs for batch-checked resources.
- **`process_resources_remove(ctx, resources, verb)`** — for uninstall: removes resources in `Correct` state, skips others.

Both `process_resources` and `process_resource_states` are implemented in
`engine/orchestrate.rs` and re-exported from `tasks/mod.rs`. They accept a `ProcessOpts` value
that controls which states are fixable and whether errors bail or warn.
Use these helpers for **all** new resource-based tasks.

**Parallel execution:** When `ctx.parallel` is `true` and there is more than one
resource, both helpers automatically dispatch to Rayon's parallel iterator.
Task-level parallelism uses OS threads (via `std::thread::scope`) so blocking
on `mpsc::Receiver::recv()` does not exhaust the Rayon thread pool. Resources
must implement `Send`; because `Executor: Sync`, all resources holding `&dyn Executor`
satisfy this automatically. Tests set `parallel: false` in their `Context` to keep
execution deterministic.

## Rules

- All task logic in `cli/src/tasks/*.rs` — never in shell scripts
- Every task: `name`, `should_run`, `run`; check `ctx.dry_run` before side effects
- **Use `task_deps![…]`** inside `impl Task` to declare dependencies — never write `const DEPS` by hand
- **Use `process_resources()` / `process_resource_states()`** for resource-based tasks — do not duplicate the state-match loop
- Use `Resource` trait for declarative state checks where applicable
- Guard tools with `executor.which()`; return `TaskResult::Skipped(reason)` when not applicable
- Pass `&*ctx.executor` (deref coercion) to resource constructors and batch query functions — never call `exec::*` free functions directly from tasks or resources
- Add `#[cfg(test)] mod tests` to every module; use `Platform::new()` in tests

## Documentation Conventions

All public items (modules, structs, enums, traits, functions) require `///` doc comments.

### Standard Sections (in order)

1. **Main description** (first, no header) — brief summary
2. **`# Examples`** — code examples (compiled as doctests unless annotated)
3. **`# Errors`** — **required** for all functions returning `Result<T>`
4. **`# Panics`** — document panic conditions
5. **`# Safety`** — required for `unsafe` functions

### Example Annotations

| Annotation | When to use |
|---|---|
| *(none)* | Rust code — compiled and tested as doctests |
| `` `ignore `` | Conceptual examples or pseudo-code |
| `` `ini `` / `` `bash `` / `` `text `` | Non-Rust code |

### `#[must_use]`

Apply on boolean queries (`is_*`, `has_*`, `supports_*`), constructors returning
`Self`, and pure functions:

```rust
#[must_use]
pub const fn supports_systemd(&self) -> bool {
    self.os == Os::Linux
}
```

### Structs, Enums, and Traits

Document all public fields, enum variants, and trait methods. Include `# Errors`
on every public function returning `Result<T>`.
