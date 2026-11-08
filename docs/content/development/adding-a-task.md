---
title: Adding a task
description: Take a desired-state feature from domain ownership and configuration through task registration, dependencies, and regression coverage.
---

# Adding a task

A task is a selectable kind of work, not necessarily one command or one file.
Add one when readers need to select, diagnose, or schedule a distinct part of
machine setup. Adding a package, application preference, or service to an
existing declaration usually does not need a new task.

This guide follows the existing File permissions task as a small example.
Use [Architecture](architecture.md) for engine contracts,
[Writing tests](writing-tests.md) for fixtures, and
[Contributing](contributing.md#development-setup) for the build environment.

## Choose the boundary

Write down what the feature owns before choosing its Rust types:

| Question | Decision to record |
|---|---|
| What is the desired state? | The file, setting, package, or workflow the feature manages |
| Who selects it? | Platform, profile categories, and whether an empty declaration means no work |
| What already exists? | Correct state, repairable differences, protected user content, and inspection failures |
| What can a preview know? | Read-only observations and prerequisites that only apply will create |
| What survives removal? | Whether the feature can safely remove owned state or only apply changes |

Use a `Resource` for independently inspectable items. Use an `Operation` when
a multi-step workflow must plan and report as one unit. The task owns selection,
applicability, dependencies, and privilege assessment around either model.
Do not add another engine abstraction just because a domain needs a new feature.

For File permissions, each declared path becomes a resource. The task chooses
Unix-capable platforms and waits for Home symlinks. The resource owns mode
inspection and application. It has no removal promise because the original
permissions are not recorded.

### Follow one declaration through the task

This excerpt from
[conf/chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml)
omits the other permission entry:

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
]
```

It requests owner-only read/write access to the existing `~/.ssh/config`.
This is not a new table to paste into the file: extend its existing
`permissions` array when adding an entry. File creation belongs to another
task or the user, not File permissions.

Follow the same entry across these boundaries:

| Step | Connected implementation |
|---|---|
| Decode and select | The [chmod decoder](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/config/chmod.rs) reads `permissions` into `ChmodEntry` values; the shared loader selects active categories |
| Share the input | The [configuration inventory](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/config/mod.rs) exposes the selected entries as `store.chmod` |
| Construct the task | The update catalog adds `Box::new(ApplyFilePermissions::new(store.chmod.clone()))`; the remove catalog does not include it |
| Schedule it | The task metadata declares selector `file-permissions` and a success dependency on `InstallSymlinks`; `should_run` requires chmod-capable runtime platforms |
| Inspect and apply | Each entry becomes `ChmodResource::from_entry(entry, ctx.home())`; the adapter supplies managed source roots and explicit descendant exclusions before calling the shared lifecycle |

These are exact excerpts from the task adapter, with the surrounding
implementation omitted:

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

The dependency ensures link work succeeds first. The processing policy repairs
an existing target but leaves a missing one unmet; it never creates the file.
Keep the source-root and descendant handling when adapting this example,
because they protect linked sources and overlapping permission declarations.

From `cli/`, inspect the declaration's selected update graph without applying
permissions:

```sh
cargo run --locked --profile ci -- list --root .. --profile desktop --graph update --only file-permissions --with-deps
```

Expect `file-permissions` and its `symlinks` prerequisite in the graph.
The graph can also include transitive predecessors, and does not probe runtime
chmod support or inspect the target file. Native permission changes still need
an isolated Unix fixture.

## Add configuration only where needed

Keep fixed integration behavior in the implementation. Config-backed state
belongs in the owning domain's typed configuration and a declaration under
[conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/).
Do not put application preferences in Rust when a managed source file already
expresses them.

For a new configuration section:

1. Define and test its decoder, path rules, duplicate handling, and semantic
   validation in the domain's configuration module.
2. Register it with `config_section_inventory!` in
   [app/config](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/config/).
   The inventory supplies the configuration field, typed store handle, summary
   count, and empty unit fixture.
3. Wire special loading and validation in `Config::load` and
   `Config::validate`. Preserve source origins, category selection, and overlay
   precedence instead of reimplementing them in a task.
4. Add the required main file, even if it is empty on some platforms, and
   update isolated configuration fixtures where needed.

Read [Configuration basics](../guides/configuration-basics.md) before choosing a merge
key. An overlay replacing one declaration does not authorize hiding conflicts
within that source. Structural decoding also covers inactive categories.
The [configuration-drift tests](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/config_drift.rs)
check the relationship between real declarations and the application.

## Implement inspection and application

Start with the closest resource and the
[resource contracts](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/resource/contract.rs).
File permissions uses
[ChmodResource](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/resources/chmod.rs);
its task adapter is
[ApplyFilePermissions](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/chmod.rs).

Keep constructors and discovery read-only. Distinguish missing state from an
inspection error: a failed query must not trigger speculative replacement or
installation. Return a typed resource error when discovery fails.

Let the shared lifecycle decide whether to preview, apply, skip, or report
already-correct state. Reuse `run_resource_task`, `run_batch_resource_task`,
or the `process_resources` entry points as appropriate. Choose `ProcessOpts`
for the intended policy, not just for a convenient verb. For example, permissions
uses `fix_existing` because it must not create missing files.

Use the injected executor and environment. Declare access claims when resources
edit the same document or exclusive state. Serializing one batch does not stop
another task from modifying that state concurrently. Batch discovery must not
read mutable claimed state before its reservation is held.

Implement `RemovableResource` only when ownership and removal behavior are
defined. Do not provide a stub that fails at runtime or assume every update
needs a matching remove task.

## Define task metadata and readiness

Choose a stable public selector and a readable console name. Static tasks use
their type identity; multiple dynamic instances also need a stable instance key.
Use `task_metadata!` and, for a task holding one configuration handle,
`config_task!` rather than repeating their boilerplate.

Applicability answers whether the task belongs in this run. A runtime
requirement answers whether its needed inputs are available. Keep inputs that
a predecessor may create inside `run()`, after the dependency wait. A missing
tool is unmet work, not proof the task is irrelevant.

Use runtime platform capabilities for decisions, with compile-time gates only
where platform APIs require them. Privilege assessment must not mutate the host
or prompt independently of the application's elevation policy.

## Wire command membership and dependencies

Register static update/remove tasks in
[app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs).
Validation tasks instead belong to the
[check command's task list](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/commands/check.rs).
Do not add a task to remove unless it promises a supported removal action.

Same-domain dependencies belong with the task. Cross-domain relationships
belong in the application catalog, not imports between sibling domains.
List position is not execution order.

| Relationship | Use it when |
|---|---|
| Requires success | The task cannot safely proceed if the predecessor fails or remains unmet |
| Runs after | The task should wait, then inspect its own state despite unrelated predecessor failures |
| Conditional ordering wait | Readiness evidence can safely release an ordering-only wait early |

File permissions has a same-domain success dependency on Home symlinks.
Systemd's waits for package and file work are app-owned ordering dependencies.
Keep those distinctions in tests and the task's reference table. Removal has
its own graph; it does not inherit update ordering.

Test selection with and without `--with-deps`, skipped predecessors, unavailable
tasks, and the relevant command membership. Use the existing
[catalog tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs)
and [task execution tests](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/task_execution.rs)
as starting points.

## Finish the reader-facing contract

Add a page following the
[task reference conventions](writing-documentation.md#task-reference-pages).
Explain the domain before its implementation details, show a valid declaration,
and state preview and removal limits. Add the selector to
[Task reference](../reference/task-reference.md) and register the page in the site navigation.

Cover the resource lifecycle, task wiring, and observable failures at their
owning layers. Include preservation of unrelated state, repeat application,
and non-mutating previews. [Writing tests](writing-tests.md) explains how to
build those cases without using the developer's installation.

Select focused and cross-platform coverage from
[Testing](testing.md#choosing-coverage). Before pushing, validate the committed
patch and run focused mutation testing for changed Rust behavior. A working
unit test does not prove selection, elevation, or native platform behavior.
