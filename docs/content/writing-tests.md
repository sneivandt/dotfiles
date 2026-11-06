---
title: Writing tests
description: Build isolated Rust regression tests with the existing context, filesystem, process, and command fixtures.
---

# Writing tests

Write a regression that fails for the behavior you want to prevent, then assert
the outcome and the state left behind. A task returning success is not enough:
it might have overwritten unrelated content, hidden a failed query, or changed
files during preview.

This guide covers designing regressions: choosing a boundary, constructing
fixtures, and proving an observable outcome. [Testing](testing.md) owns commands,
coverage selection, script/application checks, and pre-push verification.
Use [Adding a task](adding-a-task.md) when a change also needs configuration or
catalog wiring.

## Choose the smallest useful layer

| Behavior | Test location |
|---|---|
| Parsing, normalization, or one resource decision | Unit tests beside the owning module |
| Task applicability, prerequisites, and batch accounting | The domain task's unit suite |
| Dependency scheduling and shared lifecycle semantics | Engine suites and `task_execution` |
| Command selection, restart, exit status, or output contract | The corresponding integration target in `cli/tests/` |
| Real declarations agreeing with supported behavior | `config_drift` |
| A materially different apply/remove journey | Isolated lifecycle or end-to-end integration cases |

Keep small cohesive unit suites inline. Large sibling suites can live in
`tests.rs`; domain-root task suites can use `domains/<domain>/tests/<feature>.rs`
through a test-only `#[path]`. Do not create a feature directory solely to hold
tests.

