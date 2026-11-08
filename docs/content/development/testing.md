---
title: Testing
description: Select and run existing checks for a patch, from focused commands through committed verification and native-platform coverage.
---

# Testing

Use this page to choose and run existing checks. To create or improve a
regression, use [Writing tests](writing-tests.md) for test ownership, fixtures,
and assertions. [CI and releases](ci-and-releases.md) explains what runs after
a push.

Start with a focused check of the behavior you changed. Before pushing,
validate the committed patch with the coverage selected below. A local pass
is not proof of a complete machine installation, and a skipped check is not a pass.

Choose local coverage from the diff, then let remote CI provide its broader
matrix. A focused check can be the final local check; there is no mandatory
full-suite pass or second run through the coordinator. Keep commit hooks
lightweight. Reuse a passing result while its tested inputs, platform, command,
and tool environment remain unchanged. Before pushing, confirm those inputs
match the committed patch; rerun after relevant changes or uncertain provenance.
Native Linux and Windows tests cover different runtime behavior.

Each example starts at the repository root unless it says otherwise or includes
a `cd` command. Return to the root before following a separate example.

## Choosing coverage

| Change | Start with |
|---|---|
| Skills, agent instructions, or ordinary prose | Review accuracy; metadata and source-link checks as affected. No Rust, site build, or browser run by default |
| Docs routes, headings, rendering, navigation, search, or dependencies | Select the affected [site checks](#documentation-site); build or browser checks only where they exercise the change |
| TOML, categories, or source layout | `config_drift` and local `config` stage |
| Task selection or dependencies | Affected command suite and `task_execution` |
| A concrete resource | Domain unit tests and affected lifecycle/end-to-end cases |
| Process execution | `cargo test --locked --profile ci infra::exec::` from `cli/` |
| Wrappers or hooks | [Isolated wrapper/hook fixtures](#wrapper-and-hook-tests) |
| Desktop or application content | [Desktop shell](#desktop-shell) or [Application configuration](#application-configuration) |
| CI selection or workflows | Local `ci` stage and the changed job's command |
| Arch package recipes | Offline Linux recipe tests, configuration validation/drift, and PKGBUILD/install-script ShellCheck; verify upstream availability and native build/install separately in a disposable Arch home/cache |

Read the changed code and the tests' inputs before choosing targets. A test can
be affected through a shared helper, embedded file, real configuration, or
snapshot even when its source did not change. Include deletions and both sides
of renames. A Markdown file embedded in the CLI is a CLI input, not merely prose.
When the dependency is unclear, inspect it or broaden the checks.

Rust source changes normally need formatting, host Clippy, and the owning unit
or integration tests, using the [pinned toolchain](contributing.md#selecting-the-pinned-toolchain).
Broaden to the full Rust suite for shared configuration, catalog, scheduler,
execution, or lifecycle changes with wide impact. A test-only edit generally
needs its affected target, not unrelated application integrations. Cargo/build
changes need relevant compilation and dependency checks; minimum-Rust checks
are useful when syntax, APIs, dependencies, or the supported version can change.

Use [platform coverage parity](#platform-coverage-parity) to decide whether to
add target compilation or native runtime tests. Cross-target Clippy is useful
for changed platform gates, imports, types, dependencies, or shared APIs consumed
by Windows. An isolated Linux-only implementation edit does not automatically
need Windows compilation. Cross-compilation cannot verify Windows paths,
registry, elevation, process behavior, or symlinks; let exact-commit native CI
supply that coverage when it is unavailable locally. Do not provision another
OS merely to repeat the matrix. Shared code can affect both platforms without
an explicit platform gate.

For prose and skills, metadata/link checks and source review are usually enough.
Heading and path changes need incoming-link checks. Build the site when changed
Markdown syntax, routes, metadata generation, assets, or dependencies need
rendering evidence. Run focused browser checks for changed layout, navigation,
search, diagrams, or interaction. Ordinary wording and internal skills do not
need a running docs site. See the [site check table](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks).

Before running checks, briefly state the affected behavior, selected checks,
and material coverage deferred to CI. This is a short explanation, not another
report or approval step. Broaden after failures or newly discovered impact;
once the selected checks pass, proceed to commit, push, and monitor CI.
Desktop session behavior and package builds still need suitable native or
disposable environments.

### Isolation and side effects

| Kind of check | What it can change |
|---|---|
| Fixture tests | Temporary files, repositories, and subprocesses within controlled roots |
| Repository checks | Build caches and sometimes network access; CLI checks can write logs and selection metadata |
| Host integration | Real home files, packages, services, registry values, or Git history |

Do not run `update`, `remove`, package updates, elevation, or desktop restarts to
compensate for missing tests. Full install/remove and application integration
belong in disposable homes/runners/VMs. The full commit-hook suite also creates
commits; a clean developer checkout is **not** a safe fixture.

## Fast local sequence

### Targeted Rust tests

Run Cargo from `cli/` with the [pinned toolchain](contributing.md#selecting-the-pinned-toolchain).
Combine related integration targets in one invocation:

```sh
cd cli
cargo test --locked --profile ci --test config_drift --test check_command
```

For one failure, list the target's tests before choosing a name filter:

```sh
cd cli
cargo test --locked --profile ci --test update_command -- --list
cargo test --locked --profile ci --test update_command repository
```

The filter is a test-name substring. Check the test count: a misspelled filter
can run zero tests and still exit successfully. Use the
[suite inventory](writing-tests.md#integration-test-suites) to find the owning
target, or [Debugging](debugging.md) to investigate an unexpected result.

### Stage-based checks

The canonical entrypoints are
[check.sh](https://github.com/sneivandt/dotfiles/blob/main/.github/workflows/scripts/linux/check.sh) and
[Check.ps1](https://github.com/sneivandt/dotfiles/blob/main/.github/workflows/scripts/windows/Check.ps1). Select stages for
your patch rather than always running everything.

Linux:

```sh
sh .github/workflows/scripts/linux/check.sh --list
sh .github/workflows/scripts/linux/check.sh --strict fmt clippy test config
```

Windows:

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1 -List
pwsh -File .github\workflows\scripts\windows\Check.ps1 -Strict fmt clippy test config
```

The examples select Rust and configuration checks. For documentation changes,
select the affected [documentation checks](#documentation-site).

With no stage names, the scripts run their default stages:

| Stage | What it checks |
|---|---|
| `fmt` | Rust formatting |
| `clippy` | Host Clippy for all targets, with warnings rejected |
| `test` | Full Rust test suite, not shell/PowerShell integration scripts |
| `rustdoc` | Private-item documentation and configured rustdoc lints |
| `config` | CLI configuration, source paths, and required config files |
| `docs` | Markdown links, skill metadata/fixtures, Rust documentation contracts/selectors, and the VitePress build, published links, and browser regressions; Linux entrypoint only |
| `ci` | Workflow contracts, path/release classification, aggregate gating, and actionlint; Linux entrypoint only |
| `shell` | Shared ShellCheck discovery and rules |
| `powershell` | PSScriptAnalyzer at Warning/Error severity |
| `audit` / `deny` | Dependency advisories / dependency policy |

Cargo verification uses `--locked` and the CI build profile where applicable.
The scripts use tools from `PATH`; they do not select the pinned compiler for
you. Compiler-dependent checks reject the wrong Rust version.

Missing tools usually produce `SKIP`. `--strict`, `-Strict`, or
`DOTFILES_CHECK_STRICT=1` makes skips fail. An installed tool's nonzero exit is
a failure, including exit code `2`. See
[Analyzer prerequisites](#analyzer-prerequisites) for required tools and the
PowerShell entrypoint difference.

Two stages are opt-in: `msrv` attempts to install and compile with the minimum
Rust version from `Cargo.toml`; Linux `qml` runs Qt Quick Test. `--all` on Linux
includes both, while Windows `-All` adds `msrv`. These options can require
downloads or additional tools; they do not run mutation tests or host integration.

The Linux docs stage needs Python, Cargo, the Node version in `docs/.node-version`,
and the [site dependencies and Chromium runtime](#documentation-site).
Its Rust fixtures check task selectors, documentation configuration examples,
JSON Lines serialization, and native Unix state transitions.
It also validates repository skill metadata and runs the validator's synthetic
regressions without third-party Python dependencies. The
[metadata validator](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/skill-maintenance/scripts/check-skills.py)
checks the supported frontmatter shape, names, and descriptions. These checks
do not measure whether an agent follows the instructions effectively.

The source link check inspects tracked and unignored new Markdown files,
including unstaged moves. It checks local inline links and resolves this
repository's GitHub `blob/main` and `tree/main` destinations against the
checkout, including Markdown anchors and source line ranges. It does not
network-check upstream URLs, other repositories or revisions, reference-style
links, Mermaid rendering, or the truth of the prose. A quick
link-only check is:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

Generated site output and `node_modules` are ignored. New documents do not
need to be staged to check their links.

## Before pushing

Review the complete intended range against the refreshed destination branch.
If a previous push failed, keep its pending source changes in scope by comparing
against the last successful ancestor. Validate the committed inputs with the
checks selected under [Choosing coverage](#choosing-coverage). A focused result
from before the commit remains useful when those exact inputs were committed
unchanged; unstaged fixes do not validate committed code.

### Documentation and skills

Use the affected checks from the
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks).
Skill wording and ordinary prose usually need no npm installation, build,
browser suite, or Rust compilation. A mixed patch adds only the checks needed
for its documentation changes. The full `python docs/check.py --committed`
checker is available for broad site changes or uncertain impact, once on a
native platform; it is not required for every Markdown edit.

### Implementation and mixed patches

The coordinated pre-push checker is an optional conservative alternative to
manually selecting checks. It uses CI's broad path policy, so its plan can
include unrelated local work, including a full docs build on a CLI-only patch.
Inspect the plan first; choose focused commands instead when their coverage is
clear. This does not change what remote CI requires. To use the coordinator,
with Python 3.11 or newer, run from Windows:

```powershell
python .github\workflows\scripts\linux\pre-push.py --plan
python .github\workflows\scripts\linux\pre-push.py
```

From Linux:

```sh
python3 -B .github/workflows/scripts/linux/pre-push.py --plan
python3 -B .github/workflows/scripts/linux/pre-push.py
```

Despite its historical directory, the entrypoint supports both platforms.
On Windows it runs documentation and selected native Rust, configuration,
PowerShell wrapper, prompt, and Python fixtures in a temporary native checkout.
It then runs the Linux worker through the default WSL distribution. Linux
snapshots live under `/tmp`, and WSL Cargo artifacts use a separate Linux
cache, never the Windows target directory. WSL reads Git objects from the
source checkout but executes fixtures on the Linux filesystem.

The Windows coordinator checks documentation once and passes the exact commit's
result to Linux automatically. It also avoids repeating PSScriptAnalyzer and
Windows cross-target Clippy after the corresponding native Windows checks.
Linux and Windows Rust tests still both run when selected. A Linux-only
invocation retains Windows cross-target Clippy and reports the native-runtime gap.
Documentation-only work can use the native checker above without provisioning
WSL. Host installation and full application integration remain disposable-runner
coverage; safe shell and editor fixtures are included locally when selected.

If documentation already passed the native checker for this commit, add
`--docs-checked <commit>` to both commands, using the SHA printed by the native
checker. The Linux checker then omits npm setup and the docs stage, but keeps
all selected CLI and platform checks. A different commit is rejected.
Do not provision another OS just to repeat documentation validation.

### Baseline and prerequisites

Refresh `origin/main` before planning. By default, the checker uses authenticated
`gh` reads to verify that tip's push CI. If it did not succeed, the checker selects
full coverage from a successful ancestor, preserving pending source changes even
when the latest fix changes only tests. API/authentication failures and a missing
successful ancestor fail rather than silently narrowing coverage.

For another destination, unavailable GitHub access, or a retry with a known
baseline, pass `--base <original-verified-sha>` and optionally `--head <commit>`.
Keep that original baseline through recovery; do not replace it with a failed
push's tip. Use `--full` to select conservative full coverage explicitly.
The plan prints selected checks without running them. An invalid range fails;
an empty range reports that no checks ran.

The checker exports the selected commit into temporary Git checkouts and runs
CI-selected local checks with strict skip handling. **Unstaged fixes cannot
satisfy a committed patch.** It reuses the Cargo target cache and redirects CLI
state and logs into temporary directories.

Provide the tools required by the printed plan before running the checker.
Keep the pinned compiler on `PATH`; Rust changes can also require the minimum
supported compiler and, on Linux-only runs, the Windows target. Windows
implementation checks require a configured WSL distribution with Linux tools.
Shell fixtures also need Bash, Zsh, fzf, zstd, bat, and tmux; editor fixtures need
Vim and Neovim. Missing required tools fail rather than yielding a passing skip.
When docs have not already been checked,
their checks need Playwright's Chromium runtime. The checker does not install
these tools; it restores locked npm
dependencies only when running docs locally.
Cargo and dependency checks may still need network or cache access. It is a
manual command, not an installed Git hook, and does not push.

Resolve selected local failures before publishing. Its `REMAINING CI` list
identifies full builds/integration, mutation, or publication work left to CI,
even when narrower local fixtures for that area passed. Report unavailable
checks explicitly. A request to push includes monitoring the
[exact commit's aggregate and informational results](ci-and-releases.md#reading-a-run)
by default, unless explicitly declined. A successful Git push is not completion
while CI is pending; report monitoring blockers instead of claiming success.

### Local npm TLS failures

If a local TLS failure prevents the committed snapshot from restoring npm
dependencies, documentation coverage is unavailable, not failed validation.
It need not block a push when the remaining selected local checks pass and
the exact commit's documentation checks will run in GitHub CI.

After confirming the TLS failure, record it explicitly with the coordinator.
From PowerShell:

```powershell
$commit = git rev-parse HEAD
python .github\workflows\scripts\linux\pre-push.py --npm-tls-unavailable $commit
```

The receipt must identify the exact commit being validated. The coordinator
omits npm installation and documentation checks, prints `UNAVAILABLE` rather
than a passing result, and preserves every other selected local check.
Windows forwards that same unavailable status to WSL rather than pretending
documentation passed or retrying it on another platform. This option also
supports documentation-only changes without provisioning WSL.

This is an explicit acknowledgement of an observed npm TLS failure, not an
automatic exception for arbitrary install errors. Do not use it for missing
package versions, authentication errors, build failures, or failing tests.
Do not pass `--docs-checked` for an unavailable check, disable TLS validation,
or weaken CI. Report the local coverage gap and follow the exact commit's
CI and documentation publication through completion. GitHub's documentation
job remains required; its failure still blocks completion.

## Specialized checks

Choose the checks for the files you changed. These recipes supplement the
local stages above; they are not a sequence to run in full.

### Documentation site

For edit-loop checks, browser prerequisites, and manual preview instructions, use
[docs/README.md](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks).
Choose the [committed-patch gate](#before-pushing) above before pushing.

### Staged hook checks

From the repository root, enable extra guards with:

```sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

`true` and `yes` also enable full mode. Set the same variable for `git commit`
to run it during a commit. Guards are selected by staged paths, not run wholesale:

| Trigger | Additional coverage |
|---|---|
| Rust/build inputs | Windows GNU-target Clippy when available, then full Rust tests |
| `conf/*.toml` or `symlinks/` | Typed configuration and `config_drift` |
| Cargo inputs or deny policy | Wildcard dependency guard and available cargo-deny checks |
| Shell or hook paths | Available ShellCheck on affected inputs |
| POSIX wrapper or its tests | Linux wrapper tests |
| Release workflow | Staged publishing invariants, including provenance and permissions |

Deletions and both sides of renames select guards. Rust and CI guards inspect an
isolated snapshot of the index, ignoring unstaged deletions and untracked files;
release guards read the staged workflow directly. Builds reuse the Cargo cache.
The verifier, pattern files, and analyzer helpers themselves come from the
working tree, so review their changes separately. The staged compiler pin is
checked, not installed or switched automatically.

`sh hooks/check-ci-guards.sh` runs targeted guards directly. Without full mode
it omits config drift, cargo-deny, and wrapper tests. Neither invocation alone
establishes coverage of the full committed diff;
choose any additional checks under [Before pushing](#before-pushing).

### Sensitive-pattern scanning

[check-sensitive.sh](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-sensitive.sh)
scans added lines from `HEAD` to the index (an empty tree for an initial commit).
Edited renames do not rescan every unchanged line. Case-insensitive extended
regular expressions match the line text; diagnostics redact matched content.
Invalid detection or allowlist patterns fail even with an empty diff.

Change [sensitive-patterns.ini](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-patterns.ini)
for detection and [sensitive-allowlist.ini](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-allowlist.ini)
for exceptions. Exceptions redact only the matching safe span before scanning
the remaining line. Use synthetic fixtures proving both the intended exception
and detection of a secret elsewhere on the same line. Existing exceptions cover
Action pins, checksums, `test.local` fixtures, and GitHub SSH syntax;
`example.com` addresses are not generally exempt.

The scanner does not cover unchanged history, binary data, or every encoded or
unknown credential format. Follow [Secrets](../guides/security-and-trust.md#secrets).
Keep hook scripts POSIX `sh` compatible and test partially staged input,
renames/deletions, unusual filenames, and empty diffs. Run integration tests only
in the disposable checkout specified under [Wrapper and hook tests](#wrapper-and-hook-tests).
The suite creates commits; never run it against this checkout.

### CLI validation

To check this checkout using the current source, without wrapper bootstrap:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop \
  --only config-diagnostics,symlink-sources,config-files
```

The `--root ..` argument selects this checkout. This command matches the local
`config` stage: it fails on configuration diagnostics, checks
symlink and permission sources, including entries in inactive categories,
and verifies required TOML files. Structural conflicts can fail during loading,
before a check task runs.

To include external plugin and script checks:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop --verbose
```

That adds APM `pack --dry-run --verbose`, ShellCheck, and PSScriptAnalyzer where
applicable. Discovery and file-read failures are not treated as empty input.

Add `--overlay /path/to/private-dotfiles` to validate combined configuration.
Keep its diagnostics private until sanitized: a public-only check cannot prove
an overlay has no conflicts.

### Analyzer prerequisites

| Check | Required tools |
|---|---|
| Local APM package validation | `apm` |
| ShellCheck | `shellcheck` and POSIX `sh` |
| PSScriptAnalyzer | PowerShell 7 (`pwsh`) and the PSScriptAnalyzer module |

Missing tools cause the affected CLI check to be skipped. `--fail-on-skip` or
a present `CI` variable makes those skips fail. Windows PowerShell does not substitute
for `pwsh` in the analyzer.

If `pwsh` is available but PSScriptAnalyzer is absent or broken, CLI checks,
the Linux local checker, and the commit hook fail. The Windows local
`Check.ps1` entrypoint instead skips a missing module; `-Strict` makes that
skip fail too. Install the prerequisites for the checks you intend to run.

### Wrapper and hook tests

### Isolated wrapper and hook regressions

These use fixture tools rather than bootstrapping or installing the real CLI:

```sh
sh .github/workflows/scripts/linux/test-hook-inputs.sh \
  test_staged_ci_guards test_staged_ci_guard_deletions
sh .github/workflows/scripts/linux/test-shell-wrapper.sh \
  test_wrapper_preserves_runtime_context test_wrapper_uses_cargo_artifact
```

With no selector, `test-hook-inputs.sh` runs all input regressions. Its build
metadata case needs the pinned Rust compiler; without it, that case skips.
Classification regressions run once in the `ci` stage, not again in this suite.

On Windows, dot-source the wrapper suite to load only the isolated functions:

```powershell
. .\.github\workflows\scripts\windows\Test-ShellWrapper.ps1
Test-IsolatedWrapperPath
Test-CargoArtifactPath
```

Full wrapper suites can build/copy binaries or use `BINARY_PATH`. Read their
setup before running all cases.

### Full commit-hook suite

**Never run the full commit-hook suite in your developer checkout.**
The `test-git-hooks.sh` script creates real
commits. A clean tree is not a safe fixture; do not use its force override.

The full suite runs in CI's disposable checkout. Prefer the isolated local
regressions above; use a disposable repository and home if you need full
integration coverage. Do not reuse a failed fixture, because its existing
commits can make a later check see an empty diff.

### Cleanup retention and Git templates

```sh
python3 -B .github/workflows/scripts/linux/test-maintenance.py
```

These fixtures check merge/rebase cleanup, protected branches, worktrees, and
file/link/directory retention in temporary repositories and directories. They
do not start services or delete real user files.

### Desktop shell

QML tests need Qt Quick Test from Qt 6.8 or newer. They substitute test doubles
for Quickshell components, so they do not control the desktop:

```sh
sh .github/workflows/scripts/linux/check.sh --strict qml
```

Python helpers, lock scripts, and stock-cache concurrency use isolated fixtures:

```sh
python3 -B -m unittest discover \
  -s symlinks/config/quickshell/tests/python -p 'test_*.py'
python3 -B -m unittest discover \
  -s symlinks/config/hypr/scripts/tests -p 'test_*.py'
sh .github/workflows/scripts/linux/test-stocks.sh
```

Offscreen tests cover logic/layout, not native rendering, focus, or a live
compositor/network session. Helpers mock desktop actions and market requests.
On Windows use `python -B`; Linux-only cases skip. Record native manual
coverage separately.

CI also runs the real Quickshell configuration under a software-rendered Sway
session in a disposable Arch container. It checks bar space reservations,
workspace activation, output addition, resizing and disabling, and acquisition
of a native Wayland session lock with Swaylock.

The same container script accepts `--native-hyprland` to require nested Hyprland
and the tracked Hyprlock configuration. Use a disposable runner with GPU-backed
buffer allocation; the software-only CI runner cannot start that backend.
Unavailable prerequisites fail this mode rather than producing a passing skip.
Neither mode tests password authentication, the systemd user service, physical
displays, or suspend/resume.

### Application configuration

These use isolated homes and mock commands:

```sh
python3 -B .github/workflows/scripts/linux/test-shell-config.py
python3 -B -m unittest discover -s symlinks/vim/tests -p 'test_*.py'
pwsh -NoProfile -File symlinks/config/powershell/tests/Test-Prompt.ps1
```

The PowerShell fixture covers first-start discovery, custom Cargo paths,
literal profile-fragment directories, and PATH deduplication on reload.
Install required native tools; a missing-tool skip gives no coverage.

### CI gates

The `ci-success` gate checks that selected jobs succeeded and unselected jobs
were intentionally skipped. See [CI and releases](ci-and-releases.md) for scheduling and
informational jobs; do not interpret an omitted job as a pass.

### CI selection fixtures

```sh
python3 -B .github/workflows/scripts/linux/test-ci-changes.py
```

These cover classification, aggregate gating, publishing contracts,
renames/deletions, unusual paths, and invalid ranges. Temporary Git repositories
and mocked `gh` calls keep fixture commits and tags local; nothing is published.
Use `ClassificationTests GateTests` as arguments for just those cases.
Also run the local `ci` stage when workflow syntax or contracts change.

### Release selection

Publishing compares the exact tested commit to the last published release,
not merely the previous push. See [Release selection](ci-and-releases.md#release-selection)
for why a docs-only push can recover a previously failed release.

## Focused mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Use local mutation testing
when it adds evidence for changed safety guards, failure handling, scheduling,
or partial success, or to diagnose a surviving mutant. It is not an automatic
local gate for every Rust edit. The selected remote mutation shards remain
required, and their reports must be inspected after pushing.

Follow [Mutation testing](mutation-testing.md) to create a disposable worktree,
select the original committed source range, inspect survivors, and preserve
reports before cleanup. Selected mutation shards must pass CI; ordinary tests
and the local checker's `--all` do not run them.

## Platform coverage parity

For Linux checks from Windows, use a checkout on WSL's Linux filesystem, such
as `~/src/dotfiles`, rather than a Windows-mounted path under `/mnt/c`.
Some fixtures create temporary files **inside the checkout** and require Unix
permission bits or filenames that Windows filesystems cannot represent.
Changing only `TMPDIR` does not relocate those fixtures.

Linux cross-target Clippy catches Windows type/import errors:

```sh
cd cli
cargo clippy --locked --profile ci --target x86_64-pc-windows-gnu \
  --all-targets -- -D warnings
```

It does **not** exercise Windows registry, UAC, symlink/junction, path, or
process behavior. Affected native Windows tests still need to pass, locally or
in exact-commit CI. The
[CI coverage matrix](ci-and-releases.md#platform-coverage) lists those boundaries.

Executor tests launch their ignored child-process fixture themselves; do not
run it manually with `--ignored`.
