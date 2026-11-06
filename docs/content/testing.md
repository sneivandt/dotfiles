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

Use focused checks during editing, ordinary lightweight commit hooks, and one
broad committed pre-push gate. Full hook mode is optional troubleshooting
coverage, not another required step before the same checks run at the final gate.
Reuse results only for the same commit, platform, command, and relevant tool
environment. Tests on native Linux and Windows cover different runtime behavior.

Each example starts at the repository root unless it says otherwise or includes
a `cd` command. Return to the root before following a separate example.

## Choosing coverage

| Change | Start with |
|---|---|
| Documentation or skills | Focused metadata/link fixtures while editing; [native docs checker](#documentation-site) and rendered-page review at the final gate |
| TOML, categories, or source layout | `config_drift` and local `config` stage |
| Task selection or dependencies | Affected command suite and `task_execution` |
| A concrete resource | Domain unit tests and affected lifecycle/end-to-end cases |
| Process execution | `cargo test --locked --profile ci infra::exec::` from `cli/` |
| Wrappers or hooks | [Isolated wrapper/hook fixtures](#wrapper-and-hook-tests) |
| Desktop or application content | [Desktop shell](#desktop-shell) or [Application configuration](#application-configuration) |
| CI selection or workflows | Local `ci` stage and the changed job's command |
| Arch package recipes | [Recipe validation](package-management.md#ci-and-validation) |

Broaden coverage when a shared loader, scheduler, adapter, or failure policy
changes. Rust changes also need formatting and host Clippy; add Windows-target
Clippy for platform-sensitive code and the full Rust suite for shared
configuration, catalog, or engine changes. Use the
[pinned toolchain](contributing.md#selecting-the-pinned-toolchain).

Heading and path changes need incoming-link checks. Native process behavior
needs testing on each affected OS. Desktop rendering and session behavior need
native checks, while package builds and host-mutating integration belong in
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
use the [native docs checker](#documentation-site) on the checkout's OS.

With no stage names, the scripts run their default stages:

| Stage | What it checks |
|---|---|
| `fmt` | Rust formatting |
| `clippy` | Host Clippy for all targets, with warnings rejected |
| `test` | Full Rust test suite, not shell/PowerShell integration scripts |
| `rustdoc` | Private-item documentation and configured rustdoc lints |
| `config` | CLI configuration, source paths, and required config files |
| `docs` | Markdown links, skill metadata/fixtures, task selectors, and the VitePress build, published links, and browser regressions; Linux entrypoint only |
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
Its selector check runs a Rust fixture.
It also validates repository skill metadata and runs the validator's synthetic
regressions without third-party Python dependencies. The
[metadata validator](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/skill-maintenance/scripts/check-skills.py)
checks the supported frontmatter shape, names, and descriptions. These checks
do not measure whether an agent follows the instructions effectively.

The source link check inspects tracked and unignored new Markdown files,
including unstaged moves. It checks local inline links, not external URLs,
reference-style links, Mermaid rendering, or the truth of the prose. A quick
link-only check is:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

Generated site output and `node_modules` are ignored. New documents do not
need to be staged to check their links.

## Before pushing

Commit the intended patch, refresh its comparison base, and check the committed
snapshot rather than relying on working-tree or commit-hook results.

### Documentation and skills

Validate documentation once on the checkout's native platform, including mixed
patches. From the repository root, run:

```powershell
python docs/check.py --committed
```

Use `python3` where required. The
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks)
owns prerequisites, comparison-range selection, snapshot behavior, and remaining
coverage. Uncommitted fixes cannot satisfy this gate. Mixed patches also need
the implementation checks below; the coordinator can run documentation once
as part of that gate.

### Implementation and mixed patches

For implementation and mixed patches, run the coordinated pre-push checker
with Python 3.11 or newer. From Windows:

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

#### Local npm TLS failures

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
it omits config drift, cargo-deny, and wrapper tests. Neither invocation replaces
the [committed-patch checker](#before-pushing).

#### Sensitive-pattern scanning

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
unknown credential format. Follow [Secrets](security-and-trust.md#secrets).
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

#### Analyzer prerequisites

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

#### Isolated regressions

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

#### Full commit-hook suite

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

#### CI selection fixtures

```sh
python3 -B .github/workflows/scripts/linux/test-ci-changes.py
```

These cover classification, aggregate gating, publishing contracts,
renames/deletions, unusual paths, and invalid ranges. Temporary Git repositories
and mocked `gh` calls keep fixture commits and tags local; nothing is published.
Use `ClassificationTests GateTests` as arguments for just those cases.
Also run the local `ci` stage when workflow syntax or contracts change.

#### Release selection

Publishing compares the exact tested commit to the last published release,
not merely the previous push. See [Release selection](ci-and-releases.md#release-selection)
for why a docs-only push can recover a previously failed release.

## Focused mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Run focused mutation tests for
Rust behavior changes, especially failure handling, scheduling, and partial success.

Run `cargo-mutants` in a disposable checkout, never the shared working copy.
For a `MISSED` result, add an assertion that passes on the original code and
fails with the mutation, or establish that the mutant is equivalent. Selected
shards with survivors, timeouts, or tooling failures block CI. Ordinary tests
and `--all` do not run mutation testing.

### Prepare an isolated source snapshot

Install `cargo-mutants` only if you intend to run this check. Use the pinned
Rust toolchain and a native filesystem for the platform under test; Windows
and Linux mutations cover different runtime contracts.

Commit the source and tests you want to check first. From the primary checkout,
run the [pre-push plan](#implementation-and-mixed-patches) and record the left
SHA of its `Committed range: BASE..HEAD` line. The default planner verifies
the upstream CI baseline and can recover to a successful ancestor. If you
provided `--base`, you are responsible for that baseline's verification.

In the commands below, replace `ORIGINAL_BASE_SHA` with that full SHA.
Keep it with the mutation report and reuse it after test-only fixes. Do not
replace it with a failed push's tip or a newly advanced upstream branch.

From the primary repository root, create a detached worktree in the unused
`.mutation-review` directory:

```sh
git worktree add --detach .mutation-review HEAD
```

Only committed files are copied. Uncommitted and untracked fixes in the primary
checkout are not part of this test. Never stage the disposable directory or its
reports as part of your patch.

### Select and run the mutations

Enter `.mutation-review/cli` in a POSIX shell, or
`.mutation-review\cli` in PowerShell. Generate a source-only diff using the
recorded baseline:

```sh
git diff --relative=cli --no-renames --unified=0 --output=mutants.diff ORIGINAL_BASE_SHA HEAD -- src
cargo mutants --in-place --in-diff mutants.diff --cargo-arg=--profile=ci --cargo-arg=--locked
```

These two commands also work in PowerShell. Git writes the diff itself so
shell redirection cannot change its encoding. Check that the diff contains the
intended source changes before running the second command. A test-only recovery
should still include the original source change. An empty diff supplies no
changed-code coverage.

`--in-place` confines source mutations to this disposable worktree. To test a
specific module rather than the committed diff, substitute:

```sh
cargo mutants --in-place --file src/app/filter.rs --cargo-arg=--profile=ci --cargo-arg=--locked
```

Use `--re` to replay a specific mutant by its report description. Inspect
`mutants.out` for survivors, timeouts, tool errors, and which tests actually ran.
After adding and committing assertions in the primary checkout, preserve the
old report, remove the disposable worktree, and repeat this procedure at the
new commit with the same `ORIGINAL_BASE_SHA`.

### Preserve evidence and remove the worktree

Copy any reports you need to keep out of `.mutation-review`, along with the
tested commit and original baseline SHA. Return to the primary repository root
and inspect the disposable checkout:

```sh
git -C .mutation-review status --short
git -C .mutation-review diff
```

Make sure no unique fix or interrupted source mutation needs recovering.
After saving that work and the reports, remove only this disposable worktree:

```sh
git worktree remove --force .mutation-review
```

The force option also deletes its untracked and ignored reports and build
artifacts. It is appropriate here only because this worktree was created for
the check; never point it at the primary checkout.

CI handles changed-code selection, shards, and native-code exclusions; see
[Mutation coverage](ci-and-releases.md#platform-coverage) instead of copying that setup into
each local command. Linux checks do not provide native Windows mutation coverage.

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
process behavior. Native Windows tests are still required. The
[CI coverage matrix](ci-and-releases.md#platform-coverage) lists those boundaries.

Executor tests launch their ignored child-process fixture themselves; do not
run it manually with `--ignored`.
