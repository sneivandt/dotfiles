---
name: error-handling-patterns
description: >
  Idempotency and error handling conventions, which are core design principles.
  Use when implementing tasks that modify system state, handle errors, or support dry-run mode.
---

# Error Handling Patterns

Idempotency, error handling, and dry-run patterns used in the Rust core engine and shell wrappers.

## Principles

- **Idempotent**: Re-running produces the same result without side effects
- **Defensive**: Check existing state before making changes
- **Fail-Fast**: Errors propagate via `anyhow::Result`; task failures are recorded, not fatal
- **Dry-Run**: Preview mode shows what would change without modifications

## Rust Error Handling

### anyhow::Result

All fallible functions return `anyhow::Result`. Add context with `.context()`:

```rust
packages::load(&conf.join("packages.toml"), active_categories)
    .context("loading packages.toml")?;
```

### ResourceError in Resources

Resource implementations (`resources/*.rs`) should return typed `ResourceError` variants
instead of `anyhow::bail!()`. This enables `categorize_error()` in the processing pipeline
to classify failures for diagnostic logging:

```rust
use crate::error::ResourceError;

// Platform-unsupported operations:
Err(ResourceError::NotSupported {
    reason: "registry operations are only supported on Windows".to_string(),
}.into())

// External command failures:
Err(ResourceError::CommandFailed {
    program: "pacman".to_string(),
    message: format!("exit code {code}"),
}.into())
```

Available variants: `CommandFailed`, `PermissionDenied`, `ConflictingState`, `NotSupported`.

### Task Failure Recording

Task failures don't abort the run. `tasks::execute()` catches errors and
records `TaskStatus::Failed`; remaining tasks still execute. The summary
reports all failures at the end.

### Intentionally Ignored Errors

The `let_underscore_drop` and `unused_result_ok` lints forbid both `let _ =
result` and bare `result.ok();`. Always handle the error explicitly — either
log it, or `drop(...)` an infallible value:

```rust
if let Err(e) = fs::remove_file(&path) {
    tracing::debug!("could not remove {}: {e}", path.display());
}
```

For a value that genuinely needs to be discarded (not a `Result`), use `drop()`
so the intent is explicit:

```rust
drop(some_owned_resource);
```

## Idempotency in Tasks

### Resource-Based Tasks (preferred)

For tasks that manage declarative resources (`Resource` trait), use the generic
`process_resources()` / `process_resources_with_provider()` helpers. They enforce the
correct check→plan/diff→dry-run/apply order automatically:

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    let items = ctx.config_read().items.clone();
    let resources = items.iter()
        .map(|entry| MyResource::from_entry(entry, &*ctx.executor));
    process_resources(ctx, resources, &ProcessOpts::lenient("install"))
}
```

`ProcessOpts` controls behaviour per state variant via a `ProcessMode` enum
(`Strict`, `Lenient`, `InstallMissing`, `FixExisting`). See the
**`engine-orchestration`** skill for the full mode table and constructor
helpers.

### Custom Tasks (non-resource)

For tasks that don't use the `Resource` trait, write the check→dry-run→mutate
loop manually:

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    if already_in_desired_state() {
        return Ok(TaskResult::Ok);
    }
    if ctx.dry_run {
        ctx.log.dry_run("would do something");
        return Ok(TaskResult::DryRun);
    }
    perform_mutation()?;
    Ok(TaskResult::Ok)
}
```

### Pattern Order

1. Check if already in desired state → skip or count as `already_ok`
2. Check dry-run flag → log and return `DryRun`
3. Perform the mutation → `Ok`

This order ensures dry-run never mutates, and re-runs skip completed work.

## Shell Wrapper Error Handling

The shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) are thin but strict:

- `dotfiles.sh` uses `set -o errexit` and `set -o nounset`
- `dotfiles.ps1` uses `$ErrorActionPreference = 'Stop'`

Both verify checksums after downloading binaries and fall back to existing binaries when GitHub is unreachable.

## Rules

1. **Use `anyhow::Result` with `.context()`** for all fallible Rust code
2. **Use `process_resources()` / `process_resources_with_provider()`** for resource-based tasks — they enforce idempotency and dry-run automatically
3. **Check existing state** before mutations (idempotency) in custom tasks
4. **Check `ctx.dry_run`** before any side effect in custom tasks
5. **Do not use `.ok()` to ignore errors**; handle them explicitly
6. **Use `if let Err(e)` with debug logging** for errors worth noting
7. **Return `TaskResult` variants** correctly: `Skipped`, `DryRun`, `Ok`
8. **Don't abort on task failure** — record and continue

## Related

- **`rust-patterns`** skill — Task trait and Context struct
- **`logging-patterns`** skill — Logger API and task recording
- **`engine-orchestration`** skill — `ProcessMode` / `ProcessOpts` reference
