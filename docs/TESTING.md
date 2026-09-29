# Testing and validation

Use this page to choose a check, run it against the intended source, and state
what it did **not** cover. Commands start at the repository root unless a block
explicitly changes directory. Prefer a focused check over a full machine
installation.

## Choosing coverage

| Change | Start with | Widen when |
|---|---|---|
| Documentation or skills | `sh .github/workflows/scripts/linux/check.sh docs` | Task names, file locations or headings changed: inspect incoming references too |
| TOML/category/source layout | `config_drift` and the local `config` stage | Loader/merge/profile behavior changed: add command tests and the full Rust suite |
| Task selection or dependencies | Affected command suite, `task_execution` | Shared catalog/engine behavior changed: scheduler contracts and full suite |
| Concrete resource | Its domain unit tests and `lifecycle_contracts` or affected `e2e_apply` cases | Shared planning, error accounting or adapters changed |
| Process execution | `cargo test --profile ci infra::exec::` from `cli/` | Platform process-tree, timeout or capture behavior changed: native Windows too |
| Wrapper or hook | Focused fixtures under [Wrapper and hook tests](#wrapper-and-hook-tests) | Full wrapper/hook suites only in a disposable environment |
| Managed desktop/shell content | [Desktop shell](#desktop-shell) regressions | Native rendering/session behavior changed: manual testing on the relevant desktop |
| Workflow/CI classification | Local `ci` stage and the changed job's exact command | Trigger or dependency changes: classification/hook-input regressions |

For Rust changes, add formatting and host Clippy to the affected tests; consider
cross-target Clippy for Windows-sensitive code. Broaden coverage for shared
configuration, engine or catalog changes, not merely because a file is Rust.

### Isolation and side effects

There are three different kinds of check:

- **Fixture tests:** Rust integration/unit tests and targeted wrapper/hook-input
  regressions use controlled roots, homes or injected tools. They can create
  files, Git repositories and subprocesses inside their fixtures. CI-selection
  tests also create fixture commits/tags and mock `gh`; they do not publish a
  release or commit into the developer checkout.
- **Repository checks:** formatting, linters and `dotfiles check` inspect actual
  source/configuration. Cargo writes build caches; dependency audits can use
  the network. The CLI can write run logs and persist profile/overlay choices
  even though `check` does not apply managed resources.
- **Host integration:** install/uninstall and application jobs, and the full
  commit-hook suite, intentionally mutate their environment. Use a disposable
  checkout **and** a disposable home/runner/VM as appropriate. A clean Git tree
  does not make your real home safe.

Do not run an install, package update, elevation request or desktop restart as
a substitute for missing tests. Do not invoke a wrapper to validate changed
Rust: it may bootstrap a release or reuse an older binary.

## Fast local sequence

The canonical entrypoints are
[`linux/check.sh`](../.github/workflows/scripts/linux/check.sh) and
[`windows/Check.ps1`](../.github/workflows/scripts/windows/Check.ps1).
They share CI's Cargo profile, but **do not run the entire CI workflow**.

```bash
sh .github/workflows/scripts/linux/check.sh --list
sh .github/workflows/scripts/linux/check.sh fmt clippy test config
sh .github/workflows/scripts/linux/check.sh
```

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1 -List
pwsh -File .github\workflows\scripts\windows\Check.ps1 fmt clippy test config
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

With no stage names, each script runs its default stages:

| Stage | Actual scope | Availability caveat |
|---|---|---|
| `fmt` | `cargo fmt --check` | Requires Cargo and rustfmt |
| `clippy` | Host `cargo clippy --profile ci --all-targets -- -D warnings` | Does not include cross-target Clippy |
| `test` | `cargo test --profile ci` | Does not run shell/PowerShell CI integration scripts |
| `config` | CLI `check --profile desktop --only config-warnings,symlink-sources,config-files` | Intentionally excludes external APM/linters |
| `docs` | Relative inline Markdown links/heading anchors and static task-selector inventory | Linux entrypoint only |
| `ci` | CI/release scheduling contracts, changed-path and release-baseline fixtures, and aggregate result-gate behavior | Linux entrypoint only; requires Python 3 plus Git/POSIX shell for fixtures |
| `shell` | Shared ShellCheck file discovery in `test-static-analysis.sh` | Requires ShellCheck; Windows also needs POSIX `sh` |
| `powershell` | Repository PowerShell analysis at Warning/Error severity | See the different missing-module behavior below |
| `audit` | `cargo audit` for `cli/Cargo.lock` | Requires cargo-audit |
| `deny` | `cargo deny check all` | Requires cargo-deny |

Missing top-level tools generally produce `SKIP` without failing the script.
Read the summary: **“All checks passed” can include skipped stages.** Installed
Cargo with a missing/broken subcommand can still fail rather than skip.
On Linux, absent `pwsh` skips `powershell`, but present `pwsh` without
PSScriptAnalyzer fails. The Windows entrypoint explicitly skips a missing
PSScriptAnalyzer module.

The opt-in `msrv` stage reads `rust-version` from `cli/Cargo.toml`, attempts to
install that compiler, then runs `cargo +<msrv> check --all-targets`. It is a
compile check, not a second full test suite. Request it explicitly, or use
`--all` on Linux / `-All` on Windows. Those options can download a toolchain;
they still do not enable host integration, coverage or mutation testing.

Run individual Rust checks from `cli/` so the repository's pinned
[`rust-toolchain.toml`](../cli/rust-toolchain.toml) applies:

```bash
cd cli
cargo fmt --check
cargo clippy --profile ci --all-targets -- -D warnings
cargo test --profile ci
```

The docs checker validates local inline links, not external URLs or the
correctness of prose. It compares [TASKS.md](TASKS.md) with static selectors in
code; dynamic overlay selectors are documented by convention.

The `ci` stage runs both
[`check-ci-contract.py`](../.github/workflows/scripts/linux/check-ci-contract.py)
and [`test-ci-changes.py`](../.github/workflows/scripts/linux/test-ci-changes.py).
To focus on the latter's pure classification and result-gate cases without
creating Git fixtures:

```bash
python3 -B .github/workflows/scripts/linux/test-ci-changes.py \
  ClassificationTests GateTests
```

Omit the class names to include rename/deletion, unusual-path, invalid-range and
published-release-baseline fixtures. Those fixtures need Git and `sh`, use a
mock GitHub CLI, and make no release API calls.

## Integration test suites

Rust integration targets under [`cli/tests/`](../cli/tests) are distinct from
the host-mutating CI integration jobs:

| Target | Use it for |
|---|---|
| `config_drift` | Real configuration/source/category invariants |
| `domain_boundaries` | Allowed architectural dependencies and platform/environment boundaries |
| `install_command`, `uninstall_command`, `test_command` | Command task sets, selection, loading and outcomes (`test_command` tests `check`) |
| `task_execution` | Filesystem-backed task/resource convergence and dry-run |
| `task_output` | Visible statuses, reasons, logs and exit behavior |
| `lifecycle_contracts` | Shared real-task dry-run, apply/repeat, retry and conservative-removal contracts |
| `e2e_apply` | Representative CLI convergence against controlled state |
| `behavioral_ci` | Cross-cutting regressions supporting CI assumptions |

Combine related targets in one invocation:

```bash
cd cli
cargo test --profile ci --test config_drift --test test_command
cargo test --profile ci --test lifecycle_contracts
cargo test --profile ci --test install_command -- --list
```

To focus further, append a substring of the test name before `--`, for example
`cargo test --profile ci --test install_command repository`. Check the reported
test count: a misspelled filter can run zero tests successfully.

## Test ownership

Put the regression at the lowest boundary that can prove the behavior:

- Unit tests: parsing, validation, state transitions, resource errors and
  process adapters.
- [`scheduler/conformance.rs`](../cli/src/engine/tests/scheduler/conformance.rs):
  shared sequential/parallel scheduler contracts. Adjacent `parallel.rs` and
  `output.rs` own synchronization-specific and stage/result-ordering cases.
- [`batch_reports.rs`](../cli/src/engine/tests/batch_reports.rs) and
  [`parallel.rs`](../cli/src/engine/tests/parallel.rs): partial failure,
  cancellation, discovery errors and in-flight accounting.
- Command suites: selection, dependency wiring, elevation/re-exec policy and
  exit status.
- Lifecycle/end-to-end fixtures: externally observable state across runs.

[`common::cli_command`](../cli/tests/common/mod.rs) isolates home, config,
cache, state and logs; clears inherited `DOTFILES_*`/`GIT_*` overrides; pins
repository discovery to a fixture Git boundary; and disables self/repository
updates. Use it for subprocess tests instead of rebuilding a partly isolated
command. In-process tests should use injected environment/executors and an
isolated logger.

[`common/lifecycle.rs`](../cli/tests/common/lifecycle.rs) drives real symlink,
Git-hook and Git-setting tasks on fixtures. Preserve assertions about exact
logical actions/counts, managed state, fresh contexts between runs, and safe
repeat/removal behavior. Do not reduce a lifecycle test to “the output contains
this message.”

Use named case tables for repeated input/rejection cases. Test observable
contracts rather than trivial derives or field copies; retain separate cases
where platform behavior differs.

## CLI validation

To validate the current Rust implementation against this checkout without
bootstrap:

```bash
cd cli
cargo run --profile ci -- check --root .. --profile desktop \
  --only config-warnings,symlink-sources,config-files
```

This is the same selection as the local `config` stage. It treats configuration
diagnostics as failure, checks configured symlink and permission sources
(including retained inactive definitions), and verifies required TOML files.
Structural/conflicting configuration can fail before any check task runs.

For all checks, use the built CLI or Cargo:

```bash
cd cli
cargo run --profile ci -- check --root .. --profile desktop --verbose
```

The additional tasks run APM's `pack --dry-run --verbose` on discovered local
plugins, ShellCheck, and PSScriptAnalyzer. They are not interchangeable with
the local scripts' lint stages; see
[`validation/checks.rs`](../cli/src/app/validation/checks.rs) for discovery scope.

Missing `apm`, `shellcheck` or `pwsh` is **unmet work**, normally displayed as a
skip locally. `--fail-on-skip` or any present `CI` environment variable makes
unmet skips fail. A present `pwsh` without its
analyzer module fails outright. Explicitly select/skip tools that are outside
your intended check; do not report omitted checks as passes.
Linter discovery and file-read failures also fail the check rather than silently
omitting inputs. Missing optional input directories remain acceptable.

When changing an overlay, validate the combined configuration with
`--overlay /path/to/private-dotfiles` on that same command. Keep diagnostics and
paths private until sanitized. A successful public-only check says nothing
about overlay conflicts.

## Dry-run testing

Preview the smallest affected task set with the **current build**:

```bash
cd cli
cargo run --profile ci -- install --root .. --profile desktop \
  --no-repo-update --only symlinks --dry-run --verbose
cargo run --profile ci -- install --root .. --profile desktop \
  --no-repo-update --update --only apm --dry-run --verbose
cargo run --profile ci -- uninstall --root .. --profile desktop \
  --only symlinks --dry-run --verbose
```

These previews must not apply managed filesystem, package, registry, unit or
generated-manifest changes. They can still inspect the host, create logs and
persist selection metadata. Prove absence of mutations with fixture assertions,
not the presence of the words “dry run.”

Only preview trusted configuration. Overlay scripts run check/preview code and
must honor their flags; dry-run is not a sandbox. See
[Private overlays](SECURITY.md#private-overlays).

## Wrapper and hook tests

The [Linux scripts](../.github/workflows/scripts/linux) and
[Windows scripts](../.github/workflows/scripts/windows) include both isolated
regressions and CI host integration. Read a suite before running all its cases.
Full wrapper suites may build/copy binaries or use a supplied `BINARY_PATH`.

### Focused, isolated regressions

These run fixture tools rather than bootstrap/install the real CLI:

```bash
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_staged_ci_guards
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_ci_change_classification
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_staged_ci_guard_deletions
sh .github/workflows/scripts/linux/test-shell-wrapper.sh \
  test_wrapper_preserves_runtime_context test_wrapper_uses_cargo_artifact
```

`test_ci_change_classification` delegates to the complete Python CI-selection
suite described above, including its isolated Git/release-baseline fixtures.
`test-hook-inputs.sh` without a selector runs all its input regressions,
including `test_build_version_ref_triggers`. That case compiles the std-only
`cli/build.rs` directly, without Cargo dependencies, to check branch-ref
invalidation of build metadata. It skips if the pinned Rust compiler is
unavailable and also runs explicitly in the Linux CI build job.

On Windows, dot-source the suite to load functions without executing all cases:

```powershell
. .\.github\workflows\scripts\windows\Test-ShellWrapper.ps1
Test-IsolatedWrapperPath
Test-CargoArtifactPath
```

### Checks on your real index

These do not create commits, but inspect **what is staged**, not all edits:

```bash
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
sh hooks/check-ci-guards.sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

The Rust and CI guards export the staged index and reuse the checkout's Cargo
cache; they do not stash, overwrite or stage working-tree content. Helper code
and sensitive-pattern configuration themselves are read from the checkout.
Full mode is staged-change-aware, not a replacement for the full local/CI
sequence. See [Hooks](HOOKS.md) for triggers and skipped tools.

### Full commit-hook suite: disposable repository only

[`test-git-hooks.sh`](../.github/workflows/scripts/linux/test-git-hooks.sh)
creates **real commits in its current repository**. It rejects tracked
uncommitted changes, but that guard is not permission to use a clean developer
checkout. A failed detection case can commit its fixture and anything else
staged. Never use its force override to test your working repository.

One fixture setup, from the source checkout (use a fresh directory each time):

```bash
(
  source_root="$PWD"
  fixture="$PWD/.hook-test-repo"
  test ! -e "$fixture" || exit 1
  mkdir -p "$fixture/hooks"
  cp -R hooks/. "$fixture/hooks/"
  cd "$fixture" || exit 1
  unset GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE
  unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
  unset GIT_CONFIG GIT_CONFIG_PARAMETERS
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_COUNT=0
  git init -q --template=
  git config core.hooksPath .git/hooks
  git config user.name "Hook Test"
  git config user.email "ci@test.local"
  git add hooks
  git commit -qm baseline
  cp hooks/pre-commit .git/hooks/pre-commit
  chmod +x .git/hooks/pre-commit
  DIR="$source_root" sh "$source_root/.github/workflows/scripts/linux/test-git-hooks.sh"
)
```

Inspect and remove only `.hook-test-repo` afterward; never stage it. Do not
reuse a failed fixture: its committed test data can turn the next run into an
empty diff and give misleading results.

## Desktop shell

These checks do not require installing the managed configuration.

Quickshell layout, queued-refresh, and process-lifecycle QML regressions use
Qt 6.8 or newer Quick Test. On Arch:

```bash
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML_DISABLE_DISK_CACHE=1 \
  /usr/lib/qt6/bin/qmltestrunner \
  -import symlinks/config/quickshell/tests/qml/mocks \
  -input symlinks/config/quickshell/tests/qml
```

The mock import is required: it supplies inert Quickshell process and window
types, so no installed Quickshell runtime or live services are used.

Network/power and desktop-script regressions mock actions; they do not toggle
adapters, lock the desktop, capture the screen or power off the machine:

```bash
python3 -B -m unittest discover \
  -s symlinks/config/quickshell/tests/python -p 'test_*.py'
python3 -B -m unittest discover \
  -s symlinks/config/hypr/scripts/tests -p 'test_*.py'
pwsh -NoProfile -File symlinks/config/powershell/tests/Test-Prompt.ps1
```

On Windows use `python -B` instead of `python3 -B`. The prompt test starts an
isolated PowerShell host without the installed profile and covers argument
forwarding, completion, home abbreviation, root detection and exit status.
The native GLib case
skips when its runtime is unavailable; mocked Python cases still run. When their
inputs change, CI selects the affected Python/PowerShell regression steps on
Linux and Windows, **not** the QML runner or a native desktop session.

Shell startup, completion, utility failure handling and download regressions
use isolated homes, mock commands and a loopback-only HTTP server. Vim
regressions run without plugin installation:

```bash
python3 -B .github/workflows/scripts/linux/test-shell-config.py
python3 -B -m unittest discover -s symlinks/vim/tests -p 'test_*.py'
sh .github/workflows/scripts/linux/test-stocks.sh
```

These Linux checks require their native tools (including Bash, Zsh, wget and
Vim). Missing-tool skips are not passes. CI runs the shell and Vim suites in
their application jobs; stock-cache concurrency also has its own job.

## CI gates

[`ci.yml`](../.github/workflows/ci.yml) starts for pushes/PRs to `main` and manual
dispatch without workflow-level path filters that could leave required checks
pending. [`classify-ci-changes.sh`](../.github/workflows/scripts/linux/classify-ci-changes.sh)
delegates selection to the standard-library-only
[`Python classifier`](../.github/workflows/scripts/linux/classify-ci-changes.py).
Documentation checks, scheduling-contract checks and classifier regressions
remain always-on, including for documentation-only changes.

The `ci-success` job runs with `always()` and depends on every gating job.
[`check-ci-contract.py`](../.github/workflows/scripts/linux/check-ci-contract.py)
validates workflow dependencies, selection outputs, matrix wiring and release
conditions. In `--results` mode it also checks actual outcomes: classification
must succeed, every **selected** job must succeed, and every **unselected** job
must be skipped. Failure, cancellation, missing outputs and accidental skips
cannot masquerade as intentional omissions. A deliberately unselected job still
provides no coverage for that run.

### Change-aware scheduling

Selection is per input, platform, linter and application—not one global
“code changed” switch. This table summarizes the policy; the classifier and its
regressions own the exact path rules. Linter selections add to other applicable
checks rather than replacing them.

| Changed inputs | Selected work beyond the always-on checks |
|---|---|
| Documentation, agent guidance, known repository/editor metadata | No compilation; these changes alone are not binary-release inputs |
| Rust source, embedded source-tree assets, Cargo/build/toolchain configuration | Both platform builds, Rust checks, profile/round-trip integration, wrappers, and application tests |
| Rust integration tests and fixtures | Both platform builds and Rust checks, but no unrelated application/wrapper tests or new binary-release input |
| Cargo manifests/lockfiles | Dependency audit and deny checks in addition to Rust checks |
| `cli/deny.toml` / `cli/rustfmt.toml` | Only dependency-policy / formatting checks, respectively |
| `conf/` | Configuration validation, drift tests, and profile/round-trip integration; application tests for package, symlink, or Git config changes |
| `system/` | Linux configuration and profile/round-trip checks |
| Managed application configuration | Configuration validation/drift checks and affected Git, Zsh, Vim, or Neovim application entries |
| PowerShell prompt, session lock, Quickshell | Configuration checks plus the affected managed-script regressions on Linux and Windows |
| Stock helper | Configuration checks and the isolated stock regression |
| Wrapper or platform-specific integration script | Its platform's build and relevant integration job |
| Hooks | Hook and isolated staged-input regressions; hook-only changes do not request CLI builds |
| `.sh` / `.ps1` / `.psm1` files | Add the corresponding linter; the extensionless `hooks/pre-commit` also selects ShellCheck |
| CI workflow, shared CI helpers, classifier, manual dispatch, or unavailable comparison range | Conservative full CI; unknown inputs also use this fallback |
| Release workflow or release-selection script | Publishing selection without unrelated CI compilation |

Changed paths include deletions and both sides of renames. An invalid Git range
fails classification rather than silently skipping checks. A valid empty diff
runs only the always-on checks. Manual/missing-range full CI is a separate
fallback; it does not itself request publication or mutation testing.

Both build jobs run Clippy and the full Rust suite only when the classifier
requests Rust checks: Rust inputs or the conservative full-CI fallback.
On Linux, configuration-only changes run `config_drift` instead. Cargo
integration tests produce the executable uploaded for downstream jobs; other
artifact consumers use `cargo build`. A wrapper-only change builds only its
platform. Configuration checks compile the current CLI where needed; they do
not substitute a potentially incompatible downloaded release.

Coverage still has deliberate limits:

- Profile matrices use `base` and `desktop`, skip VS Code in dry-run, and skip
  external linter/APM tasks in `check`. Those tools have separate or local
  coverage; this is not proof that every installer works on a clean machine.
- Windows application integration skips APM, packages and VS Code and tests
  Git's effective configuration. Linux application jobs cover Git, zsh, Vim
  and Neovim.
- Linux/Windows `cargo llvm-cov` HTML reports are informational and excluded
  from `ci-success`.
- Eight mutation-test shards cover the complete changed-code mutant set using
  the `ci` profile for PRs/main pushes that change Rust source under `cli/src/`.
  They are informational and excluded from `ci-success`. Lockfile-only,
  standalone-test-only and CI-only changes do not select mutation shards.

### Release selection

After successful same-repository push CI on `main`, a read-only job runs
[`classify-release.sh`](../.github/workflows/scripts/linux/classify-release.sh)
to compare the exact tested commit with the latest non-draft, non-prerelease
publication returned by GitHub.
Release builds, version allocation, attestations, and publishing run only when
binary inputs or the publishing pipeline changed. Documentation, configuration,
wrappers, standalone test fixtures, and lint-policy changes do not by themselves
publish a new binary. Changes inside `cli/src/` are conservatively treated as
binary inputs, including embedded assets and inline unit tests. Unknown inputs
also conservatively select publication; full CI by itself does not.

Comparing with the published release rather than the previous push catches binary
changes left unpublished by a failed or cancelled run. Consequently, a docs-only
push can recover a pending binary release, but never causes an otherwise redundant
one. With no published release, an initial release is built. Already-released or
older commits are skipped; API errors, missing release tags, and divergent release
history fail explicitly. Release asset names, checksums, provenance, and the
`vYYYY.MM.DD-N` version format are unchanged.

## Platform coverage parity

| Boundary | Linux CI | Windows CI | What remains unproved |
|---|---|---|---|
| Rust build/Clippy/tests | Yes | Yes, subject to selection above | Other architectures' runtime behavior |
| `base`/`desktop` preview and config check | Yes | Yes | Real external installs omitted by those jobs |
| Install/uninstall round-trip | Yes | Yes | Every possible pre-existing user configuration |
| Wrapper integration | POSIX wrapper | PowerShell wrapper | All network/authentication environments |
| Applications | Git, zsh, Vim, Neovim | Git | Full Windows package/VS Code installation |
| Managed Python/PowerShell scripts | Yes | Yes | Native compositor/network/lock session |
| QML Quick Test | No (local command above) | No | Rendered desktop integration |
| Commit-hook sensitive scan | Yes | No native hook job | Git-for-Windows hook runtime |
| ShellCheck/PSScriptAnalyzer, audit/deny/MSRV | Linux jobs | No duplicate jobs | Native behavior beyond static/compile checks |

From Linux, check Windows-gated Rust paths without claiming runtime coverage:

```bash
cd cli
cargo clippy --profile ci --target x86_64-pc-windows-gnu \
  --all-targets -- -D warnings
```

Record a missing target/compiler/toolchain as an omitted check. Native Windows
tests are still required for registry, elevation, symlink/junction, path and
process behavior. Executor tests include timeout, cancellation and output
draining after a process leader exits; their ignored child fixture is launched
by the tests and is **not** a standalone `--ignored` test to run manually.
