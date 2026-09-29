# Architecture

Dotfiles is a desired-state CLI, not a sequence of installation scripts.
Configuration says what should exist; domain code determines how to converge it;
the engine schedules work and records what happened. The same application runs
on Linux and Windows.

Read this guide to choose an implementation boundary or trace a command.
[Contributing](CONTRIBUTING.md) covers the change workflow,
[Configuration](CONFIGURATION.md) the data format, and [Testing](TESTING.md) the
checks that protect these contracts.

## System view

```text
dotfiles.sh / dotfiles.ps1                 bootstrap or build, then forward
             |
             v
app: CLI -> runtime policy -> command runner
                               |
                profiles + main/overlay configuration
                               |
                      immutable ConfigStore
                               |
             catalog + command tasks + dynamic overlay tasks
                               |
                  selection -> execution coordinator
                               |
engine:             dependency graph -> scheduler
                                          |
                                     Task::run
                                      /       \
domains:                         Resource    Operation
                                      \       /
infra:                    filesystem / executor / platform adapters
                                          |
                         structured outcomes -> console + retained log
```

**A concrete trace: home symlinks**

1. [`app/run.rs`](../cli/src/app/run.rs) parses an install command and resolves
   startup policy. [`CommandRunner`](../cli/src/app/commands/runner.rs) resolves
   root, overlay and profile, loads configuration, and builds the context.
2. [`Config::load`](../cli/src/app/config/mod.rs) decodes
   [`conf/symlinks.toml`](../conf/symlinks.toml), appends the overlay's entries,
   selects active categories, expands source globs and rejects target conflicts.
3. The [`catalog`](../cli/src/app/catalog.rs) gives `InstallSymlinks` a typed
   configuration handle and adds its cross-domain dependency. The
   [`filter`](../cli/src/app/filter.rs) selects the requested tasks.
4. The coordinator assesses applicability and elevation, then the scheduler
   dispatches the task when its prerequisites are satisfied.
5. [`InstallSymlinks`](../cli/src/domains/files/symlinks.rs) turns each entry into
   a [`SymlinkResource`](../cli/src/domains/files/resources/symlink.rs). The
   resource discovers state; the engine chooses a no-op, skip or change. Dry-run
   renders the change without calling its apply method.
6. Resource results become task statistics and dependency outcomes. Logging
   presents them, but the scheduler's execution summary decides command success.

An operation follows the same task/scheduler path; only the task body changes.

## Repository layout

| Location | Owns | Does not own |
|---|---|---|
| [`app/`](../cli/src/app) | CLI, aggregate configuration, command composition, cross-domain wiring, startup/restart/elevation policy | Concrete domain mutations |
| [`engine/`](../cli/src/engine) | Task graph, scheduling, resource plans, operation lifecycle, result accounting | Which packages, files or applications to configure |
| [`domains/`](../cli/src/domains) | Typed domain data, tasks, resources, workflows and provider contracts | Imports from sibling domains to coordinate them |
| [`infra/`](../cli/src/infra) | Process, filesystem, environment, platform, logging and other system mechanisms | Install/update command membership |
| [`conf/`](../conf), [`symlinks/`](../symlinks), [`system/`](../system) | Desired state, managed home content and privileged file fragments | Scheduler policy |
| [`hooks/`](../hooks), [workflows](../.github/workflows) | Commit checks and CI/release automation | Runtime application behavior |

[`domain_boundaries`](../cli/tests/domain_boundaries.rs) checks architectural
boundaries against Rust syntax, including domain imports and direct platform or
environment access. These boundaries are executable constraints, not just a
directory convention.

## Wrappers

[`dotfiles.sh`](../dotfiles.sh) and [`dotfiles.ps1`](../dotfiles.ps1) locate the
checkout and binary, consume `--build`, build or download when necessary, export
bootstrap context, and forward the remaining arguments. They must not implement
install, update, selection or profile semantics independently of Rust.

