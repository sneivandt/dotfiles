---
name: error-handling-patterns
description: >
  Use for error propagation, checked/unchecked subprocess results, argument
  redaction, output-stream retention, TaskResult, or ResourceChange semantics
  in cli/src/. Not for scheduler topology or console formatting alone.
---

# Error Handling Patterns

See [result types](../../../cli/src/engine/stats.rs),
[mutation/error handling](../../../cli/src/engine/apply.rs), and
[`CommandSpec`](../../../cli/src/infra/exec/mod.rs).

## Error and cancellation propagation

- Return `anyhow::Result` from commands/tasks/operations and typed
  `ResourceResult<_>` from resources; preserve `ExecError` classification
  through `ResourceError::Exec` rather than flattening to strings before adding
  context.
- Propagate cancellation even in lenient processing; lenient mode continues
  independent items but never clears failed-item accounting.

## Subprocess boundary

- Route subprocesses through `CommandSpec` and the injected executor; commands
  are checked by default. Use `.unchecked()` only for a documented exit-code
  protocol and classify every outcome, including an absent exit code (signal
  termination)—an `Ok(ExecResult)` from `.unchecked()` is not proof of success.
- Argument redaction and stream retention are independent:
  - `CommandSpec::redact_arguments()` hides sensitive argv entries in logs.
  - `CommandSpec::output_log` controls persistent stream capture: `Diagnostics`
    retains failed streams and successful stderr, `Full` adds successful stdout,
    and `Omit` suppresses streams in logs and checked errors (though the caller
    still receives the `ExecResult`, so avoid re-logging sensitive output).

## Unmet vs skipped semantics

- Distinguish benign `TaskResult::skipped` / `ResourceChange::skipped` from
  `TaskResult::unmet` (applicable work could not converge) and
  `ResourceChange::unusable` (item failure).
- Task-level `unmet` blocks dependents and escalates to failure under
  `require_complete` (including CI); failed batch items (`unusable` / `failed`)
  fail the run regardless.
- Prefer `TaskStats::changed().finish()` over `TaskResult::Ok` when a mutation
  actually occurred; use `TaskResult::DryRun` only when planned changes cannot
  be quantified.
