---
name: resource-implementation
description: >
  Patterns for implementing concrete Resource, IntrinsicState, and
  ResourceStateProvider types in cli/src/resources/. Use when adding a new
  resource or modifying existing resource behaviour.
---

# Resource Implementation

Resources in `cli/src/resources/` are the declarative primitives that apply
system state. State discovery is intentionally separate: resources either
implement `IntrinsicState` for per-resource checks or use a
`ResourceStateProvider` for cached/bulk checks.

## Trait Definitions

```rust
/// Core trait — implement for every resource.
pub trait Resource {
    fn description(&self) -> String;
    fn apply(&self) -> ResourceResult<ResourceChange>;
    fn remove(&self) -> ResourceResult<ResourceChange>; // default: unimplemented
}

/// Extension trait — implement for resources that can independently check
/// their own state (e.g. symlinks, file permissions).
pub trait IntrinsicState: Resource {
    fn current_state(&self) -> Result<ResourceState>;
    fn needs_change(&self) -> Result<bool>; // default: Missing|Incorrect → true
}

/// Provider trait — implement or compose for cached/bulk state checks.
pub trait ResourceStateProvider<R: Resource> {
    type Cache: Sync;
    fn load(&self, resources: &[R]) -> Result<Self::Cache>;
    fn current_state(&self, resource: &R, cache: &Self::Cache) -> Result<ResourceState>;
}

pub enum ResourceState {
    Missing,
    Correct,
    Incorrect { current: String },
    Invalid { reason: String },
    Unknown { reason: String },
}
pub enum ResourceChange {
    Applied,
    AlreadyCorrect,
    Skipped { reason: String },
}
```

`apply`/`remove` return `ResourceResult<T>` (alias for `Result<T, ResourceError>`),
so failures are typed and the engine classifies them via `ResourceError::category()`
without downcasting. `current_state` (on `IntrinsicState`/`ResourceStateProvider`)
still returns `anyhow::Result`. Inside `apply`/`remove`, `?` auto-converts
`std::io::Error` (→ `ResourceError::Io`) and `anyhow::Error` from internal helpers
(→ `ResourceError::Other`); return a concrete variant directly for classifiable
resource-level failures. See the `error-handling-patterns` skill.

## Which Trait to Implement

| Trait / provider | When | Examples |
|---|---|---|
| `Resource` + `IntrinsicState` | Resource can independently check its own state | `SymlinkResource`, `ChmodResource`, `GitConfigResource`, `HookFileResource`, `WrapperResource`, `PathEntryResource`, `ScriptResource` |
| `Resource` + `ResourceStateProvider` | State requires one shared cached/bulk query | `VsCodeExtensionResource`, `PackageResource`, `RegistryResource` |

## Self-Checking Resource Template

Resources that use only filesystem or native crate operations — no executor needed:

```rust
#[derive(Debug, Clone)]
pub struct MyResource {
    pub target: PathBuf,
    pub desired: String,
}

impl MyResource {
    #[must_use]
    pub const fn new(target: PathBuf, desired: String) -> Self {
        Self { target, desired }
    }

    #[must_use]
    pub fn from_entry(entry: &config::MyEntry, home: &Path) -> Self {
        Self::new(home.join(&entry.path), entry.value.clone())
    }
}

impl Resource for MyResource {
    fn description(&self) -> String { format!("{}", self.target.display()) }
    fn apply(&self) -> ResourceResult<ResourceChange> { /* create/update */ }
    fn remove(&self) -> ResourceResult<ResourceChange> { /* undo */ }
}

impl IntrinsicState for MyResource {
    fn current_state(&self) -> Result<ResourceState> { /* check */ }
}
```

Real examples: `ChmodResource`, `GitConfigResource`.

## Executor-Dependent Resource Template

Resources that shell out take a borrowed executor with a lifetime parameter:

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
}

impl Resource for MyResource<'_> {
    fn description(&self) -> String { self.name.clone() }
    fn apply(&self) -> ResourceResult<ResourceChange> { /* create/update */ }
}

impl IntrinsicState for MyResource<'_> {
    fn current_state(&self) -> Result<ResourceState> {
        let result = self.executor.run_unchecked("tool", &["check", &self.name])?;
        if result.success { Ok(ResourceState::Correct) } else { Ok(ResourceState::Missing) }
    }
}
```

The `Executor: Debug` supertrait allows `#[derive(Debug)]`. Resources are not `Clone`
when they hold trait object references.

## Bulk-Checked Provider Template

For resources whose state comes from a single expensive query:

