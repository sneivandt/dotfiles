---
title: Contributing
description: Find the right place for a change, set up the pinned toolchain, and follow the contribution and verification workflow.
---

# Contributing

Most changes start with one application's files, not the CLI engine. This guide
helps you find the right place, make a bounded change, and verify what you will
push. Use [Architecture](architecture.md) for design details and
[Testing](testing.md) for exact checks.

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
| Packages, links, settings, or services | The matching file under [conf/](https://github.com/sneivandt/dotfiles/tree/main/conf/); see [Configuration](configuration-reference.md) |
| Administrator-owned file content | [system/](https://github.com/sneivandt/dotfiles/tree/main/system/) and [conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) |
| CLI options or command selection | [app/cli.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and [app/commands/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/commands/) |
| Task membership or cross-domain dependencies | [app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs) |
| How one kind of state is applied | Its module under [domains/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/) |
| Process, filesystem, or output mechanics | [infra/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/) |
| Custom Arch package recipes | [packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/) and [Arch package recipes](package-management.md#arch-package-recipes) |
| Documentation content or site | [Documentation](#documentation), with website sources in the [site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md) |
| Hooks or automation | [Git hooks](#git-hooks) and [CI and releases](ci-and-releases.md) |

Read the closest existing implementation before adding a new abstraction.
Repository [skills](https://github.com/sneivandt/dotfiles/tree/main/.agents/skills/) contain focused contributor procedures;
they do not replace the source or these guides. For common configuration work,
start with [shell configuration](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/shell-configuration/SKILL.md),
[Hyprland sessions](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/hyprland-session/SKILL.md),
[Quickshell](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/desktop-shell/SKILL.md), or
[Vim/Neovim](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/editor-configuration/SKILL.md).
[Self-update](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/self-update/SKILL.md) covers binary replacement;
[Rust patterns](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/rust-patterns/SKILL.md) routes unfamiliar CLI work.

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
A request to push includes that monitoring by default, not just uploading Git
objects. Do not report completion while selected CI is pending.

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

Use `line-tables-only` instead of `2` for file/line backtraces, or
`CARGO_PROFILE_TEST_DEBUG` for tests. In PowerShell, set the corresponding
environment variable before invoking Cargo. Changing these settings rebuilds
affected artifacts.

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
For a new configuration section:

1. Add its typed decoder and semantic validation in the owning domain, including
   invalid-input tests.
2. Add it to `config_section_inventory!` in
   [app/config](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/config/). This provides its field, typed store
   handle, summary count, and empty unit fixture.
3. Wire any special loading, overlay behavior, provenance, and platform
   filtering in `Config::load`, and validation in `Config::validate`.
4. Implement the resource or operation, register its task, and add real
   configuration plus command/configuration tests.
5. Keep conditional symlinks aligned with categories and document the format
   in [Configuration](configuration-reference.md).

### Tasks and workflows

A resource manages independent items; an operation manages a workflow
that converges as one unit. Reuse their lifecycle helpers rather than writing
another preview/apply/result-accounting loop.

Give a task a stable selector and explicit applicability, prerequisites,
privilege requirements, and command membership. Static update/remove tasks go
in the catalog; command-specific tasks belong to that command. Same-domain
dependencies stay in the domain, and cross-domain dependencies belong in the
app layer. List order does not schedule work.

Use a failure-blocking dependency when success is required, and an ordering-only
dependency when the task should wait and then recheck state itself. Preserve
selection, `--with-deps`, and discovery behavior; update the
[task reference](task-reference.md). [Architecture](architecture.md) explains the contracts
and existing task helpers.

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

Inspect the hook destination before installing:

```sh
git config --show-origin --get core.hooksPath
git rev-parse --git-common-dir
dotfiles update --only git-hooks --dry-run
```

No output and status 1 from the first command means no override is configured.
Dotfiles honors a repository-local `core.hooksPath`, resolving relative values
from the repository root; otherwise it uses the common Git directory's `hooks/`,
including worktrees. An inherited shared override blocks installation until you
choose a repository-local destination.

Installation preserves foreign or locally modified hooks and reports a conflict.
Back them up and move them aside before retrying. Matching legacy copies can be
adopted; subsequent updates use a sibling `.dotfiles-owned` hash record to check
ownership. Keep that record with the installed hook. Extensionless entrypoints
are copied, while `.sh` helpers and pattern files run from the checkout. Reinstall
after changing the copied `pre-commit`; helper edits take effect immediately.

To remove managed hooks, preview `dotfiles remove --only git-hooks --dry-run`,
then apply only after review. Removal preserves modified or mismatched hooks.
Install and removal leave a destination that aliases the source intact and do
not reset `core.hooksPath`. Missing hooks sources or Git metadata makes the task
inapplicable.

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

Write for the reader's task. Use direct language, short examples with verified
commands, and safety information beside the action it affects. Keep private
content, credentials, and unsanitized logs out of examples.

Keep only flags needed for the example's purpose. Use headings that name the
reader's task or the setting they need to find, and inspect the generated
"On this page" navigation. Prefer two-column tables when cells need explanation;
keep wider tables for short comparisons. Highlight code lines only when the
surrounding text explains why they matter.

Give each explanation one home. Everyday commands belong in
[Commands and usage](commands-and-usage.md); exact syntax belongs in
[CLI reference](cli-reference.md). Keep application, agent, and package guidance
in their owning guides, with maintenance details after everyday workflows.
Link to existing explanations instead of repeating them.

Use relative `.md` links between guides and full GitHub URLs for tracked sources
outside the guide collection. For new pages, match the title, filename, and
sidebar label, then register the guide in
[docs/.vitepress/config.mts](https://github.com/sneivandt/dotfiles/blob/main/docs/.vitepress/config.mts).
Update incoming links after moving pages or headings; do not add compatibility
routes or heading aliases.

Run the [documentation checks](testing.md#documentation-site) and review the
rendered result. Site preview, configuration, and publishing instructions live in
[docs/README.md](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md).
For detailed guidance, use
[Docs writing](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/docs-writing/SKILL.md)
and [Docs publishing](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/docs-publishing/SKILL.md).
