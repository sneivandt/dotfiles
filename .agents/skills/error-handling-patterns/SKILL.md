---
name: error-handling-patterns
description: >
  Use for error propagation, dry-run ordering, idempotency, TaskResult, or
  ResourceChange semantics in cli/src/. Not for scheduler topology or console
  formatting alone.
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

Never hide failures with broad catches, `.ok()`, `let _ =`, or success-shaped
fallbacks. If cleanup is intentionally best effort, handle and log its error
explicitly.

## Mutation order

Every mutation path is:

1. Discover state.
2. Return when already correct or inapplicable.
3. Preview and return in dry-run mode.
4. Apply the mutation.
5. Return the exact result.

Prefer `process_resources*()` or `process_operation()` over hand-written loops.

## Result semantics

- `NotApplicable`: task is not eligible.
- `Skipped`: eligible work was intentionally not performed.
- `DryRun`: a change was found but not applied.
- `Ok`: execution completed successfully.
- `ResourceChange::skipped`: benign no-op.
- `ResourceChange::unusable`: unmet work; run must fail.

Choosing benign skip for unmet work is a correctness bug because it produces a
successful exit.

Add focused already-correct, dry-run, mutation, and failure tests. Use
`logging-patterns` only when user-visible rendering changes.
