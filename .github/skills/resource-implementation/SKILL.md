---
name: resource-implementation
description: >
  Patterns for implementing concrete Resource and Applicable types in
  cli/src/resources/. Use when adding a new resource or modifying existing
  resource behaviour.
metadata:
  author: sneivandt
  version: "1.0"
---

# Resource Implementation

Resources in `cli/src/resources/` are the declarative primitives that check
and apply system state. Each resource file implements either `Resource`
(self-checking) or `Applicable` (bulk-checked).

## Which Trait to Implement

| Trait | When | Examples |
|---|---|---|
| `Resource` (implies `Applicable`) | Resource can independently check its own state | `SymlinkResource`, `ChmodResource`, `GitConfigResource`, `HookFileResource`, `WrapperResource`, `PathEntryResource` |
| `Applicable` only | State requires a single bulk query shared across instances | `VsCodeExtensionResource`, `PackageResource` |

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

impl Applicable for MyResource {
    fn description(&self) -> String { format!("{}", self.target.display()) }
    fn apply(&self) -> Result<ResourceChange> { /* create/update */ }
    fn remove(&self) -> Result<ResourceChange> { /* undo */ }
}

impl Resource for MyResource {
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
    fn current_state(&self) -> Result<ResourceState> {
        let result = self.executor.run_unchecked("tool", &["check", &self.name])?;
        if result.success { Ok(ResourceState::Correct) } else { Ok(ResourceState::Missing) }
    }
}
```

The `Executor: Debug` supertrait allows `#[derive(Debug)]`. Resources are not `Clone`
when they hold trait object references.

## Bulk-Checked (Applicable-Only) Template

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

impl Applicable for MyResource {
    fn description(&self) -> String { self.id.clone() }
    fn apply(&self) -> Result<ResourceChange> { /* install */ }
}

// Provide a standalone query function:
pub fn get_installed(executor: &dyn Executor) -> Result<HashSet<String>> { /* single command */ }
```

The task calls `get_installed()` once, then builds `(resource, state)` pairs
and passes them to `process_resource_states()`.

Real examples: `VsCodeExtensionResource`, `PackageResource`.

## ResourceState Usage

| Variant | Meaning | Typical check |
|---|---|---|
| `Missing` | Does not exist | Path/entry not found |
| `Correct` | Matches desired state | Symlink points correctly, value matches |
| `Incorrect { current }` | Exists but wrong | Symlink wrong target, wrong value |
| `Invalid { reason }` | Cannot be applied | Source missing, target is real directory |

Use `Invalid` for conditions where applying would be wrong or dangerous.
The engine logs the reason and skips the resource regardless of `ProcessMode`.

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
- Use `tempfile::tempdir()` for filesystem resources
- Add `#[cfg(test)] mod tests` to every resource module

## Rules

- One resource type per file in `resources/`
- Implement `Resource` when state can be checked individually; `Applicable` when bulk-checked
- Use `#[must_use]` on constructors and `from_entry()` methods
- All public items need `///` doc comments with `# Errors` on fallible functions
- Resources must implement `Send` for parallel processing
- Use `from_entry()` as the standard factory method for config-to-resource conversion
