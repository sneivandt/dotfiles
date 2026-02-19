---
name: error-handling-patterns
description: >
  Idempotency and error handling conventions, which are core design principles.
  Use when implementing tasks that modify system state, handle errors, or support dry-run mode.
metadata:
  author: sneivandt
  version: "3.0"
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
packages::load(&conf.join("packages.ini"), active_categories)
    .context("loading packages.ini")?;
```

### Task Failure Recording

Task failures don't abort the run. `tasks::execute()` catches errors and records `TaskStatus::Failed`; remaining tasks still execute. The summary reports all failures at the end.

### Intentionally Ignored Errors

Use `.ok()` with a comment, not `let _ =`:

```rust
fs::remove_file(&path).ok(); // Cleanup: ignore if already removed
```

For operations that can legitimately fail but deserve logging, use `if let Err`:

```rust
if let Err(e) = fs::remove_file(&path) {
    ctx.log.debug(&format!("Could not remove {}: {e}", path.display()));
}
```

## Idempotency in Tasks

### Resource-Based Tasks (preferred)

For tasks that manage declarative resources (`Resource` trait), use the generic
`process_resources()` / `process_resource_states()` helpers. They enforce the
correct check→dry-run→apply order automatically:

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    let resources = ctx.config.items.iter()
        .map(|entry| MyResource::from_entry(entry, ctx.executor));
    process_resources(ctx, resources, &ProcessOpts {
        verb: "install",
        fix_incorrect: true,
        fix_missing: true,
        bail_on_error: false,
    })
}
```

`ProcessOpts` controls behaviour per state variant:
- `fix_missing` / `fix_incorrect` — skip states that shouldn't trigger an apply
- `bail_on_error` — `true` propagates `apply()` errors; `false` warns and counts as skipped

See the `rust-patterns` skill for full `ProcessOpts` reference.

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
2. **Use `process_resources()` / `process_resource_states()`** for resource-based tasks — they enforce idempotency and dry-run automatically
3. **Check existing state** before mutations (idempotency) in custom tasks
4. **Check `ctx.dry_run`** before any side effect in custom tasks
5. **Use `.ok()` with a comment** for intentionally ignored errors
6. **Use `if let Err(e)` with debug logging** for errors worth noting
7. **Return `TaskResult` variants** correctly: `Skipped`, `DryRun`, `Ok`
8. **Don't abort on task failure** — record and continue

## Related

- **`rust-patterns`** skill — Task trait and Context struct
- **`logging-patterns`** skill — Logger API and task recording
