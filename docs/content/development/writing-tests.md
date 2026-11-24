---
title: Writing tests
description: Build isolated Rust regression tests with the existing context, filesystem, process, and command fixtures.
---

# Writing tests

Write a regression that fails on the broken behavior you want to prevent, then
verify both the returned result and the state left behind. Checking `is_ok()`
alone isn't enough: a passing task might still have clobbered unrelated files,
swallowed a query error, or mutated disk state during a preview.

This guide covers test placement, isolated fixtures, subprocess mocks, and
state-transition assertions. See [Testing](testing.md) for running existing
suites and choosing pre-push coverage, and [Adding a task](adding-a-task.md)
when wiring new config or catalog entries.

## Choose the smallest useful layer

| Behavior | Test location |
|---|---|
| Parsing, normalization, or one resource decision | Unit tests beside the owning module |
| Task applicability, prerequisites, and batch accounting | The domain task's unit suite |
| Dependency scheduling and shared lifecycle semantics | Engine suites and `task_execution` |
| Command selection, restart, exit status, or output contract | The corresponding integration target in `cli/tests/` |
| Real declarations agreeing with supported behavior | `config_drift` |
| A materially different apply/remove journey | Isolated lifecycle or end-to-end integration cases |

Keep small unit suites inline. Larger sibling suites fit cleanly in `tests.rs`
(or `domains/<domain>/tests/<feature>.rs` via a test-only `#[path]` for
domain-root task suites) without creating single-file feature directories just
for tests.