```rust
#[derive(Debug)]
pub struct MyResource {
    pub id: String,
    executor: Arc<dyn Executor>,
}

impl MyResource {
    #[must_use]
    pub fn state_from_installed(&self, installed: &HashSet<String>) -> ResourceState {
        if installed.contains(&self.id) { ResourceState::Correct } else { ResourceState::Missing }
    }
}

impl Resource for MyResource {
    fn description(&self) -> String { self.id.clone() }
    fn apply(&self) -> ResourceResult<ResourceChange> { /* install */ }
}

// Provide a standalone query function:
pub fn get_installed(executor: &dyn Executor) -> Result<HashSet<String>> { /* single command */ }
```

The task calls `get_installed()` once, then uses
`process_resources_with_provider()` with `PreloadedStateProvider` or
`BorrowedStateProvider` so all resources share the same cache:

```rust
let installed = get_installed(&*ctx.executor)?;
let resources = items.iter().map(|item| MyResource::from_entry(item, Arc::clone(&ctx.executor)));
let provider = BorrowedStateProvider::new(&installed, |resource: &MyResource, installed| {
    Ok(resource.state_from_installed(installed))
});
process_resources_with_provider(ctx, resources, &provider, &ProcessOpts::lenient("install"))
```

Real examples: `VsCodeExtensionResource`, `PackageResource`.

## Adding a New Resource Type

When asked to add a new resource type, wire the whole vertical slice rather than
only the resource struct:

1. Create `cli/src/resources/<resource>.rs`; implement `Resource`, plus
   `IntrinsicState` when the resource can check itself or a
   `ResourceStateProvider` when state should be bulk/cached.
2. Create `cli/src/config/<resource>.rs`; use `config_section!`, support
   category-based filtering, and add a config-loading test.
3. Add `conf/<resource>.toml` with the appropriate section and item format.
4. Create the task in the appropriate `cli/src/tasks/<domain>/` folder; prefer
   `resource_task!`, declare dependencies, and register it in
   `cli/src/tasks/catalog.rs`.
5. Add module declarations in `cli/src/resources/mod.rs`,
   `cli/src/config/mod.rs`, and the relevant `cli/src/tasks/**/mod.rs`.
6. Validate with the Rust checks from `cross-platform-verification`.

Read `rust-patterns`, `toml-configuration`, and `engine-orchestration` before
starting if the resource touches config loading or task dependencies.

## ResourceState Usage

| Variant | Meaning | Typical check |
|---|---|---|
| `Missing` | Does not exist | Path/entry not found |
| `Correct` | Matches desired state | Symlink points correctly, value matches |
| `Incorrect { current }` | Exists but wrong | Symlink wrong target, wrong value |
| `Invalid { reason }` | Cannot be applied | Source missing, target is real directory |
| `Unknown { reason }` | State cannot be determined | Detection tool unavailable |

Use `Invalid` for conditions where applying would be wrong or dangerous.
Use `Unknown` when the engine genuinely cannot determine state. The engine logs
the reason and skips `Invalid` and `Unknown` resources regardless of
`ProcessMode`.

## ResourceChange Usage

| Variant | Return when |
|---|---|
| `Applied` | Successfully created or updated |
| `AlreadyCorrect` | No change needed (rare in `apply()`) |
| `Skipped { reason }` | Graceful skip (e.g., install command failed non-fatally) |

## Cross-Platform Resources

Use `#[cfg(unix)]` / `#[cfg(windows)]` for platform-specific implementations
within a single resource file (see `chmod.rs`, `symlink.rs`).

For Windows symlinks, use `MetadataExt::file_attributes()` instead of
`symlink_metadata().is_dir()` (see windows-specific-patterns skill).

## Testing Resources

- Self-checking resources: construct directly and call `current_state()` / `apply()` against temp dirs
- Executor-dependent resources: pass `SystemExecutor` for description-only tests; mock the executor for behaviour tests
- Bulk-checked resources: test `state_from_*` mapping and provider-backed task behaviour
- Use `tempfile::tempdir()` for filesystem resources
- Add `#[cfg(test)] mod tests` to every resource module. Keep small tests inline;
  put large resource test modules in `cli/src/resources/tests/<resource>.rs` and
  include them with `#[path = "tests/<resource>.rs"] mod tests;`.

## Rules

- One resource type per file in `resources/`
- Implement `Resource` for apply/remove behaviour on every resource
- Add `IntrinsicState` when state can be checked individually; use `ResourceStateProvider` when state is bulk-checked
- Use `#[must_use]` on constructors and `from_entry()` methods
- All public items need `///` doc comments with `# Errors` on fallible functions
- Resources must implement `Send` for parallel processing
- Use `from_entry()` as the standard factory method for config-to-resource conversion
