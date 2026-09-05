---
name: error-handling-patterns
description: >
  Use for error propagation, checked/unchecked subprocess results, dry-run
  ordering, idempotency, TaskResult, or ResourceChange semantics in cli/src/.
  Not for scheduler topology or console formatting alone.
---

# Error Handling Patterns

## Error boundary

| Layer | Return style |
|---|---|
| command or task | `anyhow::Result` with context |
| resource state/apply/remove | typed `ResourceResult<_>` |
| operation state/preview/apply | `anyhow::Result` |
| task execution | return the error; engine records it |

Preserve `ExecError` classification through `ResourceError::Exec`. Propagate
cancellation even in lenient processing.

Do not discard actionable errors with `.ok()`, `let _ =`, or success-shaped
fallbacks. Handle documented absence or optional probes narrowly; permission,
I/O, timeout, and parse failures are not equivalent to "missing." If cleanup is
intentionally best effort, handle and log its error explicitly.

## Subprocess boundary

Use owned [`CommandSpec`](../../../cli/src/infra/exec/mod.rs) requests through
the injected executor. Pass arguments separately and set the working directory
on the request, rather than changing the process-wide directory or building a
shell command string.

Commands are checked by default. Use `.unchecked()` only for a documented
exit-code protocol and classify every outcome, including an absent exit code.
An `Ok(ExecResult)` from an unchecked call is not proof of command success.
Do not flatten typed errors into strings before adding context.

## Mutation order

The lifecycle owner follows this order:

1. Discover state.
2. Return when already correct or inapplicable.
3. Preview and return in dry-run mode.
4. Apply the mutation.
5. Return the exact result.

Prefer `process_resources*()` or `process_operation()` over hand-written loops.
They gate resource mutation; individual resources do not need their own dry-run
flag. An operation's `preview()` must remain read-only and propagate failures.
External overlay scripts enforce this only cooperatively, not through a sandbox.

## Result semantics

- `NotApplicable`: task is not eligible.
- `TaskResult::skipped`: deliberate benign skip.
- `TaskResult::unmet`: applicable work could not converge; not a harmless skip.
- `Failed` or an error: attempted work failed.
- `CheckPassed`: validation succeeded.
- `Ok`: completed without quantified changes.
- `Batch(TaskStats)`: concrete changed/current/skipped/failed counts, including
  planned changes during dry-run. Prefer `TaskStats::changed().finish()` over
  `Ok` when a mutation actually occurred.
- `DryRun`: work is planned but concrete changes cannot be quantified.
- `ResourceChange::skipped`: benign no-op.
- `ResourceChange::unusable`: unmet work; run must fail.

Choosing benign skip for unmet work is a correctness bug. Lenient processing
continues independent work but still counts failures; it does not make them
successful. Preserve the command's completion policy and dependency blocking.
Task-level `unmet` outcomes block dependents and are escalated to failures under
`require_complete` (including CI); failed batch items are failures regardless.

Read the [result types](../../../cli/src/engine/stats.rs) and
[mutation/error handling](../../../cli/src/engine/apply.rs). Cover
already-correct, dry-run, mutation, failure, and cancellation outcomes. Use
`logging-patterns` only when user-visible rendering changes and
[Testing](../../../docs/TESTING.md) for commands.
