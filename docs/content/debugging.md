---
title: Debugging
description: Reproduce a CLI problem safely, find its owning layer, and turn the diagnosis into a focused regression.
---

# Debugging

Start by identifying the binary, configuration, and failure you are actually
observing. Then reproduce the smallest part of the problem in a fixture.
Repeatedly applying configuration to the development machine can destroy the
state that explains the failure, as well as make unrelated changes.

This guide is for investigating the Rust CLI. For a failed installation,
start with [Troubleshooting](troubleshooting.md). Use
[Contributing](contributing.md#development-setup) for the build environment and
[Architecture](architecture.md) for execution contracts.

## Confirm the inputs

Record the command and exit status, CLI version, operating system, selected
profile, and whether an overlay was active. Distinguish the checkout's
uncommitted source from the release binary behind an installed launcher.
A wrapper can download or reuse a binary; it does not automatically execute
the Rust code you just edited.

From `cli/`, inspect the current source without applying configuration:

```sh
cargo run --locked --profile ci -- --version
cargo run --locked --profile ci -- --help
```

For a task-selection problem, inspect a graph. This example explicitly selects
the base profile from the parent checkout:

```sh
cargo run --locked --profile ci -- list --root .. --profile base --graph update --only symlinks --with-deps
```

The graph shows selected tasks and declared dependencies, not runtime
prerequisite results or planned file changes. Listing does not run task probes
or save profile selections. Use the affected profile and overlay when those
inputs explain the failure, but keep private configuration out of shared
reproducers.

## Locate the failing boundary

Follow the first relevant error rather than the last blocked task. Later work
can be blocked because an earlier prerequisite failed; fixing its consumer
would hide the real cause.

| What failed? | Start with |
|---|---|
| Argument parsing or a missing option | [app/cli.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and CLI parser tests |
| Decoding, categories, or conflicting declarations | [app/config/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/config/) and the domain's decoder |
| A missing or unexpectedly selected task | The command's task list, [app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs), and command tests |
| A task waiting or running too early | Its dependency edges, runtime requirements, and engine scheduler tests |
| A wrong file, setting, or package decision | The domain resource's discovery and apply methods |
| A spawn error, timeout, or unexpected exit interpretation | The caller's checked/unchecked executor contract and [infra/exec/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/exec/) |
| Correct work with misleading output or exit status | Task result accounting, [infra/logging/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/logging/), and command outcome tests |
| Different behavior on another OS | Runtime platform/environment adapters, compile-time API gates, and native-platform tests |

A failed state query is not evidence that the resource is missing. If the
program installs or overwrites after such a failure, inspect that branch
before adding a retry. A shared lifecycle bug belongs in the engine; a single
tool's exit-status protocol usually belongs with that tool's resource.

## Read an existing run without repeating it

Use the stable log command printed by the failed run. With an existing CLI
binary, these commands read recorded results without applying configuration:

```sh
dotfiles log --list
dotfiles log --task symlinks --verbose
```

The second command filters the newest run. To inspect an older failure, use
its `--id` from the first command rather than assuming it is still newest.
Follow linked child logs after repository reloads or elevation.
[Logs](commands-and-usage.md#logs) describes the output and storage contract.

Separate a reported failure from skipped, blocked, and inapplicable work.
Include the underlying subprocess diagnostic where available, not only the
summary label. Sanitize private paths, configuration, and external tool
output before sharing logs.

## Reproduce the behavior in a test

Choose the lowest layer that can express the failure and replace external
state with a temporary home, repository, environment, and executor.
[Writing tests](writing-tests.md) describes the helpers and suite boundaries.

If a subprocess result triggers the bug, reproduce that exact command,
checked mode, exit result, and relevant output. An always-successful mock
cannot reproduce a failed discovery query. For a filesystem problem, preserve
the relevant distinction: absent versus unreadable, ordinary file versus
symlink, or managed versus foreign content.

Run the owning target from `cli/`. For example:

```sh
cargo test --locked --profile ci --test update_command -- --list
cargo test --locked --profile ci --test update_command repository -- --nocapture
```

The second command runs names containing `repository`; substitute the filter
you found in the listing and check that it selects tests. Prefer assertions
about files, commands, results, and visible errors over temporary print
statements.

### Panics and debugger builds

For a panic, enable a backtrace in the same shell invocation as the test.
On a POSIX shell:

```sh
RUST_BACKTRACE=1 cargo test --locked --profile ci --test update_command
```

On PowerShell:

```powershell
$env:RUST_BACKTRACE = '1'
cargo test --locked --profile ci --test update_command
```

Backtraces do not replace the error context of an ordinary returned failure.
If frames lack useful symbols, enable debug information as described in
[Builds and debugging](contributing.md#builds-and-debugging).
Revert temporary logging and inspect snapshot diffs before finishing.

## Verify the fix, not just the symptom

First prove the reproducer fails before the fix and passes after it. Add the
nearby preservation or failure case that would catch a tempting but unsafe
shortcut, such as treating every query error as absence.

Then choose broader coverage from [Testing](testing.md#choosing-coverage).
Native process, registry, symlink, and permission behavior still needs the
affected platform. A mocked platform decision or successful cross-compilation
does not establish native runtime behavior.
