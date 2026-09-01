# Architecture

The wrappers handle bootstrap only. The Rust code separates application
orchestration, domain behavior, infrastructure adapters, and declarative desired
state.

## System view

```text
dotfiles.sh / dotfiles.ps1
          |
          v
      Rust CLI (clap)
          |
          v
 application commands and task catalog
          |
          v
 dependency graph + task executor
          |
          +------------------+
          v                  v
   resources/providers    operations
          |                  |
          +--------+---------+
                   v
          platform/executor/filesystem
```

## Repository layout

| Path | Responsibility |
|---|---|
| `dotfiles.sh`, `dotfiles.ps1` | Binary bootstrap/build and argument forwarding |
| `cli/src/app/` | CLI definitions, command composition, catalog, aggregate config, validation |
| `cli/src/engine/` | Task scheduling, resource convergence, operations, logging contracts |
| `cli/src/domains/` | Git, packages, files, system, AI, editor, repository, shell, and overlay behavior |
| `cli/src/infra/` | Platform detection and concrete system adapters |
| `conf/` | Declarative desired state |
| `symlinks/` | Versioned files linked into the user's home directory |
| `hooks/` | Repository-maintained Git hooks and checks |
| `.github/workflows/` | CI and release publishing |

## Wrappers

The wrappers:

1. Determine the repository root and target binary.
2. Consume wrapper-only `--build`.
3. Build from source or download a release asset when needed.
4. Verify downloaded content.
5. Export bootstrap context.
6. Execute the Rust CLI with all remaining arguments unchanged.

Do not add command semantics to the wrappers. The Rust CLI must remain the
single implementation on Linux and Windows.

## Application layer

The application layer composes commands and configuration:

- `cli.rs` defines public commands and options.
- `catalog.rs` constructs the static install and uninstall task lists.
- command modules select/filter tasks and execute them.
- aggregate configuration loading merges domain-specific configuration.
- validation modules build the `test` workflow.

Cross-domain dependencies belong here. A domain task may declare same-domain
prerequisites, while the catalog decorates it with dependencies on tasks from
other domains.

## Task engine

Every task exposes:

- a scheduler identity
- a stable CLI selector
- a human-readable display label
- user-facing or internal visibility
- command membership such as update-only behavior
- failure-blocking and ordering-only dependency identities
- one immutable applicability/elevation assessment per execution phase
- execution returning a structured task result

These identities are independent. `TaskId` is the DAG key, `selector()` is the
CLI value used by `--only`, and `name()` is the display label. `visibility()`
controls discovery, normal console rows, and totals.

The coordinator computes each task's `TaskAssessment` once and shares it between
elevation preparation and dispatch. Assessment probes must use state stable for
the phase; checks for state produced by a prerequisite run from
`run_configured()` after dependencies finish.

The scheduler validates a dependency graph and runs ready tasks in parallel.
Every ordering requirement is an explicit edge; the order of entries in
`catalog.rs` is not execution order. Failure-blocking prerequisites stop
dependents, while ordering-only predecessors merely delay them until completion.
Duplicate identities and cycles fail before execution with the conflicting
identities or closed cycle path. Visible rows retain natural completion order;
completed work is not sorted or grouped afterward.

`Task::update_only()` is command membership metadata, not an ordering class.
`install` excludes update-only tasks unless `--update-pins` includes them in
the same graph as ordinary install tasks.

Dynamic overlay tasks use structured identities containing their concrete task
type and complete stable instance key, avoiding hash collisions when multiple
configured scripts share one Rust task type. They use
`script-<normalized-script-name>` selectors.

`dotfiles tasks` loads a read-only configuration snapshot, merges visible
metadata across command catalogs by selector, and prints selector, label, and
command membership in discovery order. It does not create a log, acquire the
run lock, or persist profile and overlay selections. `--only` performs exact
normalized selector matching; exact full-label matching remains available for
compatibility. Internal tasks are not discoverable or selectable.

## Resources

A `Resource` models independently convergent desired state:

1. Discover current intrinsic state.
2. Compare it with desired state.
3. Produce a change plan.
4. Preview or apply that change.

Resources are used for packages, symlinks, registry values, permissions, and
similar state. Providers can batch or cache state discovery, reducing repeated
system calls.

