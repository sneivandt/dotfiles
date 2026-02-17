---
name: error-handling-patterns
description: >
  Idempotency and error handling conventions, which are core design principles.
  Use when implementing tasks that modify system state, handle errors, or support dry-run mode.
metadata:
  author: sneivandt
  version: "2.0"
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

### Check Before Mutate

Every task's `run()` must check existing state before acting:

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    if link_path.exists() && link_path.read_link()? == target {
        return Ok(TaskResult::Skipped); // Already correct
    }
    if ctx.dry_run {
        return Ok(TaskResult::DryRun);
    }
    std::os::unix::fs::symlink(&target, &link_path)?;
    Ok(TaskResult::Ok)
}
```

### Pattern Order

1. Check if already in desired state → `TaskResult::Skipped`
2. Check dry-run flag → `TaskResult::DryRun`
3. Perform the mutation → `TaskResult::Ok`

This order ensures dry-run never mutates, and re-runs skip completed work.

## Shell Wrapper Error Handling

The shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) are thin but strict:

- `dotfiles.sh` uses `set -o errexit` and `set -o nounset`
- `dotfiles.ps1` uses `$ErrorActionPreference = 'Stop'`

Both verify checksums after downloading binaries and fall back to existing binaries when GitHub is unreachable.

## Rules

1. **Use `anyhow::Result` with `.context()`** for all fallible Rust code
2. **Check existing state** before mutations (idempotency)
3. **Check `ctx.dry_run`** before any side effect
4. **Use `.ok()` with a comment** for intentionally ignored errors
5. **Use `if let Err(e)` with debug logging** for errors worth noting
6. **Return `TaskResult` variants** correctly: `Skipped`, `DryRun`, `Ok`
7. **Don't abort on task failure** — record and continue

## Related

- **`rust-patterns`** skill — Task trait and Context struct
- **`logging-patterns`** skill — Logger API and task recording