Building uses Cargo's reported executable artifact rather than assuming a
particular target directory. Downloaded binaries receive checksum and provenance
checks with the limitations in [Security](SECURITY.md#release-downloads).
An already available binary is not proof that it matches edited Rust source.

## Application layer

[`cli.rs`](../cli/src/app/cli.rs) defines public syntax.
[`run.rs`](../cli/src/app/run.rs) separates engine commands from standalone
`tasks`, `log` and completion commands. `update` and `install --update` enter
the same install pipeline with update membership enabled.

Engine commands resolve one immutable
[`RuntimePolicy`](../cli/src/app/commands/runtime.rs) **before logging starts**.
It captures flags, the injected environment and terminal capabilities.
Profile prompting, restart guards and elevation consume this decision;
[`Context`](../cli/src/engine/context/mod.rs) receives a path-free
`ExecutionPolicy` rather than interpreting the flags again.

Important distinctions:

- `CI` and re-exec guards use **presence**, even an empty value. CI makes
  execution non-interactive and requires applicable work to complete.
- Non-interactive does not itself mean strict completion. Missing tools can be
  reported as unmet skips locally; `--fail-on-skip` makes them failures.
- The elevated-child environment marker requires a nonempty value. Windows
  exit-pause and interrupt compatibility behavior is separate from task prompt
  policy.

[`CommandRunner`](../cli/src/app/commands/runner.rs) holds the run lock, immutable
configuration store and execution context. Static install/uninstall tasks come
from [`catalog.rs`](../cli/src/app/catalog.rs); check tasks come from
[`commands/check.rs`](../cli/src/app/commands/check.rs). The
[`execution coordinator`](../cli/src/app/commands/execution/mod.rs) owns the
policy spanning scheduler phases: elevation preparation, restart boundaries,
visible progress and final command status.

## Configuration flow

```text
root + overlay + resolved profile
                |
       parse each TOML document
                |
    structural preflight + domain decoding
                |
   main entries, then overlay entries (with source origins)
                |
 active categories / platform views -> expansion + conflict checks
                |
       Config -> ConfigStore -> typed task handles
```

The [aggregate loader](../cli/src/app/config/mod.rs) owns merge order.
`ConfigDocument` lets category checks and typed decoding share a parsed tree and
source spans. `SectionLoader` appends main then overlay records, retaining the
origin needed to resolve paths. Overlay script definitions are deliberately
loaded **only** from the overlay. Some sections also retain unfiltered records
for validation, so inactive sources can still be checked.

Structural errors, conflicting symlink targets and contradictory active
Git/registry declarations fail loading. Other semantic diagnostics are returned
by `Config::validate`: startup displays them; the `config-warnings` check task
fails if any remain. Do not silently turn diagnostics into defaults or make
every warning a load-time error.

[`ConfigStore`](../cli/src/app/config/store.rs) distributes
[`Arc`-backed handles](../cli/src/infra/config/handle.rs). `read()` clones an
immutable handle, not the underlying data; it neither locks mutable state nor
reloads disk. Tasks, including dynamic overlay tasks, are constructed once from
that snapshot.

### Repository updates are a restart boundary

An install may synchronize the checkout before applying the rest of its tasks.
Changing files beneath an already-built task catalog would leave stale inputs,
so the coordinator first executes the dependency closure ending at repository
update. When that phase successfully changes content, it starts the **current
binary** with the original arguments and repository restart guard. The child
reloads configuration and constructs a fresh catalog.

The parent retains the run lock while waiting. The child does not reacquire it,
repeat repository synchronization or retry the original self-update preflight.
Cancellation or failure prevents restart. If no content changed, remaining work
uses the existing snapshot; if the boundary was filtered out, execution uses a
single graph. Selection and dry-run retain their normal meaning.
See [`install.rs`](../cli/src/app/commands/install.rs) and
[`reexec.rs`](../cli/src/app/commands/reexec.rs).

## Task engine

[`Task`](../cli/src/engine/task/mod.rs) owns metadata and orchestration policy;
it is not synonymous with a resource.

| Identity or metadata | Purpose |
|---|---|
| `task_id()` | DAG identity; static type or type plus a stable dynamic instance key |
| `selector()` | Stable public value used by `--only` and `--skip` |
| `name()` | Human-readable label, allowed to change independently |
| `log_key()` | Persistent identity, including dynamic instance keys |
| Visibility | Whether a task appears in ordinary discovery, rows and totals |
| `update_only()` | Command membership, **not** an ordering tier |

Dependencies are the only ordering mechanism; catalog position is irrelevant.
[`graph.rs`](../cli/src/engine/graph.rs) rejects duplicate identities and cycles.
Blocking prerequisites propagate unmet work/failure; ordering-only predecessors
delay a task without making their failure its failure. Same-domain edges belong
in the task; cross-domain edges belong in the application catalog.

Applicability and elevation are assessed once per execution phase and reused
for dispatch. These probes must be read-only and depend on phase-stable state.
Check for a tool or file produced by a prerequisite inside `run()`, after that
prerequisite finishes. Empty configuration should return `NotApplicable`, not
claim that work was applied.

[`scheduler.rs`](../cli/src/engine/scheduler.rs) runs ready tasks in scoped
threads; resource batches can independently use Rayon. `ctx.parallel()` gates
both. Resource `.sequential()` protects shared-file or lock-bound writes within
one task; it cannot serialize separate tasks. Those need graph edges.

[`tasks`](../cli/src/app/commands/tasks.rs) loads a read-only configuration
snapshot and exposes selectors and graph relationships without a run log, lock,
or persisted selections. Its graph describes selection, not actual runtime
applicability. Internal tasks can appear in graph diagnostics but are not public
selectors. See [Task reference](TASKS.md) for user-facing discovery and filtering.

## Resources

Use a [`Resource`](../cli/src/engine/resource/contract.rs) for independently
convergent items such as symlinks, packages or registry entries:

```text
state discovery -> pure plan -> dry-run preview OR apply -> ResourceChange
```

State discovery uses `IntrinsicState::current_state()` or an injected batch
lookup when one system query can serve many resources. The
[`plan`](../cli/src/engine/plan.rs) and
[`apply`](../cli/src/engine/apply.rs) layers are separate:

- `Correct` is a no-op. `Missing` and `Incorrect` are acted on according to
  [`ProcessMode`](../cli/src/engine/mode.rs).
- `Unknown` is not `Missing`: failed discovery must not cause blind creation.
  `Invalid` and `Unknown` leave unmet work.
- `ResourceChange` distinguishes applied, already correct and skipped work.
  A benign skip and an unsuccessful attempt must not share success semantics.
- Strict mode stops on errors; lenient mode continues independent items but
  retains failure accounting. Mode and parallelism are independent decisions.
- Removal is a separate `RemovableResource` capability. The default removal
  plan acts only on matching managed state, not arbitrary mismatched user data.

Stopped batches carry a typed [`BatchReport`](../cli/src/engine/batch.rs):
completed statistics, interrupted work and unattempted items. Parallel execution
joins already-started work before reporting. Do not replace a partial failure
with an empty failure or let later cancellation erase an earlier error.

## Operations

Use an [`Operation`](../cli/src/engine/operation.rs) for a workflow that must
converge as one unit rather than independent records: repository synchronization
and overlay scripts are examples.

`current_state()` returns complete, not applicable, blocked, or needs-run with
an immutable plan. `process_operation()` passes that **same plan** to either
`preview()` or `apply()`. Planning must be read-only, and preview must not
recompute or partially apply it. This is a lifecycle contract, not a transaction:
external scripts cooperate with `--check`/`--dryrun`; the engine cannot sandbox
them or roll back a partially completed workflow.

## Platform abstraction

Tasks use context capabilities and injected environment/executor/filesystem
adapters rather than scattering process-global reads or OS checks. Concrete
platform implementations can still require compile-time `cfg` gates.
[`platform.rs`](../cli/src/infra/platform.rs) owns capability detection;
[`exec/`](../cli/src/infra/exec) owns process mechanics.

The [elevation broker](../cli/src/app/commands/execution/elevation.rs) scopes
privilege to declared tasks: Linux primes sudo credentials for privileged
commands; Windows delegates selected tasks to one elevated child. Unavailable
elevation leaves unmet work and blocks blocking dependents, not unrelated work.
See [Security](SECURITY.md#elevation) for prompt and privilege limitations.

## Error handling and observability

Use checked [`CommandSpec`](../cli/src/infra/exec/mod.rs) requests unless a
nonzero exit has a specific domain meaning that the caller handles.
Typed errors preserve spawn, I/O, nonzero-exit, timeout and cancellation causes
through resource/task boundaries.

Timeout and cancellation cover both child lifetime and captured-pipe draining.
Unix process groups and Windows job objects allow cleanup of descendants that
retain output pipes after the leader exits; Windows assignment occurs before
execution. This prevents indefinite capture waits, not arbitrary-code execution.

[`TaskResult`/statistics](../cli/src/engine/stats.rs) feed structured dependency
outcomes and logging records. The scheduler's `ExecutionSummary`, **not logger
counters**, decides completion. Interruption exits 130; genuine failure takes
precedence and exits 1. Neither implies rollback.

[`infra/logging/`](../cli/src/infra/logging) separates presentation from retained
records. Normal output shows changed work and attention-worthy outcomes;
verbose output adds current tasks and decision details. Internal and
non-applicable tasks stay out of normal rows/totals. Completed rows retain
completion order rather than being sorted after execution.

Run logs retain chronological, schema-versioned records for task identity,
actions, duration and commands. Restarted/elevated processes have their own run
IDs linked to a parent. A missing finish record means unfinished, not a proven
crash. `dotfiles log` reads these records without creating a new run and can
also read legacy text logs. The newest 50 process logs are retained.

Command argument redaction and captured-output policy are independent. The
default retains failed streams and successful stderr while summarizing
successful stdout by byte count. `OutputLog::Omit` excludes streams from both
logs and checked-command errors. This is opt-in control, not automatic secret
scrubbing; see [Secrets](SECURITY.md#secrets).

## Extending the system

Choose the smallest suitable boundary:

| Need | Prefer |
|---|---|
| Different desired values or managed application content | `conf/`, `symlinks/` or `system/` |
| Another independently convergent item | Resource plus a thin task adapter |
| One idempotent multi-step workflow | Operation |
| Selection, dependencies, visibility or elevation policy | Task/application catalog |
| OS/tool interaction reusable below a domain | Infrastructure adapter |
| Pure parsing or transformation | Plain function/module |

Do not add a task merely to split a function, or a resource merely to wrap one
command. Follow [Change checklists](CONTRIBUTING.md#change-checklists) for the
minimum wiring, regression coverage and documentation.