Integration tests import the curated
[testing API](https://github.com/sneivandt/dotfiles/blob/main/cli/src/testing/mod.rs);
prefer testing observable behavior or adding a unit test rather than widening
public visibility solely for an assertion.

### Integration test suites

Integration binaries under
[cli/tests/](https://github.com/sneivandt/dotfiles/tree/main/cli/tests/) run
against isolated fixtures rather than modifying your host:

| Target | Main responsibility |
|---|---|
| `config_drift` | Real configuration, source, and category invariants |
| `domain_boundaries` | Architectural dependencies and platform/environment boundaries |
| `update_command`, `remove_command`, `list_command`, `check_command` | Command membership, selection, loading, and outcomes |
| `completion_contracts` | Runtime shell completion, parsing parity, and read-only task discovery |
| `task_execution` | Task/resource convergence and dry-run |
| `task_output` | Statuses, reasons, logs, and exit behavior |
| `lifecycle_contracts` | Preview, apply/repeat, retry, and conservative removal across real tasks |
| `audit_lifecycle` | Existing user-state preservation across preview, convergence, conflicts, and removal |
| `docs_contracts` | Configuration examples and state transitions in human documentation |
| `e2e_apply` | CLI convergence against controlled state |
| `behavioral_ci` | Cross-cutting regressions for CI assumptions |

Use command suites to verify task selection and exit status, and keep per-branch
resource logic in unit tests so failures point straight to the owning module.

## Confine every writable path

Isolate temporary repositories, homes, and state directories together. A
temporary repo alone isn't safe if a command still reads your real home, Git
config, overlay, or log folder.

The shared
[integration helpers](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/mod.rs)
build minimal declarations and isolated commands: `cli_command` redirects
home, config, cache, and log paths into temp dirs, disables self-update, skips
repo sync by default, and initializes a fixture Git repo so Git never walks up
into your real checkout. Opt into the repository-sync variant only when testing
sync behavior.

In unit tests, set both repo root and home, and pass a `MapEnv` instead of
mutating process environment variables. The
[unit helpers](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/test_helpers/mod.rs)
provide `empty_config`, platform contexts, a context builder, and result
assertions; check their defaults carefully, since simulating a Linux platform
doesn't change the host filesystem's semantics.

Never call real package managers, Windows registry hives, systemd, UAC/sudo,
private overlays, or live agent deployments inside tests; use synthetic public
values everywhere.

## Model subprocesses precisely

Inject a test executor so you control both command discovery and process
outcomes independently:

| Test double | Best use |
|---|---|
| `MockExecutor` | Branching behavior, call counts, readiness probes, or unordered execution |
| `ScriptedExecutor` | A fixed, ordered command sequence |
| `FailingExecutor` / `FailingResource` | Failure injection at a chosen lifecycle point |
| `CapturingOutput` | Inspecting emitted events and visible diagnostics |

All four live in
[app/test_helpers](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/test_helpers/).
`ScriptedExecutor::command` asserts program, arguments, working directory, and
checked mode (and `.git(...)` asserts a checked Git invocation), whereas `ok`
and `err` return canned results without checking what command ran.

Use typed `ExecError` values when testing failure propagation, and
`ExecResult::failure` only for unchecked commands whose exit code production
code explicitly interprets. Cover spawn, timeout, and cancellation errors
separately whenever production code branches on them.

## Exercise a complete state transition

Cover every transition relevant to your resource (remembering that some
resources report conflicts instead of overwriting mismatched files):

| Starting state | Assertion |
|---|---|
| Missing | Intended content is created, or missing state stays visibly unmet if creation is unsupported |
| Already correct | No mutation occurs and the result reports current state |
| Different but repairable | Only the owned setting changes |
| Foreign or unsafe | User state stays untouched and the conflict is reported |
| Inspection failed | Failure surfaces cleanly with no speculative install or overwrite |
| Preview | Planned outcome is reported without changing managed state |
| Repeated apply | A second run makes no unnecessary writes |
| Partial failure | Completed work is accounted for while the command still fails |

Look at the
[Shell PATH tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/path.rs)
for a crisp preview/apply/repeat pattern: compare file bytes after preview,
verify the managed line after apply, and confirm zero changes on repeat.

### A complete isolated task regression

Save this self-contained example as `cli/tests/hook_regression.rs` to see the
`common` helpers and testing API in action (the hook content is inert and is
never executed):

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

`TestContextBuilder` creates minimal declarations and a local hooks directory,
while `make_context` sets up an isolated home, `MapEnv`, fixture log dir, and a
forbidding executor; temporary directories clean up automatically on drop.

Run it from `cli/`:

```sh
cargo test --locked --profile ci --test hook_regression
```

On either OS, this runs one test asserting planned counts, zero preview writes,
installed bytes, ownership metadata, untouched user files, and an idempotent
repeat. In real patches, add new cases to the existing owning suite rather than
creating a separate test binary for every regression.

For removal, test a matching managed target, a user-modified target, and a
missing target, asserting what survives just as carefully as what gets removed.
For path and permission logic, also cover resolved destinations and parent
symlinks wherever ownership or containment depends on them. See
[common/lifecycle.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/lifecycle.rs)
for shared symlink, Git hook, and Git settings fixtures.

### Dry-run testing

Assert both what the preview reports and what it did **not** touch: snapshot
fixture files and expected subprocesses before running with dry-run, then
compare afterward (seeing "dry run" in logs never proves files stayed intact).

Use `common::cli_command` for subprocess tests so logs and saved selections
land inside the fixture, and test removal previews separately where supported.
For overlay scripts, write fake scripts that record every invoked flag and
assert that apply/remove modes never ran; never point tests at a real overlay
(see [Private overlays](../guides/security-and-trust.md#private-overlays)).

## Test scheduling without timing guesses

Assert required dependency ordering and failure propagation rather than
incidental completion order, using barriers or channels instead of `sleep` to
coordinate concurrent threads. For graph-only tests, `test_task!` defines quick
no-op tasks with selectors and edges; write custom test tasks when execution or
dynamic keys are under test.

Verify that ordering-only failures don't block independent work, blocking
dependencies do block dependents, unavailable tasks prune cleanly, and
`AccessClaim` blocks conflicting work while allowing independent work to run
concurrently. See
[task_execution.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/task_execution.rs),
[scheduler/conformance.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/scheduler/conformance.rs),
[batch_reports.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/batch_reports.rs),
and [parallel.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/parallel.rs).

## Review failures and coverage honestly

Read snapshot diffs line by line before accepting updates, keeping snapshots
focused on meaningful output contracts.

When fixing a mutation survivor, ask what a user would actually observe if that
fault shipped and assert that outcome directly (a timeout is not a caught
mutant). Replay the original source range even for a test-only fix; see
[Mutation testing](mutation-testing.md) for the disposable-worktree workflow.

Finally, keep simulated platform checks distinct from native runtime coverage:
cross-compilation never tests Windows registry, elevation, or symlinks, and
Unix permission or non-UTF-8 filename tests need a native Unix filesystem (see
[Platform coverage parity](testing.md#platform-coverage-parity)).
