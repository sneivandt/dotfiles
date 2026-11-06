---
title: Contributing
description: Find the right place for a change, set up the pinned toolchain, and follow the contribution and verification workflow.
---

# Contributing

Most changes start with one application's files, not the CLI engine. This guide
helps you find the right place, make a bounded change, and verify what you will
push. Use [Architecture](architecture.md) for design details and
[Testing](testing.md) for exact checks. For implementation work, use
[Adding a task](adding-a-task.md) to follow a feature through configuration and
scheduling, and [Writing tests](writing-tests.md) to build isolated regressions.

| What do you need to do? | Guide |
|---|---|
| Find a change's owner and prepare the toolchain | This page |
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

Preserve unrelated work. This checkout may be the live source of home-directory
symlinks: editing `symlinks/` can affect an application immediately. Edit tracked
sources, not installed copies.

Use isolated fixtures, not a real `update` or `remove`, to test changes. Those
commands can alter home files, packages, Git settings, the registry, and system
configuration. A clean working tree does not make a live installation safe.

### 2. Find the owning files

| What is changing? | Start here |
|---|---|
| Application configuration | The application's directory under [symlinks/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/) |
| Packages, links, settings, or services | The matching file under [conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/); see [Configuration](configuration-basics.md) |
| Administrator-owned file content | [system/](https://github.com/sneivandt/dotfiles/tree/main/system/) and [conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) |
| CLI options or command selection | [app/cli.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and [app/commands/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/commands/) |
| Task membership or cross-domain dependencies | [app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs) |
| How one kind of state is applied | Its module under [domains/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/) |
| Process, filesystem, or output mechanics | [infra/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/) |
| Custom Arch package recipes | [packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/) and [Arch package recipes](arch-package-recipes.md) |
| Documentation content or site | [Documentation](#documentation), with website sources in the [site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md) |
| Hooks or automation | [Git hooks](#git-hooks) and [CI and releases](ci-and-releases.md) |

Read the closest existing implementation before adding a new abstraction.
For application preferences, start with [Application settings](application-settings.md);
for session and desktop behavior, use [Arch desktop](arch-desktop.md).
Those guides identify the editable sources and explain effects that can occur
before the CLI runs. [Architecture](architecture.md#repository-layout) maps
unfamiliar Rust work to its owning layer.

### 3. Make and test the change

Keep desired-state edits declarative. For a behavior change, include the
configuration, implementation, wiring, regression test, and relevant docs.
Avoid unrelated cleanup that obscures the fix.

Test the observable failure, not just the happy path. State-changing code should
cover repeat execution, preview, unavailable prerequisites, and partial failure
where relevant. Test removal only where the feature promises it: `remove` is
not a universal undo operation.

### 4. Review, commit, and verify the push

Start with [Choosing coverage](testing.md#choosing-coverage). Before committing,
review the diff for unintended behavior, private data, generated files, and
stale examples. Stage explicit paths or hunks and inspect `git diff --cached`.
The [commit hooks](#git-hooks) inspect the index, which may differ from your
working files.

After committing, run the [pre-push checker](testing.md#before-pushing).
It verifies the committed range, not unstaged fixes. Resolve local failures and
record unavailable checks. Keep ordinary hooks lightweight rather than running
the full gate at every commit. After pushing, follow the exact commit's
[CI result](ci-and-releases.md#reading-a-run), including informational failures.
Wait for the selected checks and any required publication before treating the
change as delivered. A successful upload is not evidence that CI passed.

## Development setup

This setup is for Rust CLI changes or source builds. Application preferences
and guide prose do not require compiling the CLI just to edit them; select
their checks from [Testing](testing.md#choosing-coverage).

You need Git, rustup-managed Cargo/Rust, and native build tools for the CLI's
C dependencies. On Arch, provide `base-devel`, `pkgconf`, and `openssl`.
For Windows's default MSVC toolchain, install the C++ build tools and Windows
SDK requested by the Rust installer.

Rustup selects the repository's pinned toolchain when you enter `cli/`.
From the checkout:

```sh
cd cli
cargo run --locked --profile ci -- --help
```

This runs current source. Do not use a bootstrap wrapper to test Rust changes:
it may download a release or reuse a different binary.

### Selecting the pinned toolchain

Use rustup-managed `cargo` and `rustc`, and run them from `cli/`. Rustup reads
[rust-toolchain.toml](https://github.com/sneivandt/dotfiles/blob/main/cli/rust-toolchain.toml) there and can install missing
components or targets.

```sh
cd cli
rustup show active-toolchain
rustc --version
```

Check the reported toolchain against the file before validation. System-installed
Rust tools can ignore it. Configure your editor to use the same toolchain and
restart its Rust language server after switching.

The minimum supported Rust version in [Cargo.toml](https://github.com/sneivandt/dotfiles/blob/main/cli/Cargo.toml) is a
separate compatibility check, not the compiler to use for ordinary development.
If a required compiler or component is unavailable, report the missing coverage
rather than substituting another version and calling it a pass.

### Builds and debugging

Use `cargo build` and `cargo test` for a normal edit/build loop, or the `ci`
profile to reproduce verification. Local builds keep incremental compilation,
debug assertions, and overflow checks, but omit debug information to reduce
build time and disk use.

To restore debugger information for a Linux build:

```sh
CARGO_PROFILE_DEV_DEBUG=2 cargo build --locked
```

The override must match Cargo's selected profile. Use
`CARGO_PROFILE_TEST_DEBUG` for ordinary `cargo test`, but
`CARGO_PROFILE_CI_DEBUG` for the `--profile ci` commands in
[Debugging](debugging.md#panics-and-debugger-builds). From `cli/`, enable
debugger information for one targeted CI-profile test run in a POSIX shell:

```sh
CARGO_PROFILE_CI_DEBUG=2 cargo test --locked --profile ci --test update_command repository
```

In PowerShell, restore the previous environment value afterward so later
commands do not inherit the override:

```powershell
$previousDebug = $env:CARGO_PROFILE_CI_DEBUG
try {
    $env:CARGO_PROFILE_CI_DEBUG = '2'
    cargo test --locked --profile ci --test update_command repository
} finally {
    $env:CARGO_PROFILE_CI_DEBUG = $previousDebug
}
```

Use `line-tables-only` instead of `2` when only file/line backtraces are
needed. Changing these settings rebuilds affected artifacts.

Install optional analyzers only for the checks you need. The
[local stages](testing.md#fast-local-sequence) explain their requirements.

### Source builds

To use the checkout through a wrapper instead of downloading a release, pass
`--build`. These commands build and print the version without applying
configuration:

```sh
./dotfiles.sh --build --version
```

```powershell
.\dotfiles.ps1 --build --version
```

Run them from the repository root with the prerequisites above installed.
Builds may download dependencies and write build artifacts. The wrapper uses
the dev-opt build profile and executes Cargo's reported artifact, honoring custom
target/output directories; it need not create `bin/dotfiles` or
`bin\dotfiles.exe`.

`--build` applies to one invocation. Keep it in later wrapper commands, or use
`dotfiles --build` after installing the launcher. Without it, the wrapper
returns to its download/cached-binary route. For normal Rust development,
prefer the direct Cargo commands above.

## Change checklists

### Config-backed state

For an existing field or category, change the data and its focused regression.
Keep conditional symlinks aligned with categories. For a new section, follow
[Adding configuration](adding-a-task.md#add-configuration-only-where-needed),
including typed decoding, inventory registration, validation, and fixtures.
Document the format on its [task page](task-reference.md); shared loading rules
belong in [Configuration basics](configuration-basics.md).

### Tasks and workflows

Follow [Adding a task](adding-a-task.md) for the full implementation workflow:
choose resource or operation ownership, define applicability and readiness,
wire command membership and dependencies, and document the reader-facing
contract. [Architecture](architecture.md) explains why those boundaries exist.
Use [Writing tests](writing-tests.md) for lifecycle and scheduling regressions.

### Platform-specific changes

Use runtime capabilities and injected adapters for decisions. Keep compile-time
gates around platform-only APIs so the other build still compiles. Tests should
not change process-global state or invoke real package managers or elevation.

Cross-target compilation is not a Windows runtime test. See
[Platform coverage parity](testing.md#platform-coverage-parity).

## Git hooks

The scripts in [hooks/](https://github.com/sneivandt/dotfiles/tree/main/hooks/)
check staged changes before committing. These contributor checks are separate
from the shared [personal Git templates](application-settings.md#git-templates)
and do not replace pre-push validation or CI.

Review `git diff --cached`, then commit normally. To check the index without
creating a commit, run from this checkout:

```sh
sh hooks/pre-commit
```

The hook scans added staged text for sensitive patterns, then selects checks:

| Staged change | Default check |
|---|---|
| Rust source, Cargo inputs, or compiler pin | Whole-crate formatting and host Clippy using the staged toolchain pin |
| Present PowerShell files | PSScriptAnalyzer on staged files |
| Only TOML data, shell, or workflow files | Sensitive scan; additional guards require full mode |

Checks do not stage fixes or stash your work. A fixed working copy still fails
if the index contains the error. Review and stage only the intended fix before
retrying. Missing tools are skips, not coverage. For optional full guards and
scanner maintenance, see [Staged hook checks](testing.md#staged-hook-checks).

### Installation and removal

The [Git hooks task](git-hooks.md) explains destination selection, ownership
records, previewing installation, and removal. Preserve custom hooks before
installing and keep the ownership record beside each managed hook.

### Diagnosing a rejected commit

Compare `git diff` and `git diff --cached` first. For formatting, run `cargo fmt`
in `cli/`, review, and restage the intended changes. A hook-only build failure can
mean the staged snapshot lacks a source or dependency present in your working copy.
If no hook runs, check its actual destination, filename, and Unix executable bit.

Inspect sensitive findings locally. Remove and rotate real credentials; never
share raw findings or unsanitized logs. Narrow scanner exceptions need synthetic
regression coverage. `git commit --no-verify` skips the whole hook and is reserved
for reviewed recovery from a broken check, never known sensitive content.

## Documentation

Update the guide that owns the reader's workflow rather than adding a second
explanation elsewhere. Everyday commands belong in
[Commands and usage](commands-and-usage.md), shared declaration rules in
[Configuration basics](configuration-basics.md), and each task's behavior and
format on its [task page](task-reference.md).

[Writing documentation](writing-documentation.md) covers page structure,
reusable examples, source verification, and task reference conventions.
Site preview, checks, and publication mechanics live in the
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md).