Integration tests import the intentionally exposed
[testing API](https://github.com/sneivandt/dotfiles/blob/main/cli/src/testing/mod.rs).
Avoid exposing unrelated production internals merely to assert an implementation
detail. Prefer the public behavior or a focused unit test.

### Integration test suites

Rust integration targets under
[cli/tests/](https://github.com/sneivandt/dotfiles/tree/main/cli/tests/) use
controlled fixtures. They are different from CI's host-mutating installation
jobs.

| Target | Main responsibility |
|---|---|
| `config_drift` | Real configuration, source, and category invariants |
| `domain_boundaries` | Architectural dependencies and platform/environment boundaries |
| `update_command`, `remove_command`, `list_command`, `check_command` | Command membership, selection, loading, and outcomes |
| `task_execution` | Task/resource convergence and dry-run |
| `task_output` | Statuses, reasons, logs, and exit behavior |
| `lifecycle_contracts` | Preview, apply/repeat, retry, and conservative removal across real tasks |
| `e2e_apply` | CLI convergence against controlled state |
| `behavioral_ci` | Cross-cutting regressions for CI assumptions |

Use the command suite to prove that a task is selected and its failure affects
the exit status. Keep the task's individual resource branches in its unit suite.
Repeating every branch at the command boundary makes failures harder to locate
without proving a different contract.

## Confine every writable path

Use temporary repositories, homes, and state directories. A temporary repository
alone is insufficient if the command still discovers the real home, Git
configuration, overlay, or log directory.

The shared
[integration helpers](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/mod.rs)
provide minimal declarations and isolated CLI commands. `cli_command` sets
fixture home/config/cache/log paths, disables binary self-update, and normally
skips repository synchronization. It also initializes a fixture Git repository
so discovery cannot fall through to the developer's checkout. Use the explicit
repository-sync variant only for tests that need it.

For unit tests, set both the repository root and home. Inject a `MapEnv` rather
than changing the process environment. The
[unit helpers](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/test_helpers/mod.rs)
offer `empty_config`, platform contexts, a context builder, and result assertions.
Check their defaults before reusing one: an injected Linux platform does not
turn the host filesystem into Linux.

Never use a real package manager, registry, service manager, elevation prompt,
private overlay, or installed agent deployment to make a unit test pass.
Use synthetic public values in fixtures and captured output.

## Model subprocesses precisely

Use the injected executor so tests control executable discovery and process
results independently.

| Test double | Best use |
|---|---|
| `MockExecutor` | Branching behavior, call counts, readiness probes, or unordered execution |
| `ScriptedExecutor` | A fixed, ordered command sequence |
| `FailingExecutor` / `FailingResource` | Failure injection at a chosen lifecycle point |
| `CapturingOutput` | Inspecting emitted events and visible diagnostics |

These helpers live in
[app/test_helpers](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/test_helpers/).
`ScriptedExecutor::command` checks program, arguments, working directory, and
checked mode. Its `git` helper expects a checked Git command.
The `ok` and `err` helpers supply results without matching the command, so they
cannot prove that the correct command was issued.

For a command whose failure should propagate, model the executor's typed
`ExecError`. Use `ExecResult::failure` for an explicitly unchecked exit protocol,
where production code interprets the exit status. Test spawn, timeout, or
cancellation errors separately when the feature handles them differently.
An always-successful mock can conceal both unwanted commands and missing checks.

## Exercise a complete state transition

For a state-changing feature, choose the relevant cases below. Not every
resource repairs incorrect state: some deliberately preserve it as a conflict.

| Starting state | Assertion |
|---|---|
| Missing | The intended content is created, or missing state is visibly unmet if creation is unsupported |
| Already correct | No mutation occurs and the result records current state |
| Different but repairable | Only the owned setting changes |
| Foreign or unsafe | User state is preserved and the conflict is reported |
| Inspection failed | Failure is visible; no speculative installation or replacement follows |
| Preview | The planned outcome is reported without creating or changing managed state |
| Repeated apply | A second application performs no unnecessary mutation |
| Partial failure | Completed work remains accounted for and the command still fails |

The
[Shell PATH tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/path.rs)
show a compact preview/apply/repeat sequence across missing, existing, and
already-managed profile content. They compare the file bytes after preview,
check the managed addition after apply, and assert zero changes on repeat.
Use that pattern rather than checking only `is_ok()`.

### A complete isolated task regression

This example can be saved as `cli/tests/hook_regression.rs`. It uses the
existing `common` integration helpers and the public testing API, not an
installed CLI or a real home. The hook is inert fixture content; installing it
does not execute it or create commits.

```rust
//! Isolated preview, apply, and repeat coverage for managed hooks.
#![allow(clippy::unwrap_used, reason = "fixture failures abort the regression")]

mod common;

use dotfiles_cli::testing::tasks::git::hooks::InstallGitHooks;
use dotfiles_cli::testing::tasks::{Task, TaskResult};

#[test]
fn hook_preview_apply_and_repeat_stay_in_the_fixture() {
    let content = "#!/bin/sh\nexit 0\n";
    let repo = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", content)
        .with_git_hooks_dir()
        .build();
    let fixture = repo.make_context("base");
    let target = repo.root_path().join(".git").join("hooks").join("pre-commit");
    let ownership = target.with_extension("dotfiles-owned");
    let unrelated = target.with_file_name("user-note.txt");
    std::fs::write(&unrelated, "keep me").unwrap();
    let task = InstallGitHooks::new();

    let preview = task.run(&fixture.ctx.with_dry_run(true)).unwrap();
    assert!(matches!(preview, TaskResult::Batch(stats)
        if stats.changed_count() == 1));
    assert!(!target.exists());
    assert!(!ownership.exists());

    let applied = task.run(&fixture.ctx).unwrap();
    assert!(matches!(applied, TaskResult::Batch(stats)
        if stats.changed_count() == 1));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), content);
    let recorded_owner = std::fs::read(&ownership).unwrap();

    let repeated = task.run(&fixture.ctx).unwrap();
    assert!(matches!(repeated, TaskResult::Batch(stats)
        if stats.changed_count() == 0 && stats.already_ok_count() == 1));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), content);
    assert_eq!(std::fs::read(&ownership).unwrap(), recorded_owner);
    assert_eq!(std::fs::read_to_string(&unrelated).unwrap(), "keep me");
}
```

The builder creates minimal declarations and a repository-local hooks path.
`make_context` supplies an isolated home, `MapEnv`, fixture log directory,
and an executor that rejects subprocesses. The helpers own the temporary
directories and remove them when dropped.

Run only this example from `cli/`:

```sh
cargo test --locked --profile ci --test hook_regression
```

Expect one passing test on either native platform. The assertions check the
planned count, absence of preview writes, installed bytes, ownership record,
unchanged user content, and an already-current repeat. They do not prove hook
execution or command-level selection. Keep the case in the owning existing
suite when incorporating it into a real change, rather than adding a target
for every regression.

Use named cases for closely related inputs, but separate tests when platform
contracts differ. The shared
[lifecycle fixtures](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/lifecycle.rs)
exercise symlinks, Git hooks, and Git settings across fresh contexts.

For removal, add cases for a matching managed target, a user-modified target,
and a missing target. Assert what is preserved as carefully as what is removed.
For paths and permissions, include resolved destinations and parent links where
those affect ownership or containment.

### Dry-run testing

A preview needs both a positive assertion about the reported plan and a
negative assertion about side effects. Capture the fixture's starting files
and subprocess calls, run with dry-run enabled, then compare them. A "dry run"
message alone does not prove that no mutation occurred.

Use `common::cli_command` for subprocess cases so the fixture, not your real
home, receives logs or selection metadata. Cover removal previews separately
where supported. Missing targets should remain absent; existing bytes,
links, permissions, and unrelated keys should remain unchanged as appropriate.

Overlay scripts execute their own check/preview code. Use fake scripts that
record protocol modes and assert that apply/remove modes were not invoked.
Never use a live overlay as a test fixture; see
[Private overlays](security-and-trust.md#private-overlays).

## Test scheduling without timing guesses

Assert required ordering and failure propagation, not incidental completion
order. Use synchronization primitives rather than sleeps to coordinate
concurrent tests. For graph-only fixtures, `test_task!` creates distinct no-op
task types with selectors and dependency edges; use handwritten tasks when
execution or dynamic identity is the behavior under test.

Cover ordering-only predecessors failing without blocking independent work,
success dependencies blocking when needed, and unavailable tasks removed during
selection. Access claims need evidence that conflicting state is excluded while
independent work can proceed. Serial execution alone cannot prove that contract.

Command-level cases should verify the selected task set and exit policy, not
repeat every resource branch. The
[task execution suite](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/task_execution.rs)
and [command suites](https://github.com/sneivandt/dotfiles/tree/main/cli/tests/)
provide examples.

Shared sequential/parallel contracts live in
[scheduler/conformance.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/scheduler/conformance.rs).
Use the adjacent
[batch reports](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/batch_reports.rs)
and [parallel scheduling](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/parallel.rs)
suites for partial failure, cancellation, and in-flight accounting.

## Review failures and coverage honestly

When a snapshot changes, read the diff before accepting it. Keep snapshots
focused on a meaningful output contract; do not regenerate unrelated output
to hide a failure.

For a mutation survivor, identify what the caller would observe if that
mutation were real, then assert that outcome or preserved state. A timeout is
not a caught mutant. Rerun the original source range even when the fix changes
only tests; [Focused mutation testing](testing.md#focused-mutation-testing)
explains selection and disposable runs.

Finally, distinguish simulated platform decisions from native runtime coverage.
Windows compilation cannot prove registry, process-launch, or symlink behavior.
Unix permission and filename cases need a filesystem with those semantics.
Select the required native checks from
[Platform coverage parity](testing.md#platform-coverage-parity) and record
unavailable coverage instead of treating a skip as a pass.
