# Architecture

The project separates bootstrap, application orchestration, domain behavior,
infrastructure adapters, and declarative desired state.

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
| `cli\src\app\` | CLI definitions, command composition, catalog, aggregate config, validation |
| `cli\src\engine\` | Task scheduling, resource convergence, operations, logging contracts |
| `cli\src\domains\` | Git, packages, files, system, AI, editor, repository, shell, and overlay behavior |
| `cli\src\infra\` | Platform detection and concrete system adapters |
| `conf\` | Declarative desired state |
| `symlinks\` | Versioned files linked into the user's home directory |
| `hooks\` | Repository-maintained Git hooks and checks |
| `.github\workflows\` | CI, release, and container publishing |

## Wrappers

The wrappers are intentionally thin. They:

1. Determine the repository root and target binary.
2. Consume wrapper-only `--build`.
3. Build from source or download a release asset when needed.
4. Verify downloaded content.
5. Export bootstrap context.
6. Execute the Rust CLI with all remaining arguments unchanged.

Command semantics must not be added to the wrappers. Keeping one behavioral
implementation avoids Linux/Windows drift.

## Application layer

The application layer owns composition:

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

- a human-readable name
- command membership such as update-only behavior
- a stable identity
- dependency identities
- an applicability guard
- optional elevation planning
- execution returning a structured task result

The scheduler validates a dependency graph and runs ready tasks in parallel.
Every ordering requirement is an explicit dependency edge; the order of entries
in `catalog.rs` is not execution order. Failed prerequisites block dependents,
and duplicate identities or cycles fail before execution.

`Task::update_only()` is command membership metadata, not an ordering class.
`install` excludes update-only tasks, while `update` includes them in the same
graph as ordinary install tasks.

Dynamic overlay tasks use per-instance hashed identities because multiple
configured scripts share one Rust task type.

## Resources

A `Resource` models independently convergent desired state:

1. Discover current intrinsic state.
2. Compare it with desired state.
3. Produce a change plan.
4. Preview or apply that change.

Resources are used for packages, symlinks, registry values, permissions, and
similar state. Providers can batch or cache state discovery, reducing repeated
system calls.

Resource processing is dry-run aware and returns explicit outcomes such as
applied, already correct, skipped, invalid, or unknown. Tasks translate those
outcomes into user-facing summaries.

## Operations

An `Operation` models a whole workflow that converges as a unit rather than a
collection of independent records. It has current-state, preview, and apply
steps. Configuration reload and convention-based overlay scripts use this model
because their correctness depends on completing a coherent workflow.

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
that supported overlay sections are merged consistently. The sparse-checkout
manifest is the exception: it describes the main repository and is not merged.

Shared configuration handles enable mid-run reload. Dynamic overlay tasks make
the active task set configuration-dependent, so `ReloadConfig` is a discovery
boundary:

1. Run the dependency closure ending at `ReloadConfig`.
2. Stop if it fails or execution is cancelled.
3. Rebuild dynamic tasks from the refreshed handles.
4. Run remaining static and dynamic tasks in one dependency graph.

If the boundary is filtered out, dynamic tasks are built from current
configuration before one graph is run. This split controls discovery only;
normal ordering still comes from dependency edges.

## Platform abstraction

Tasks prefer capability methods exposed by context and system adapters rather
than scattering direct operating-system checks. Platform guards still determine
applicability, but concrete mutations are delegated to the relevant adapter or
provider.

This supports:

- Linux and Windows implementations behind common contracts
- test doubles for filesystem and command execution
- explicit capability failures instead of silent platform assumptions
- elevation planning before parallel task dispatch

## Error handling and observability

Errors propagate with context; they are not converted into success-shaped
fallbacks. Non-applicability and optional-tool absence are separate structured
results.

The logger records stages, actions, warnings, summaries, and diagnostic detail.
`dotfiles log --verbose` prefers the diagnostic log for post-run investigation.

## Extension points

### Add declarative state

For independent config-backed state, the normal vertical slice is:

```text
typed config -> loader -> validation -> conf file -> resource/provider
-> task -> catalog registration -> tests
```

### Add a workflow

For whole-workflow convergence:

```text
operation -> task metadata/dependencies -> command registration -> tests
```

### Add private behavior

Use overlay configuration. Conventional private scripts become dynamic tasks
without teaching the public catalog about private repositories.

See [Contributing](CONTRIBUTING.md) and [Task reference](TASKS.md).