Resource processing respects dry-run and returns explicit outcomes such as
applied, already correct, skipped, invalid, or unknown. A skipped outcome
records whether the skip is harmless or leaves work unfinished. Unfinished work
can still fail the run. Tasks turn these outcomes into user-facing summaries.

## Operations

An `Operation` models a whole workflow that converges as a unit rather than a
collection of independent records. It has current-state, preview, and apply
steps. Repository synchronization and convention-based overlay scripts use this
model because their correctness depends on completing a coherent workflow.

## Configuration flow

```text
profile resolution
      |
      v
main TOML load ---- overlay TOML load
      |                  |
      +------ append ----+
              |
              v
      aggregate validation
              |
              v
        ConfigStore handles
              |
              v
         catalog tasks
```

Each domain owns its parser and typed records. The app-level loader guarantees
that supported overlay sections are merged consistently.

`ConfigStore` publishes immutable, `Arc`-backed handles. Static catalog tasks and
dynamic overlay tasks are built once from that startup snapshot.

Repository synchronization is a guarded process boundary:

1. Run the dependency closure ending at `UpdateRepository`.
2. Continue normally when the checkout did not change, the boundary was
   filtered out, execution failed, or execution was cancelled.
3. When content changed, spawn the current binary with the original arguments
   and repository re-exec guard, then wait for it while retaining the run lock.
4. The child reloads configuration and rebuilds all tasks from the updated
   checkout. It omits repository synchronization and continues with the selected
   work.

The guard also suppresses self-update and run-lock reacquisition in the child.
`--only`, `--skip`, `--retry-failed`, `--no-repo-update`, dry-run, and elevation retain
their normal selection semantics. A filtered boundary falls back to one graph.

## Platform abstraction

Tasks prefer capability methods exposed by context and system adapters rather
than scattering direct operating-system checks. Platform guards still determine
applicability, but concrete mutations are delegated to the relevant adapter or
provider.

The abstraction provides:

- Linux and Windows implementations behind common contracts
- test doubles for filesystem, command execution, and process environment
- explicit capability failures instead of silent platform assumptions
- elevation planning before parallel task dispatch, scoped to the tasks that
  declare it rather than the whole process

Tasks and resources read environment variables through the context adapter, not
process globals. Tests can provide a fixed environment without changing shared
state. Startup code runs before a context exists, so argument parsing, re-exec
guards, and log-directory discovery still read the process environment
directly.

## Error handling and observability

Errors propagate with context; they are not converted into success-shaped
fallbacks. Non-applicability and optional-tool absence are separate structured
results. Process requests use owned `CommandSpec` values, and typed `ExecError`
variants preserve cancellation, timeout, spawn, I/O, and non-zero-exit
failures through task and resource boundaries.

The logger records stages, structured results, actions, warnings, summaries, and
diagnostics. Internal orchestration remains in diagnostic and file logs but does
not appear in normal task rows or totals.
Engine records are keyed by scheduler identity rather than display name, so
dynamic tasks with the same label retain separate status, detail, and duration
records. Command success policy consumes the scheduler's `ExecutionSummary`;
logger counters are presentation data only.

Visible rows use `✓`, `~`, `⊘`, and `✗`, plus the verbose-only `○` and `⁃`.
`--no-symbols` uses ASCII words instead. A task's reason follows a `·`
separator. Indented lines are actions the task took or planned.

Normal output includes only tasks that changed state or need attention, with no
detail truncation. Verbose output includes every task, elapsed time for tasks
that ran, and each resource decision behind the result. Standard summaries
report changed or would-change tasks, then current, ignored, and failed tasks as
applicable. Test summaries report passed, ignored, and failed tasks. Both omit
status glyphs and finish with elapsed time. The progress line and summary count
the same tasks. When a task proves non-applicable, it leaves the progress
denominator.
`dotfiles log` prints a retained run log for post-run investigation. Each run
writes its own file in a platform state directory and the newest 50 are kept, so
a failed run stays readable after later runs. `dotfiles log --list` enumerates
them and an index selects one. Without `--verbose` the command hides `debug`
events so its output matches what the console showed during the run.

## Extending the system

Contributor checklists for adding declarative state, tasks, workflows, and
platform-specific behavior live in [Contributing](CONTRIBUTING.md). Private
behavior extends public desired state through overlay configuration;
convention-based overlay scripts become dynamic tasks without adding private
repositories to the public catalog.
