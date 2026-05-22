---
name: engine-orchestration
description: >
  Task scheduling and resource parallelism in the dotfiles engine. Use when
  working with task dependencies, parallel execution, the scheduler, or
  the Rayon-based resource processing pipeline.
---

# Engine Orchestration

The engine has two levels of parallelism: **task-level** (scheduler) and
**resource-level** (Rayon). Both are gated by `ctx.parallel`.

## Phase Barrier

`run_tasks_to_completion()` in `commands/mod.rs` enforces a strict three-phase
execution model: all **Bootstrap** tasks complete before any **Repository**
task starts, and all **Repository** tasks complete before any **Apply** task
starts.  The function loops over
`[TaskPhase::Bootstrap, TaskPhase::Repository, TaskPhase::Apply]`, filtering
and dispatching tasks per phase.  Within each phase, tasks run via the
scheduler (parallel) or sequentially (single task / `--no-parallel`).

Before parallel dispatch, the runner evaluates task execution policies and
`should_run()` as part of `requires_elevation()`. It primes sudo only for tasks
that declare `ExecutionPolicy::RequiresElevation`, are supported on the current
platform, are not skipped by dry-run policy, are applicable, and predict a
privileged mutation via `needs_elevation()`.

## Task-Level: Scheduler

Within each phase, `run_tasks_to_completion()` dispatches:

1. If `ctx.parallel` and more than one task → check for cycles via `graph::has_cycle()` (Kahn's algorithm)
2. If cycle detected → bail with an error (abort the run)
3. Otherwise → `engine::scheduler::run_tasks_parallel()` spawns OS threads

Each dispatched task is still passed through `phases::execute()`, which applies
`execution_policies()` before `should_run()` and `run_if_applicable()`.

### Why OS Threads (Not Rayon)

Tasks block on `mpsc::Receiver` waiting for dependencies. Blocking inside a
Rayon worker would exhaust the fixed-size thread pool and deadlock on machines
with fewer cores than tasks (e.g., 2-vCPU CI runners). `std::thread::scope`
gives each task its own thread.

### Channel Wiring

For each task:
- A `(Sender, Receiver)` channel is created sized to the dependency count
- Each dependency holds a clone of the task's `Sender`
- When a dependency finishes, it sends `()` on all its cloned senders
- The task blocks on `rx.recv()` once per dependency
- If a sender is dropped without sending (dependency panicked), `recv()` returns `Err` — the task is skipped and propagates the failure by dropping its own senders

### Buffered Output

Each parallel task receives a `BufferedLog` via `ctx.with_log()`. Output is
captured in memory and flushed atomically via `buf.flush_and_complete(name)`
after the task finishes.

### Sequential Fallback

When `ctx.parallel` is false, single task, or cycle detected:

```rust
for task in &tasks {
    tasks::execute(*task, ctx);
}
```

## Resource-Level: Rayon

Within a single task, resources are processed in parallel when
`ctx.parallel` is true and there is more than one resource.

`engine/orchestrate.rs` dispatches to `engine/parallel.rs`:

```rust
if ctx.parallel && resources.len() > 1 {
    parallel::process_apply_parallel(ctx, resources, opts, get_resource_state)
} else {
    // sequential apply loop
}
```

### How It Works

`process_apply_parallel()` handles both self-checking resources and precomputed
`(resource, state)` pairs. It delegates to `collect_parallel_stats()`, which
uses Rayon's `try_fold` / `try_reduce` pattern:
- Each item runs `process_single()` independently after the caller-provided
  state extraction closure returns `(Resource, ResourceState)`
- Each worker accumulates local `TaskStats`, then results are merged without a
  shared stats lock
- Diagnostic thread names are re-set per iteration since Rayon reuses threads

Removal still uses `process_remove_parallel()`, which calls `remove_single()` for
resources whose current state is `Correct`.

### Resource `Send` Requirement

Resources must implement `Send` for parallel processing. Because
`Executor: Sync`, all resources holding `&dyn Executor` satisfy `Send`
automatically.

## Resource Processing Helpers

`engine/orchestrate.rs` provides three helpers that drive the
check→plan/diff→dry-run/apply loop so individual tasks don't repeat it. They
are re-exported from `phases/mod.rs`:

- **`process_resources(ctx, resources, opts)`** — calls `current_state()` on each resource.
- **`process_resource_states(ctx, resource_states, opts)`** — takes pre-computed `(Resource, ResourceState)` pairs for batch-checked resources (packages, VS Code extensions).
- **`process_resources_remove(ctx, resources, verb)`** — uninstall counterpart: removes resources in `Correct` state, skips others.

Use these helpers for **all** new resource-based tasks. They build typed
`ApplyChange` / `RemoveChange` plans, then handle dry-run checks, error
categorisation, parallel dispatch, and stats accumulation.

### ProcessMode

`ProcessMode` makes the intent of each processing strategy explicit:

| Mode | `fix_incorrect()` | `fix_missing()` | `bail_on_error()` | Typical use |
|---|---|---|---|---|
| `Strict` | yes | yes | yes | symlinks, hooks, git config |
| `Lenient` | yes | yes | no | packages, registry, developer mode |
| `InstallMissing` | no | yes | no | VS Code extensions, systemd units |
| `FixExisting` | yes | no | yes | chmod (files may not exist yet) |

### Resource Plans

`ProcessMode::action_for(&ResourceState)` returns a low-level `ResourceAction`.
`engine/plan.rs` wraps that decision with the resource description and renderable
metadata:

```rust
ApplyChange::from_state(description, state, opts);
RemoveChange::from_state(description, state, "unlink");
```

The processing loop (`process_single` / `remove_single`) executes these plans
instead of nesting matches on `ResourceState` and mode flags. This keeps the
lifecycle state machine explicit, side-effect-free, and independently testable.

### ProcessOpts

`ProcessOpts` pairs a `ProcessMode` with a human-readable `verb` for log messages.
Use named constructors:

```rust
ProcessOpts::strict("link")            // fix Missing+Incorrect, bail on errors
ProcessOpts::lenient("install")        // fix Missing+Incorrect, warn on errors
ProcessOpts::install_missing("enable") // only fix Missing, warn on errors
ProcessOpts::fix_existing("chmod")     // only fix Incorrect, bail on errors
```

## Dependency Graph

`engine/graph.rs` provides `has_cycle()` using Kahn's algorithm:
- Builds in-degree counts and reverse-dependency adjacency lists
- Processes zero-in-degree nodes; if `processed != total` → cycle exists
- Missing dependencies (filtered tasks) are silently ignored

## Diagnostic Events

The scheduler emits structured events to the diagnostic log:

| Event | Meaning |
|---|---|
| `TaskWait` | Task spawned, listing dependencies |
| `TaskStart` | All dependencies satisfied, executing |
| `TaskDone` | Task completed |
| `TaskSkip` | Skipped due to failed dependency |

## Rules

- Task parallelism uses `std::thread::scope` — never Rayon for task scheduling
- Resource parallelism uses Rayon — never OS threads for resource processing
- Both levels are gated by `ctx.parallel`
- Tests set `parallel: false` to keep execution deterministic
- Cycle detection bails with an error — never falls back to sequential
- `BufferedLog` must be flushed via `flush_and_complete()` after each parallel task
