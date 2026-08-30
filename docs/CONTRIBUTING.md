# Contributing

## Development setup

Install:

- Git
- the Rust toolchain declared by the repository
- PowerShell 7 on Windows
- ShellCheck and PSScriptAnalyzer for script checks

Build the CLI from the repository root:

```bash
cargo build --manifest-path cli/Cargo.toml
```

See [Usage](USAGE.md) for wrapper and command examples. See
[Testing](TESTING.md) for all validation commands, optional tools, focused test
selection, and CI coverage.

## Contribution workflow

1. Read [Architecture](ARCHITECTURE.md) and identify the owning layer.
2. Find the closest existing implementation and reuse its contracts and helpers.
3. Preserve user-owned working-tree changes and private overlay content.
4. Make the smallest complete change, including applicable configuration,
   wiring, tests, and user documentation.
5. Use [Choosing coverage](TESTING.md#choosing-coverage) to select focused
   checks, then widen coverage when shared behavior changed.

Keep secrets, private files, and unreviewed generated artifacts out of commits.

## Change checklists

### Config-backed state

Update every participating layer: typed configuration, loading and semantic
validation, real `conf/` data, resource or operation behavior, task wiring,
exports, registration, and tests. Keep conditional symlink changes aligned with
the profile categories that select them.

### Tasks and workflows

For a task, define stable identity and selector metadata, command membership,
applicability, dependencies, elevation policy, and focused command coverage.
Cross-domain dependency edges belong in the application catalog; catalog order
is not execution order.

Use an operation when current state, preview, and apply must converge as one
coherent workflow rather than as independent resource records.

### Platform-specific changes

Keep platform imports, types, and calls behind the appropriate boundary so the
other platform still compiles. Use capabilities and adapters for platform
behavior, and consult the [platform coverage matrix](TESTING.md#platform-coverage-parity)
for runtime gaps.

## Documentation

Update the guide closest to the behavior. If a task is added, removed, renamed,
rewired, or changes command membership, update [Task reference](TASKS.md).
Keep the root README as a landing page and detailed guidance in `docs/`.

The [documentation source-of-truth boundaries](./README.md#source-of-truth-boundaries)
define where repository guidance belongs. Documentation consistency checks,
including link and task-selector validation, are documented in
[Testing](TESTING.md).
