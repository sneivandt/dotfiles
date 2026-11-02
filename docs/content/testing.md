---
title: Testing
description: Choose focused validation, run local checks, and understand isolation, mutation testing, and platform coverage gaps.
---

# Testing

Start with the smallest check that can prove your change. Then verify the
committed patch before pushing. A local pass is not proof of a complete machine
installation, and a skipped check is not a pass.

Commands below start at the repository root unless they explicitly enter `cli/`.
[CI and releases](ci-and-releases.md) explains what runs after a push.

## Choosing coverage

| Change | Start with | Broaden coverage when... |
|---|---|---|
| Documentation or skills | Local `docs` stage | Headings, paths, or selectors changed: check incoming references too; site changes also need a visual review |
| TOML, categories, or source layout | `config_drift` and local `config` stage | Loading, merging, or profile behavior changed |
| Task selection or dependencies | Affected command suite and `task_execution` | Shared catalog or scheduler behavior changed |
| A concrete resource | Domain unit tests and affected `lifecycle_contracts` / `e2e_apply` cases | Shared planning, adapters, or failure accounting changed |
| Process execution | `cargo test --locked --profile ci infra::exec::` from `cli/` | Timeout, cancellation, or native process behavior changed: test Windows too |
| Wrappers or hooks | [Isolated wrapper/hook fixtures](#wrapper-and-hook-tests) | Integration behavior changed: use a disposable environment |
| Desktop or application content | [Desktop shell](#desktop-shell) / [Application configuration](#application-configuration) | Rendering, focus, or session behavior needs native testing |
| CI selection or workflows | Local `ci` stage and the changed job's command | Dependencies or triggers changed: include classification fixtures |
| Arch package recipes | [Recipe validation](arch-package-recipes.md#ci-and-validation) | A real package build/install is needed: use disposable Arch |

For Rust changes, also run formatting and host Clippy. Add Windows-target
Clippy for platform-sensitive code, and the full Rust suite for shared
configuration, catalog, or engine changes. Use the
[pinned toolchain](contributing.md#selecting-the-pinned-toolchain).

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

The canonical entrypoints are
[`check.sh`](https://github.com/sneivandt/dotfiles/blob/main/.github/workflows/scripts/linux/check.sh) and
[`Check.ps1`](https://github.com/sneivandt/dotfiles/blob/main/.github/workflows/scripts/windows/Check.ps1). Select stages for
your patch rather than always running everything.

Linux:

```sh
sh .github/workflows/scripts/linux/check.sh --list
sh .github/workflows/scripts/linux/check.sh --strict docs
sh .github/workflows/scripts/linux/check.sh --strict fmt clippy test config
```

Windows:

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1 -List
pwsh -File .github\workflows\scripts\windows\Check.ps1 -Strict fmt clippy test config
```

With no stage names, the scripts run their default stages:

| Stage | What it checks |
|---|---|
| `fmt` | Rust formatting |
| `clippy` | Host Clippy for all targets, with warnings rejected |
| `test` | Full Rust test suite, not shell/PowerShell integration scripts |
| `rustdoc` | Private-item documentation and configured rustdoc lints |
| `config` | CLI configuration, source paths, and required config files |
| `docs` | Markdown links, skill metadata/fixtures, task selectors, and the Starlight build, published links, and browser regressions; Linux entrypoint only |
| `ci` | Workflow contracts, path/release classification, aggregate gating, and actionlint; Linux entrypoint only |
| `shell` | Shared ShellCheck discovery and rules |
| `powershell` | PSScriptAnalyzer at Warning/Error severity |
| `audit` / `deny` | Dependency advisories / dependency policy |

Cargo verification uses `--locked` and the `ci` profile where applicable.
The scripts use tools from `PATH`; they do not select the pinned compiler for
you. Compiler-dependent checks reject the wrong Rust version.

Missing tools usually produce `SKIP`. `--strict`, `-Strict`, or
`DOTFILES_CHECK_STRICT=1` makes skips fail. An installed tool's nonzero exit is
a failure, including exit code `2`. On Linux, missing `pwsh` skips its stage,
but present `pwsh` without PSScriptAnalyzer fails; the Windows entrypoint skips
a missing analyzer module.

Two stages are opt-in: `msrv` attempts to install and compile with the minimum
Rust version from `Cargo.toml`; Linux `qml` runs Qt Quick Test. `--all` on Linux
includes both, while Windows `-All` adds `msrv`. These options can require
downloads or additional tools; they do not run mutation tests or host integration.

The docs stage needs Python, Cargo, the Node version in `docs/.node-version`,
and the [site dependencies and Chromium runtime](#documentation-site).
Its selector check runs a Rust fixture.
It also validates repository skill metadata and runs the validator's synthetic
regressions without third-party Python dependencies. The supported frontmatter
convention and manual routing cases live in
[Skill maintenance](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/skill-maintenance/SKILL.md#check-the-result).
Metadata checks do not measure agent instruction effectiveness.

The source link check inspects tracked and unignored new Markdown files,
including unstaged moves. It checks local inline links, not external URLs,
reference-style links, Mermaid rendering, or the truth of the prose. A quick
link-only check is:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

Generated site output and `node_modules` are ignored. New documents do not
need to be staged to check their links.

## Documentation site

Install the locked dependencies and browser runtime once:

```sh
cd docs
npm ci
npx playwright install chromium
```

On a minimal Linux machine, Playwright may report missing system libraries.
Its `install --with-deps chromium` command can install those libraries, but that
is a host package change: use an authorized environment or a disposable runner.
CI uses that option on its disposable Ubuntu runner.

For site-only changes, run from the repository root:

```sh
DIR="$PWD" sh .github/workflows/scripts/linux/test-docs.sh docs_links docs_site
```

The site check runs Astro diagnostics, Markdown-transform fixtures, the static
build, matching page titles/navigation/filenames, published URL/anchor/asset,
canonical sitemap, and robots/indexability checks. Browser regressions exercise
the homepage, top-level topic navigation, search, theme persistence, warning
callouts, and every Mermaid diagram at desktop and mobile sizes. Diagram labels must stay at least
14 CSS pixels. Mobile emulation is not a native phone/browser test.

After changing the Markdown processing pipeline, run
`npm exec -- astro sync --force` from `docs/` before rebuilding. This clears
cached content so unchanged guides receive the updated transforms too.

Use `npm run dev` from `docs/` for editing. Use `npm run build` followed by
`npm run preview` when inspecting production search and rendering. The browser
suite starts and stops its own loopback preview on port 4322; keep that port free.
It uses Astro's programmatic server so a separate preview can stay open for review.
Neither preview publishes the site or applies machine configuration.

## Before pushing

Commit the intended patch, then run the manual Linux pre-push checker
(Python 3.11 or newer):

```sh
python3 -B .github/workflows/scripts/linux/pre-push.py --plan
python3 -B .github/workflows/scripts/linux/pre-push.py
```

The default range is `origin/main..HEAD`. Make sure that remote-tracking ref is
current. For another destination or base, pass `--base <remote-sha>` and
optionally `--head <commit>`. The plan prints the selected checks without running
them. An invalid range fails; an empty range reports that no checks ran.

The checker exports the selected commit into a temporary Git checkout and runs
CI-selected local checks with strict skip handling. **Unstaged fixes cannot
satisfy a committed patch.** It reuses the Cargo target cache and redirects CLI
state and logs into temporary directories.

Have the pinned compiler on `PATH` and required tools, MSRV, Windows target,
and Playwright Chromium runtime installed first; this checker does not install
them. It restores locked npm dependencies inside the isolated checkout.
Cargo and dependency checks may still need network/cache access. It is a
manual command, not an installed Git hook, and does not push.

Resolve selected local failures before publishing. Its `REMAINING CI` list
identifies native Windows, host integration, mutation, or publication work left
to CI. Report unavailable checks explicitly. After pushing, inspect the
[exact commit's aggregate and informational results](ci-and-releases.md#reading-a-run).

## Integration test suites

Rust integration targets under [`cli/tests/`](https://github.com/sneivandt/dotfiles/tree/main/cli/tests/) use fixtures.
They are different from CI's host-mutating integration jobs.

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

Combine related targets in one invocation:

```sh
cd cli
cargo test --locked --profile ci --test config_drift --test check_command
cargo test --locked --profile ci --test update_command -- --list
cargo test --locked --profile ci --test update_command repository
```

The last command filters by a test-name substring. Check the test count:
a misspelled filter can run zero tests and still exit successfully.

## Test ownership

Put a regression at the lowest boundary that proves the behavior. Use unit
tests for parsing, state transitions, resource errors, and adapters; command
tests for selection, dependency wiring, elevation policy, and exit status;
lifecycle/end-to-end fixtures for state across runs.

Shared sequential/parallel scheduler contracts live in
[`scheduler/conformance.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/scheduler/conformance.rs).
Adjacent tests cover synchronization and output ordering;
[`batch_reports.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/batch_reports.rs) and
[`parallel.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/engine/tests/parallel.rs) cover partial failure,
cancellation, and in-flight accounting.

Use [`common::cli_command`](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/mod.rs) for CLI subprocess
fixtures. It isolates home/config/cache/state/logs, clears inherited
`DOTFILES_*` and `GIT_*` overrides, sets a fixture Git boundary, and disables
self/repository updates. Use injected environments/executors and isolated
loggers for in-process tests.

[`common/lifecycle.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/common/lifecycle.rs) provides real
symlink, Git-hook, and Git-setting lifecycle fixtures. Assert exact changes,
counts, preserved state, and behavior across fresh contexts, not only a success
message. Named case tables help repeated input/rejection cases; keep separate
cases where platform contracts differ.

## CLI validation

To check this checkout using the current source, without wrapper bootstrap:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop \
  --only config-diagnostics,symlink-sources,config-files
```

These source-build examples keep `--root ..` so a custom Cargo output directory
cannot make the CLI select a different checkout.

This is the local `config` stage. It fails on configuration diagnostics, checks
configured symlink/permission sources including retained inactive definitions,
and verifies required TOML files. Structural conflicts can fail during loading,
before a check task runs.

To include external plugin and script checks:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop --verbose
```

That adds APM `pack --dry-run --verbose`, ShellCheck, and PSScriptAnalyzer where
applicable. Missing tools mean unmet work, normally shown as a local skip;
`--fail-on-skip` or a present `CI` environment variable makes unmet skips fail.
Present PowerShell without its analyzer module fails outright. Discovery and
file-read failures are not treated as empty input.

Add `--overlay /path/to/private-dotfiles` to validate combined configuration.
Keep its diagnostics private until sanitized: a public-only check cannot prove
an overlay has no conflicts.

## Dry-run testing

Preview the smallest affected selection with the current build:

```sh
cd cli
cargo run --locked --profile ci -- update --root .. --profile desktop \
  --only symlinks --dry-run --verbose
```

For removal, use `remove` instead of `update`.
Substitute `agent-packages` or another selector as appropriate.

A preview must not apply normal managed-state changes, but it can inspect the
host and write logs or selection metadata. Overlay scripts execute their own
check/preview code. Only preview trusted configuration.

Prove non-mutation using fixture assertions on files, commands, and state;
the words "dry run" in output are not proof. See
[Private overlays](security-and-trust.md#private-overlays).

## Focused mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Consider it for behavior changes,
especially failure handling, scheduling, and partial success.

Run `cargo-mutants` in a disposable checkout, never the shared working copy.
Inspect each `MISSED` result: add an assertion for observable behavior or explain why the mutation
is equivalent. Ordinary tests and `--all` do not run mutation testing.

With `cargo-mutants` installed, select the affected module in that disposable
checkout. For example:

```sh
cd cli
cargo mutants --file src/app/filter.rs --cargo-arg=--profile=ci --cargo-arg=--locked
```

Use `--in-diff` with a unified Git diff for changed-line coverage. Record any
narrowed scope and inspect `mutants.out` before cleaning up. CI handles its
changed-code selection, shards, and native-code exclusions; see
[Mutation coverage](ci-and-releases.md#platform-coverage) instead of copying that setup into
each local command. Linux checks do not provide native Windows mutation coverage.

## Wrapper and hook tests

### Isolated regressions

These use fixture tools rather than bootstrapping or installing the real CLI:

```sh
sh .github/workflows/scripts/linux/test-hook-inputs.sh \
  test_staged_ci_guards test_ci_change_classification test_staged_ci_guard_deletions
sh .github/workflows/scripts/linux/test-shell-wrapper.sh \
  test_wrapper_preserves_runtime_context test_wrapper_uses_cargo_artifact
```

With no selector, `test-hook-inputs.sh` runs all input regressions. Its build
metadata case needs the pinned Rust compiler; without it, that case skips.

On Windows, dot-source the wrapper suite to load only the isolated functions:

```powershell
. .\.github\workflows\scripts\windows\Test-ShellWrapper.ps1
Test-IsolatedWrapperPath
Test-CargoArtifactPath
```

Full wrapper suites can build/copy binaries or use `BINARY_PATH`. Read their
setup before running all cases.

### Checking your real index

These inspect staged content and do not create commits:

```sh
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
sh hooks/check-ci-guards.sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

The Rust and CI guards export the index and reuse caches without stashing or
overwriting working files. Full mode remains staged-change-aware; it is not
committed-range validation. See [Git hooks](git-hooks.md).

### Full commit-hook suite

**Never run `test-git-hooks.sh` in your developer checkout.** It creates real
commits. A clean tree is not a safe fixture; do not use its force override.

The full suite runs in CI's disposable checkout. Prefer the isolated local
regressions above; use a disposable repository and home if you need full
integration coverage. Do not reuse a failed fixture, because its existing
commits can make a later check see an empty diff.

## Cleanup retention and Git templates

```sh
python3 -B .github/workflows/scripts/linux/test-maintenance.py
```

These fixtures check merge/rebase cleanup, protected branches, worktrees, and
file/link/directory retention in temporary repositories and directories. They
do not start services or delete real user files.

## Desktop shell

QML tests need Qt 6.8 or newer Quick Test and use inert Quickshell mocks:

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

## Application configuration

These use isolated homes and mock commands:

```sh
python3 -B .github/workflows/scripts/linux/test-shell-config.py
python3 -B -m unittest discover -s symlinks/vim/tests -p 'test_*.py'
pwsh -NoProfile -File symlinks/config/powershell/tests/Test-Prompt.ps1
```

The PowerShell fixture covers first-start discovery, custom Cargo paths,
literal profile-fragment directories, and PATH deduplication on reload.
Install required native tools; a missing-tool skip gives no coverage.

## CI gates

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

## Platform coverage parity

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
