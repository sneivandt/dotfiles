---
title: Contributing
description: Find the right place for a change, set up the pinned toolchain, and follow the contribution and verification workflow.
---

# Contributing

If you just want to add a package or tweak an editor preference, jump straight
to the [owning files](#2-find-the-owning-files); you usually don't need to
touch or compile the Rust CLI at all. When you are changing what the CLI
itself does, pick the guide that matches your next step:

| What do you need to do? | Guide |
|---|---|
| Build and run the CLI from source | [Development setup](#development-setup) |
| Understand execution boundaries | [Architecture](architecture.md) |
| Implement a new kind of managed state | [Adding a task](adding-a-task.md) |
| Trace an unexpected result to its cause | [Debugging](debugging.md) |
| Choose and run existing checks | [Testing](testing.md) |
| Design a regression and its fixtures | [Writing tests](writing-tests.md) |
| Explain a feature or maintain its reference | [Writing documentation](writing-documentation.md) |
| Understand CI and publication | [CI and releases](ci-and-releases.md) |

## Contribution workflow

### 1. Check the starting point

```sh
git status --short --branch
git diff --stat
git diff --cached --stat
```

Leave unrelated local edits untouched. Keep in mind that your checkout may be
the live source of home-directory symlinks, so editing files under `symlinks/`
can affect running applications right away. Always edit tracked sources rather
than installed copies.

Verify changes with isolated test fixtures rather than running a real
`dotfiles update` or `dotfiles remove` against your development machine, since
those commands modify real home files, packages, Git config, registry values,
and system files.

### 2. Find the owning files

| What is changing? | Start here |
|---|---|
| Application configuration | The application's directory under [symlinks/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/) |
| Packages, links, settings, or services | The matching file under [conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/); see [Configuration](../guides/configuration-basics.md) |
| Administrator-owned file content | [system/](https://github.com/sneivandt/dotfiles/tree/main/system/) and [conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) |
| CLI options or command selection | [app/cli.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and [app/commands/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/commands/) |
| Task membership or cross-domain dependencies | [app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs) |
| How one kind of state is applied | Its module under [domains/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/) |
| Process, filesystem, or output mechanics | [infra/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/) |
| Custom Arch package recipes | [packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/), [Custom package versions](../reference/custom-package-versions.md), and [Custom package repository](../reference/custom-package-repository.md) |
| Documentation content or site | [Documentation](#documentation), with website sources in the [site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md) |
| Hooks or automation | [Git hooks](#git-hooks) and [CI and releases](ci-and-releases.md) |

Read the closest existing implementation before inventing a new abstraction.
For application preferences, start with
[Application settings](../guides/application-settings.md); for desktop and
session files, start with [Arch desktop](../guides/arch-desktop.md). For Rust
work in unfamiliar areas, [Architecture](architecture.md#repository-layout)
maps each responsibility to its owning layer.

### 3. Make and test the change

Keep desired-state changes declarative. For CLI behavior changes, keep the
configuration, implementation, task wiring, regression tests, and docs
together in the same patch, and avoid unrelated cleanup that clutters the diff.

Test observable failure modes, not only the happy path. State-changing code
should cover repeat runs (idempotency), previews, missing prerequisites, and
partial failures where relevant. Test removal only for tasks that actually
support it, since `remove` is not a universal undo command.

### 4. Review, commit, and verify the push

Choose targeted checks using [Choosing coverage](testing.md#choosing-coverage).
Before committing, review your diff for unintended side effects, private data,
generated artifacts, and stale examples. Stage explicit paths or hunks and
check `git diff --cached`, since the [commit hooks](#git-hooks) inspect the
staged index rather than your working tree.

Make sure your local checks cover the
[committed inputs](testing.md#before-pushing) rather than unstaged edits. You
can reuse passing results when none of their inputs changed; the pre-push
coordinator is an optional conservative helper rather than a mandatory second
gate. Fix any local failures and note anything deferred to CI. Keep commit
hooks lightweight. After pushing, monitor the exact commit's
[CI run](ci-and-releases.md#reading-a-run) (including informational failures)
and any required publication step before considering the change shipped.

## Development setup

You only need this setup for Rust CLI changes or local source builds. Editing
application preferences or guide prose doesn't require compiling the CLI; see
[Choosing coverage](testing.md#choosing-coverage) for what to run instead.

Install Git, rustup-managed Rust/Cargo, and native build libraries for the
CLI's C dependencies: `base-devel`, `pkgconf`, and `openssl` on Arch, or the
C++ build tools and Windows SDK requested by the Rust installer for Windows
MSVC builds.

Entering `cli/` automatically activates the repository's pinned toolchain
through rustup:

```sh
cd cli
cargo run --locked --profile ci -- --help
```

This runs your local checkout directly. Avoid testing Rust edits through the
bootstrap wrappers, which can download a release or run a cached binary
instead.

### Selecting the pinned toolchain

Use rustup-managed `cargo` and `rustc` from inside `cli/`, where rustup reads
[rust-toolchain.toml](https://github.com/sneivandt/dotfiles/blob/main/cli/rust-toolchain.toml)
and installs any missing components or targets:

```sh
cd cli
rustup show active-toolchain
rustc --version
```

Verify the reported toolchain matches `rust-toolchain.toml` before running
checks (system-packaged Rust toolchains can ignore it). Configure your editor
to use the same toolchain and restart rust-analyzer after switching.

The MSRV declared in
[Cargo.toml](https://github.com/sneivandt/dotfiles/blob/main/cli/Cargo.toml)
is a separate compatibility target rather than the compiler for day-to-day
development. If a required compiler or component is unavailable locally, note
the missing coverage instead of substituting a different version.

### Builds and debugging

Use `cargo build` and `cargo test` for fast edit-compile loops, or `--profile ci`
to match CI verification. Local profiles keep incremental compilation, debug
assertions, and overflow checks while omitting debug symbols to save time and
disk space.

To re-enable debug symbols for a Linux build:

```sh
CARGO_PROFILE_DEV_DEBUG=2 cargo build --locked
```

Match the environment variable to Cargo's active profile:
`CARGO_PROFILE_TEST_DEBUG` for plain `cargo test`, or
`CARGO_PROFILE_CI_DEBUG` for the `--profile ci` commands shown in
[Panics and debugger builds](debugging.md#panics-and-debugger-builds). From
`cli/`, enable debug info for a single CI-profile test run in a POSIX shell:

```sh
CARGO_PROFILE_CI_DEBUG=2 cargo test --locked --profile ci --test update_command repository
```

In PowerShell, restore the previous value afterward so later commands don't
inherit it:

```powershell
$previousDebug = $env:CARGO_PROFILE_CI_DEBUG
try {
    $env:CARGO_PROFILE_CI_DEBUG = '2'
    cargo test --locked --profile ci --test update_command repository
} finally {
    $env:CARGO_PROFILE_CI_DEBUG = $previousDebug
}
```

Use `line-tables-only` instead of `2` if you only need file and line numbers
in backtraces. Changing debug settings rebuilds affected crates.

Install optional analyzers only for the checks you plan to run; see the
[fast local sequence](testing.md#fast-local-sequence) for their prerequisites.

### Source builds

To build and run the local checkout through a wrapper instead of downloading a
release, pass `--build`. These commands compile and print the version without
applying any configuration:

```sh
./dotfiles.sh --build --version
```

```powershell
.\dotfiles.ps1 --build --version
```

Run them from the repository root with the build prerequisites installed. The
wrapper compiles with the `dev-opt` profile and executes Cargo's output
artifact directly (respecting custom target directories rather than writing
`bin/dotfiles` or `bin\dotfiles.exe`).

`--build` applies only to that invocation, so keep passing it (or use
`dotfiles --build` via the installed launcher) whenever you want a source
build; omitting it returns to the downloaded or cached binary path. For
day-to-day Rust development, prefer direct `cargo` commands from `cli/`.

## Change checklists

### Config-backed state

When changing an existing field or category, update the TOML data and add a
focused regression test, keeping conditional symlinks aligned with categories.
For a brand-new configuration section, follow
[Add configuration only where needed](adding-a-task.md#add-configuration-only-where-needed)
for typed decoding, inventory registration, validation, and fixtures. Document
schema details on the owning [task reference page](../reference/task-reference.md)
and shared loading rules in
[Configuration basics](../guides/configuration-basics.md).

### Tasks and workflows

Follow [Adding a task](adding-a-task.md) end to end: pick resource vs.
operation ownership, define applicability and requirements, wire command
membership and dependencies, and write the reader-facing contract. See
[Architecture](architecture.md) for why those boundaries exist and
[Writing tests](writing-tests.md) for isolated lifecycle and scheduler
regressions.

### Platform-specific changes

Use runtime capabilities and injected adapters for platform branching, keeping
compile-time `cfg` gates only around platform-specific APIs so other targets
still compile. Keep tests free of process-global mutations, real package
managers, and live elevation.

Remember that cross-compiling for Windows doesn't exercise Windows runtime
behavior; see [Platform coverage parity](testing.md#platform-coverage-parity).

## Git hooks

The scripts in [hooks/](https://github.com/sneivandt/dotfiles/tree/main/hooks/)
validate staged changes before committing. They are separate from the shared
[personal Git templates](../guides/application-settings.md#git-templates) and
complement rather than replace pre-push validation and CI.

Review `git diff --cached`, then commit as usual. To run the staged checks
without committing:

```sh
sh hooks/pre-commit
```

The hook runs the sensitive-pattern scanner on added staged lines, then runs
checks matching what you staged:

| Staged change | Default check |
|---|---|
| Rust source, Cargo inputs, or compiler pin | Whole-crate formatting and host Clippy using the staged toolchain pin |
| Present PowerShell files | PSScriptAnalyzer on staged files |
| Only TOML data, shell, or workflow files | Sensitive scan; additional guards require full mode |

Hooks never stage fixes or stash edits for you, so fixing a working-tree file
without restaging it will still fail on the index. Missing tools are skipped
rather than treated as passes. See
[Staged hook checks](testing.md#staged-hook-checks) for optional full guards
and scanner maintenance.

### Installation and removal

See the [Git hooks task](../reference/git-hooks.md) for hook destinations,
ownership records, previewing installation, and removal. Back up any existing
custom hooks before installing, and keep each managed hook's ownership record
next to it.

### Diagnosing a rejected commit

Start by comparing `git diff` against `git diff --cached`. For formatting
failures, run `cargo fmt` in `cli/`, inspect the diff, and restage the
intended lines. A build error that happens only in the hook usually means the
staged index is missing a file or dependency you edited in the working tree.
If the hook doesn't fire at all, check the configured hooks path, filename,
and Unix executable bit.

Inspect sensitive-pattern matches locally: remove and rotate real secrets
immediately, and never paste raw findings into logs or issues. Narrow scanner
allowlist exceptions require synthetic regression tests. Reserve
`git commit --no-verify` for reviewed recovery from a broken check, never to
bypass a real secret finding.

## Documentation

Update the guide that owns the reader's question rather than duplicating an
explanation in a second place: everyday commands in
[Commands and usage](../guides/commands-and-usage.md), shared config rules in
[Configuration basics](../guides/configuration-basics.md), and task-specific
behavior on each [task reference page](../reference/task-reference.md).

Read [Writing documentation](writing-documentation.md) for style, examples,
verification, and task-page structure, and the
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md)
for local preview, build checks, and publishing.

Next, read [Architecture](architecture.md) to see how the CLI layers, task
graph, resources, and operations fit together.
