---
name: error-handling-patterns
description: >
  Idempotency, dry-run, and error propagation conventions for dotfiles tasks,
  resources, and operations. Use when implementing mutations or handling
  failures in cli/src/.
---

# Error Handling Patterns

## Boundaries

- Commands and tasks return `anyhow::Result` with contextual errors.
- Resources return `ResourceResult<ResourceChange>` so failures remain
  classifiable.
- Tasks own eligibility and result reporting; resources and operations own
  convergence.
- `engine::execute()` records task failures and allows independent work to
  continue.

Use `engine-orchestration` for process modes and scheduling, and
`logging-patterns` for user-visible output and summary behavior.

## Error Layers

| Layer | Pattern |
|---|---|
| command/task | `anyhow::Result` plus `.context(...)` |
| resource mutation | typed `ResourceError` variants |
| optional cleanup | explicit handling with diagnostic logging |
| task execution | return the error; let `engine::execute()` record it |

Prefer typed variants such as `CommandFailed`, `PermissionDenied`,
`ConflictingState`, and `NotSupported` when callers benefit from category-aware
diagnostics. Let `?` convert ordinary I/O and context-rich internal errors where
the conversion preserves useful context.

Do not use broad catches, success-shaped fallbacks, `.ok()`, or `let _ =` to
hide failures. If cleanup is intentionally best-effort, handle `Err` explicitly
and log at the level appropriate to its impact.

## Convergence Order

Every mutation path follows:

1. Discover current state.
2. Return without mutation when already correct.
3. Produce the dry-run preview when `ctx.dry_run`.
4. Apply the mutation.
5. Return the accurate `TaskResult` or `ResourceChange`.

Prefer existing abstractions:

- Independent declarative items: `Resource` with
  `process_resources*()` helpers.
- Workflow-shaped convergence: `Operation` with `process_operation()`.
- Fully custom tasks: implement the same order manually only when neither
  abstraction fits.

Config-backed tasks receive typed `ConfigHandle<T>` values; keep their read
guards out of long-running or parallel work. Route subprocesses through the
context's executor abstraction.

## Result Semantics

- `TaskResult::NotApplicable`: the task is ineligible for this run or discovered
  that it has no applicable work.
- `TaskResult::Skipped`: an applicable task deliberately did not perform its
  work and reported why.
- `TaskResult::DryRun`: changes were identified but not applied.
- `TaskResult::Ok`: execution completed successfully, including no-op success
  where the orchestration helper reports it that way.
- `ResourceChange::AlreadyCorrect`: resource was already converged.
- `ResourceChange::Skipped`: resource was intentionally not changed, with a
  reason.

Do not convert an invalid or unknown state into success. Surface the reason so
the resource processor can apply its configured strict or lenient policy.

## Validation

- Add focused tests for already-correct, dry-run, mutation, and failure paths.
