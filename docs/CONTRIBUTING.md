# Contributing

This guide is the human change workflow. Use [Architecture](ARCHITECTURE.md)
to understand runtime contracts and [Testing](TESTING.md) to select exact checks.
You do not need to learn the whole engine to change one application's config.

## Development setup

Work in a checkout with Git and Rust available. Rust commands should run from
`cli/`, where [`rust-toolchain.toml`](../cli/rust-toolchain.toml) selects the
pinned compiler, rustfmt, Clippy and cross-target components.
[`Cargo.toml`](../cli/Cargo.toml) declares the separately tested minimum Rust
version and lint policy.

```bash
cd cli
cargo build --profile ci
cargo run --profile ci -- --help
```

This builds/runs the current source without asking a bootstrap wrapper to
download a release or reuse a different binary. Cargo/rustup may need network
access for missing dependencies or toolchain components.

Install optional tools only for the checks you need: ShellCheck for POSIX
scripts, PowerShell 7 with PSScriptAnalyzer for PowerShell, and the Cargo
audit/deny tools for dependency checks. Tool availability and skips are
documented in [Testing](TESTING.md#fast-local-sequence).

## Contribution workflow

### 1. Establish the scope and safety boundary

Inspect your branch and both staged and unstaged changes before editing:

```bash
git status --short --branch
git diff --stat
git diff --cached --stat
```

Preserve unrelated changes. This checkout may be the live source of home
symlinks: editing `symlinks/` can immediately affect an auto-reloading
application. Edit tracked sources, not installed copies, and do not restart
services or deploy the change merely to validate it.

Use fixtures for mutation tests. A real update/remove can modify your home,
Git settings, packages, registry or system files; it is not the default test
procedure. Do not use private overlay content, credentials or raw machine logs
as public examples or regression fixtures.

### 2. Find the owning files

| Change | Start here | Also check |
|---|---|---|
| Application settings/content | The application under [`symlinks/`](../symlinks) | Existing link/category selection; app-specific tests |
| Package or declarative state | The matching file under [`conf/`](../conf) | [Configuration](CONFIGURATION.md), [Profiles](PROFILES.md), `config_drift` |
| Home-link placement | [`conf/symlinks.toml`](../conf/symlinks.toml) | Source layout and profile categories |
| Privileged file fragment | [`system/`](../system) and [`conf/system-files.toml`](../conf/system-files.toml) | Merge format, path confinement/validation and resource fixtures |
| CLI flags or selection | [`app/cli.rs`](../cli/src/app/cli.rs), [`app/commands/`](../cli/src/app/commands) | Command tests, completion behavior and [Usage](USAGE.md) |
| Task composition or cross-domain dependencies | [`app/catalog.rs`](../cli/src/app/catalog.rs) | Graph/command tests and [Task reference](TASKS.md) |
| Domain convergence | Owning module under [`domains/`](../cli/src/domains) | Resource/operation choice and neighboring tests |
| Process, filesystem or output mechanics | [`infra/`](../cli/src/infra) | Typed errors, both platforms, isolation and log privacy |
| Hooks or CI | [`hooks/`](../hooks), [workflows](../.github/workflows) | [Hooks](HOOKS.md), [CI gates](TESTING.md#ci-gates) |

Find the closest existing implementation before adding abstractions. Repository
skills under [`.agents/skills/`](../.agents/skills) provide narrow subsystem
procedures; they complement these guides rather than replace the source.

### 3. Make a complete, bounded change

Keep data changes declarative. For behavior changes, include the participating
configuration, execution path, wiring, regression tests and user-facing
documentation. Avoid unrelated cleanup that makes the behavioral change harder
to review.

Test the failure you are fixing, not just the happy path. For state-changing
behavior, show what happens on repeat execution, dry-run, missing prerequisites
and partial failure. Include remove only where the feature actually promises
removal; remove is not a universal undo operation.

### 4. Validate and review the exact patch

Use [Choosing coverage](TESTING.md#choosing-coverage) to start with the smallest
relevant suite. Widen for shared engine/configuration changes. Record exact
commands and material skips, especially Windows runtime checks you could not
run. A green command with skipped stages does not prove those stages passed.

Before presenting or committing the change, inspect the diff for private data,
generated files, stale examples and unintended behavior. Stage explicit paths
or hunks, then inspect `git diff --cached`: the [hooks](HOOKS.md) test the index,
which may differ from a partially staged working tree. Do not bypass a failing
hook in place of diagnosing it.

## Change checklists

### Config-backed state

For an existing field or category, change the data and its focused regression;
do not add another execution layer. For a genuinely new section:

1. Add the typed decoder and semantic validation in the owning domain. Test
   invalid input as well as accepted shorthand/forms.
2. Declare the section in `config_section_inventory!` in
   [`app/config`](../cli/src/app/config). It supplies the field, typed
   [`ConfigStore`](../cli/src/app/config/store.rs) handle, summary count and empty
   unit fixture. Set `file` for required main configuration and `decode` for
   ordinary category-filtered loading.
3. Keep special loading, overlay-only behavior, provenance, platform filtering
   and active/unfiltered validation views explicit in `Config::load`.
   Wire semantic validation in `Config::validate`.
4. Implement a resource or operation using the existing lifecycle helpers.
   Keep discovery read-only, apply idempotent and preview non-mutating.
5. Wire the task and module exports, add real `conf/` data, and extend
   configuration/command tests. Keep conditional links aligned with categories.
6. Document the user-facing format in [Configuration](CONFIGURATION.md).

### Tasks and workflows

Define stable selector, display label, scheduler identity and visibility
separately. Decide command membership, applicability, elevation and prerequisites
explicitly. Register static update/remove tasks in the catalog;
command-specific tasks belong in that command's task list.

Use failure-blocking edges only when success is required. Use ordering-only
edges when a task should wait and then independently recheck state. Same-domain
edges stay in the domain; cross-domain edges go in the catalog. List order does
not establish execution order.

Choose a resource for independent items or an operation for one coherent
workflow. Do not duplicate dry-run/result-accounting loops in task code.
Protect selector filtering, `--with-deps`, update membership and discovery when
changing composition. Update [Task reference](TASKS.md).

### Platform-specific changes

Use capabilities and adapters for runtime decisions; gate platform-only imports,
types and calls so the other build still compiles. Use injected environment and
executors in tests rather than changing process-global state or invoking real
package managers/elevation.

Cross-target compilation catches type/import mistakes, not Windows runtime
behavior. Use [Platform coverage parity](TESTING.md#platform-coverage-parity)
for registry, symlink, subprocess, path and elevation coverage.

## Documentation

Put the explanation where a reader will look for it:

- User commands and visible behavior: [Usage](USAGE.md).
- Desired-state schema and examples: [Configuration](CONFIGURATION.md).
- Selection/categories: [Profiles](PROFILES.md); task catalog: [Tasks](TASKS.md).
- Runtime boundaries: [Architecture](ARCHITECTURE.md).
- Exact verification commands and coverage gaps: [Testing](TESTING.md).
- Hook failures: [Hooks](HOOKS.md); trust boundaries: [Security](SECURITY.md).

Link to canonical guidance instead of copying it into several documents. Keep
the root README a landing page. Preserve linked heading anchors, or update
incoming links and skills when a heading must change. Run the documentation
check even when a change only moves files or renames a task.

See [documentation ownership](README.md#source-of-truth-boundaries) for the full
guide boundaries. Documentation checks catch broken links and selector drift;
they do not verify that prose is true. Compare behavioral claims with code and
tests before publishing them.
