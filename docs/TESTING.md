# Testing and validation

Start with the smallest check that can prove your change. Then verify the
committed patch before pushing. A local pass is not proof of a complete machine
installation, and a skipped check is not a pass.

Commands below start at the repository root unless they explicitly enter `cli/`.
[Test recipes](TESTING-RECIPES.md) contains longer fixture setups;
[CI and releases](CI.md) explains what runs after a push.

## Choosing coverage

| Change | Start with | Broaden coverage when... |
|---|---|---|
| Documentation or skills | Local `docs` stage | Headings, paths, or selectors changed: check incoming references too |
| TOML, categories, or source layout | `config_drift` and local `config` stage | Loading, merging, or profile behavior changed |
| Task selection or dependencies | Affected command suite and `task_execution` | Shared catalog or scheduler behavior changed |
| A concrete resource | Domain unit tests and affected `lifecycle_contracts` / `e2e_apply` cases | Shared planning, adapters, or failure accounting changed |
| Process execution | `cargo test --locked --profile ci infra::exec::` from `cli/` | Timeout, cancellation, or native process behavior changed: test Windows too |
| Wrappers or hooks | [Isolated wrapper/hook fixtures](TESTING-RECIPES.md#wrapper-and-hook-tests) | Integration behavior changed: use a disposable environment |
| Desktop or application content | [Desktop shell](#desktop-shell) / [Application configuration](#application-configuration) | Rendering, focus, or session behavior needs native testing |
| CI selection or workflows | Local `ci` stage and the changed job's command | Dependencies or triggers changed: include classification fixtures |
| Local Arch recipes | [Local-package validation](LOCAL-PACKAGES.md#ci-and-validation) | A real package build/install is needed: use disposable Arch |

For Rust changes, also run formatting and host Clippy. Add Windows-target
Clippy for platform-sensitive code, and the full Rust suite for shared
configuration, catalog, or engine changes. Use the
[pinned toolchain](CONTRIBUTING.md#selecting-the-pinned-toolchain).

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
[`check.sh`](../.github/workflows/scripts/linux/check.sh) and
[`Check.ps1`](../.github/workflows/scripts/windows/Check.ps1). Select stages for
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
| `docs` | Local Markdown links/heading anchors and the static task-selector catalog; Linux entrypoint only |
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

The docs stage needs Python and Cargo: its selector check runs a Rust fixture.
It checks **tracked** Markdown files and local inline links, not external URLs,
reference-style links, Mermaid rendering, or the truth of the prose. A quick
link-only check is:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

New documents must be staged before this tracked-file check can inspect their
outgoing links.

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

Have the pinned compiler on `PATH` and required tools, MSRV, and Windows target
installed first; this checker does not install them. Cargo and dependency
checks may still need network/cache access. It is a manual command, not an
installed Git hook, and does not push.

Resolve selected local failures before publishing. Its `REMAINING CI` list
identifies native Windows, host integration, mutation, or publication work left
to CI. Report unavailable checks explicitly. After pushing, inspect the
[exact commit's aggregate and informational results](CI.md#reading-a-run).

## Integration test suites

Rust integration targets under [`cli/tests/`](../cli/tests/) use fixtures.
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
[`scheduler/conformance.rs`](../cli/src/engine/tests/scheduler/conformance.rs).
Adjacent tests cover synchronization and output ordering;
[`batch_reports.rs`](../cli/src/engine/tests/batch_reports.rs) and
[`parallel.rs`](../cli/src/engine/tests/parallel.rs) cover partial failure,
cancellation, and in-flight accounting.

Use [`common::cli_command`](../cli/tests/common/mod.rs) for CLI subprocess
fixtures. It isolates home/config/cache/state/logs, clears inherited
`DOTFILES_*` and `GIT_*` overrides, sets a fixture Git boundary, and disables
self/repository updates. Use injected environments/executors and isolated
loggers for in-process tests.

[`common/lifecycle.rs`](../cli/tests/common/lifecycle.rs) provides real
symlink, Git-hook, and Git-setting lifecycle fixtures. Assert exact changes,
counts, preserved state, and behavior across fresh contexts, not only a success
message. Named case tables help repeated input/rejection cases; keep separate
cases where platform contracts differ.

## CLI validation

To check this checkout using the current source, without wrapper bootstrap:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop \
  --only config-warnings,symlink-sources,config-files
```

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
  --no-repo-update --only symlinks --dry-run --verbose
```

For removal, use `remove` instead of `update` and omit `--no-repo-update`.
Substitute `apm` or another selector as appropriate.

A preview must not apply normal managed-state changes, but it can inspect the
host and write logs or selection metadata. Overlay scripts execute their own
check/preview code. Only preview trusted configuration.

Prove non-mutation using fixture assertions on files, commands, and state;
the words "dry run" in output are not proof. See
[Private overlays](SECURITY.md#private-overlays).

## Focused mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Consider it for behavior changes,
especially failure handling, scheduling, and partial success.

Run `cargo-mutants` in a disposable checkout, never the shared working copy.
The [mutation recipe](TESTING-RECIPES.md#focused-mutation-testing) covers a
committed diff and explains native-code exclusions. Inspect each `MISSED`
result: add an assertion for observable behavior or explain why the mutation
is equivalent. Ordinary tests and `--all` do not run mutation testing.

## Wrapper and hook tests

Start with [isolated wrapper and hook recipes](TESTING-RECIPES.md#wrapper-and-hook-tests).
They use fixture tools rather than installing the CLI. The full commit-hook
suite creates real commits and must use the separate disposable-repository
recipe. [Hooks](HOOKS.md) explains checks against your real staged index.

## Cleanup retention and Git templates

Run the [maintenance fixtures](TESTING-RECIPES.md#cleanup-retention-and-git-templates)
to test cleanup and Git templates without starting services or deleting real
user files.

## Desktop shell

Use the [desktop recipes](TESTING-RECIPES.md#desktop-shell) for Quickshell QML,
Python helpers, lock scripts, and stock-cache concurrency. QML tests need
Qt 6.8 or newer Quick Test and use inert Quickshell mocks.

Offscreen tests cover logic/layout, not native rendering, focus, or a live
compositor/network session. Record that manual coverage separately.

## Application configuration

[Application recipes](TESTING-RECIPES.md#application-configuration) cover shell,
Vim, and PowerShell fixtures using isolated homes and mock commands. Required
native tools still need to be installed; a missing-tool skip gives no coverage.

## CI gates

The `ci-success` gate checks that selected jobs succeeded and unselected jobs
were intentionally skipped. See [CI and releases](CI.md) for scheduling and
informational jobs; do not interpret an omitted job as a pass.

### Release selection

Publishing compares the exact tested commit to the last published release,
not merely the previous push. See [Release selection](CI.md#release-selection)
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
[CI coverage matrix](CI.md#platform-coverage) lists those boundaries.

Executor tests launch their ignored child-process fixture themselves; do not
run it manually with `--ignored`.
