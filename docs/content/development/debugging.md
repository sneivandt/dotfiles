---
title: Debugging
description: Reproduce a CLI problem safely, find its owning layer, and turn the diagnosis into a focused regression.
---

# Debugging

Start by pinning down the exact binary, configuration, and failure you're
seeing, then reproduce the smallest failing case in an isolated fixture.
Re-running `dotfiles update` against your own machine risks wiping out the
state that explains the bug while making unrelated changes.

This guide covers investigating the Rust CLI itself. If an installation run
failed on your machine, start with
[Troubleshooting](../guides/troubleshooting.md). Keep
[Development setup](contributing.md#development-setup) and
[Architecture](architecture.md) handy for build commands and layer boundaries.

## Confirm the inputs

Note the exact command, exit code, CLI version, OS, profile, and active
overlay. Make sure you know whether you're running uncommitted local source or
the cached release binary behind your installed launcher (wrappers can download
or reuse a release binary instead of compiling your latest edits).

From `cli/`, inspect your current source directly without applying anything:

```sh
cargo run --locked --profile ci -- --version
cargo run --locked --profile ci -- --help
```

For a task-selection bug, inspect the graph (this example selects `base` from
the parent checkout):

```sh
cargo run --locked --profile ci -- list --root .. --profile base --graph update --only symlinks --with-deps
```

Graph output shows selected tasks and declared edges without probing runtime
prerequisites, touching files, or saving profile choices. Include the relevant
profile and overlay when reproducing locally, while keeping private overlay
data out of shared bug reports.

## Locate the failing boundary

Trace the first real failure rather than the last blocked task; fixing a
downstream consumer only hides an unmet upstream prerequisite.

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

Never confuse a failed discovery query with a missing resource: if code
overwrites or installs after an inspection error, fix that branch instead of
retrying. Shared lifecycle bugs belong in `engine/`, while an external tool's
exit-code conventions belong in that tool's domain resource.

## Read an existing run without repeating it

Inspect the failed run using its printed log command rather than re-running
`update`:

```sh
dotfiles log --list
dotfiles log --task symlinks --verbose
```

The second command inspects the latest run; pass `--id <run-id>` from
`dotfiles log --list` if another run has happened since, and follow any linked
child logs from repository restarts or Windows elevation. See
[Logs](../guides/commands-and-usage.md#logs) for log layout and retention.

Distinguish genuine failures from skipped, blocked, and inapplicable tasks, and
capture the underlying subprocess diagnostic rather than just the summary row.
Sanitize private paths, config values, and tool output before sharing excerpts.

## Reproduce the behavior in a test

Write the reproducer at the lowest layer that exhibits the bug, substituting a
temporary home, repo, environment, and mock executor for host state (see
[Writing tests](writing-tests.md) for fixture helpers and suite conventions).

When a subprocess response causes the bug, mock that exact command, checked
mode, exit code, and output (an always-succeeding mock can't expose a broken
error path). For filesystem bugs, reproduce the exact condition: missing vs.
unreadable, regular file vs. symlink, or managed vs. foreign target.

Run the targeted test binary from `cli/`:

```sh
cargo test --locked --profile ci --test update_command -- --list
cargo test --locked --profile ci --test update_command repository -- --nocapture
```

Replace `repository` with a test name from the listing and verify the filter
actually matches tests. Assert on filesystem state, executed commands, task
results, and visible diagnostics rather than leaving temporary print debugging
behind.

### Panics and debugger builds

For panics, enable backtraces for that single command:

```sh
RUST_BACKTRACE=1 cargo test --locked --profile ci --test update_command
```

In PowerShell:

```powershell
$env:RUST_BACKTRACE = '1'
cargo test --locked --profile ci --test update_command
```

Backtraces supplement panics rather than replacing clear error context on
returned `Err` values. If stack frames lack symbols, turn on debug info for the
matching Cargo profile (`CARGO_PROFILE_CI_DEBUG` for `--profile ci`, not the
`dev` or `test` variables; see
[Builds and debugging](contributing.md#builds-and-debugging)). Remove temporary
debug logs and check snapshot diffs before committing.

## Verify the fix, not just the symptom

Confirm the reproducer fails before your fix and passes afterward, and add a
companion preservation or error-path test that guards against unsafe shortcuts
(such as treating query failures as absence).

Then run the broader local checks from
[Choosing coverage](testing.md#choosing-coverage), keeping in mind that native
process, registry, symlink, and permission paths still require validation on
their target OS. When your fix changes user-facing behavior or flags, check
[Writing documentation](writing-documentation.md) to keep the guides in sync.
