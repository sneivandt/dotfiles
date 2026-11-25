---
title: Adding a task
description: Take a desired-state feature from domain ownership and configuration through task registration, dependencies, and regression coverage.
---

# Adding a task

A task is a selectable unit of work (not necessarily a single command or file).
Introduce a new task when users need to select, inspect, or schedule a distinct
area of machine setup; adding another package, preference, or service to an
existing TOML file rarely needs a new task.

This walkthrough traces the existing File permissions task end to end as a
concrete template. Keep [Architecture](architecture.md) open for engine
contracts, [Writing tests](writing-tests.md) for fixture design, and
[Development setup](contributing.md#development-setup) for building locally.

## Choose the boundary

Before writing Rust types, spell out what the feature owns:

| Question | Decision to record |
|---|---|
| What is the desired state? | The file, setting, package, or workflow the feature manages |
| Who selects it? | Platform, profile categories, and whether an empty declaration means no work |
| What already exists? | Correct state, repairable differences, protected user content, and inspection failures |
| What can a preview know? | Read-only observations and prerequisites that only apply will create |
| What survives removal? | Whether the feature can safely remove owned state or only apply changes |

Pick a `Resource` when items can be inspected and converged independently, or
an `Operation` when a multi-step workflow must plan and report as one unit.
The task itself owns eligibility, dependencies, and privilege assessment around
either model; avoid inventing a new engine abstraction for a single domain.

In the File permissions example, each declared path is an independent resource.
The task runs on Unix-capable platforms after Home symlinks, the resource owns
mode inspection and updates, and removal is unsupported because original file
modes are not recorded.

### Follow one declaration through the task

This entry in
[conf/chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml)
sets the default Linux permission rule:

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
]
```

It ensures `~/.ssh/config` has owner-only read/write permissions once the file
exists (extend the existing `permissions` array rather than repeating `[linux]`).
Creating the file itself belongs to Home symlinks or the user, not File
permissions.

Here is how that single entry flows through the codebase:

| Step | Connected implementation |
|---|---|
| Decode and select | The [chmod decoder](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/config/chmod.rs) parses `permissions` into `ChmodEntry` values while the shared loader filters active categories |
| Share the input | The [configuration inventory](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/config/mod.rs) exposes the selected entries as `store.chmod` |
| Construct the task | The update catalog registers `ApplyFilePermissions` with `store.chmod`; the remove catalog omits it |
| Schedule it | Task metadata sets selector `file-permissions` and a blocking dependency on `InstallSymlinks`; `should_run` checks for chmod-capable platforms |
| Inspect and apply | Each entry builds a `ChmodResource` via `from_entry` against `ctx.home()`; the adapter passes managed source roots and descendant exclusions into the shared lifecycle |

Here are the core metadata and lifecycle calls from the task adapter (with
surrounding setup omitted):

```rust
task_metadata! {
    name: NAME,
    selector: "file-permissions",
    deps: [crate::domains::files::symlinks::InstallSymlinks],
}
```

```rust
run_resource_task(ctx, resources, &ProcessOpts::fix_existing("configure"))
```

The dependency ensures symlinks succeed first, and `fix_existing` repairs
existing files while leaving missing ones unmet rather than creating them.
Preserve the source-root and descendant-exclusion logic if you adapt this
pattern, since it protects linked checkout files and overlapping rules.

From `cli/`, inspect the task's update graph without changing any permissions:

```sh
cargo run --locked --profile ci -- list --root .. --profile desktop --graph update --only file-permissions --with-deps
```

You'll see `file-permissions` along with `symlinks` and any transitive
predecessors. Remember that `list --graph` only inspects metadata and doesn't
probe runtime chmod support or files on disk; verify native permission behavior
with an isolated Unix test fixture.

## Add configuration only where needed

Keep fixed integration logic in Rust and put user-editable declarations in the
owning domain's TOML file under
[conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/). Don't hardcode
application preferences in Rust when a managed file under `symlinks/` already
handles them.

When adding a new configuration section:

1. Implement and test its decoder, path checks, duplicate rules, and semantic
   validation in the domain's config module.
2. Register it via `config_section_inventory!` in
   [app/config](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/config/)
   so the inventory generates the config field, typed store handle, summary
   count, and empty unit fixture automatically.
3. Wire any custom loading or validation into `Config::load` and
   `Config::validate`, preserving source origins, category selection, and
   overlay precedence instead of reimplementing them inside a task.
4. Add the required main TOML file (even if empty on some platforms) and update
   isolated config fixtures as needed.

Check [Configuration basics](../guides/configuration-basics.md) before picking
a merge key. Structural parsing covers inactive categories, and an overlay
can't mask conflicts inside the base repository. The
[configuration-drift tests](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/config_drift.rs)
verify that real repository declarations stay aligned with the CLI.

## Implement inspection and application

Use the closest existing resource alongside the
[resource contracts](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/resource/contract.rs)
as a reference. File permissions pairs
[ChmodResource](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/resources/chmod.rs)
with the thin adapter
[ApplyFilePermissions](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/chmod.rs).

Keep constructors and discovery strictly read-only. Never collapse a failed
inspection query into "missing": return a typed resource error so the engine
doesn't speculatively overwrite or reinstall state.

Delegate previewing, applying, skipping, and no-op detection to
`run_resource_task`, `run_batch_resource_task`, or `process_resources`. Pick
`ProcessOpts` for its actual policy rather than just a verb (File permissions
uses `fix_existing` specifically so missing files are never created).

Route subprocesses and environment lookups through the injected executor and
environment, and declare `AccessClaim` entries whenever concurrent tasks or
resources might edit the same file or exclusive resource. Serializing one batch
won't stop another task from modifying that file concurrently, and batch
discovery must never read mutable claimed state before its reservation is held.

Implement `RemovableResource` only when ownership and removal semantics are
well-defined; don't add stubs that fail at runtime or assume every update task
needs a remove counterpart.

## Define task metadata and readiness

Pick a stable public selector and a clear console label. Static tasks use their
type identity, while dynamic tasks also supply a stable instance key. Use
`task_metadata!` (and `config_task!` for single-handle tasks) to avoid
repeating boilerplate.

Keep applicability and runtime requirements cleanly separated: applicability
decides whether the task belongs in this run, whereas a requirement checks
whether a needed input is present. Put checks for inputs that a predecessor
might create inside `run()`, after dependency waits finish, so a missing tool
reports unmet work rather than silently marking the task inapplicable.

Use runtime platform capabilities for OS branching, keeping compile-time `cfg`
gates only where platform APIs demand them. Privilege assessment must stay
read-only and never prompt independently of the application's elevation broker.

## Wire command membership and dependencies

Register static `update` and `remove` tasks in
[app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs),
and validation tasks in the
[check command's task list](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/commands/check.rs).
Only add a task to the `remove` catalog if it promises a supported removal
action.

Declare same-domain dependencies on the task itself and cross-domain
dependencies in `app/catalog.rs` (never import a sibling domain directly).
Catalog order does not control execution order:

| Relationship | Use it when |
|---|---|
| Requires success | The task cannot safely run if its predecessor fails or stays unmet |
| Runs after | The task should wait, then inspect its own state even if the predecessor failed |
| Conditional ordering wait | Readiness evidence can safely waive an ordering-only wait early |

For example, File permissions declares a same-domain blocking dependency on
Home symlinks, while Systemd's waits on packages and files are app-owned
ordering waits. Keep those distinctions consistent across tests and task
reference tables, and remember that `remove` defines its own graph rather than
inheriting `update` edges.

Test task selection with and without `--with-deps`, skipped predecessors,
inapplicable tasks, and command membership using the existing
[catalog tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs)
and [task execution tests](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/integration/task_execution.rs).

## Finish the reader-facing contract

Add a reference page following the
[task reference conventions](writing-documentation.md#task-reference-pages):
open by explaining the domain before implementation details, include a valid
declaration example, and document preview and removal boundaries. Add the
selector to [Task reference](../reference/task-reference.md) and register the
page in the VitePress sidebar config.

Cover resource lifecycle, task wiring, and observable failures at their owning
layers, including preservation of unrelated state, repeat runs, and
non-mutating previews. See [Writing tests](writing-tests.md) to construct
isolated fixtures safely.

Select targeted and cross-platform checks from
[Choosing coverage](testing.md#choosing-coverage), and consider
[focused mutation testing](testing.md#focused-mutation-testing) before pushing
when safety or failure-propagation assertions need extra confidence. If
something behaves unexpectedly while wiring your task, move on to
[Debugging](debugging.md).
