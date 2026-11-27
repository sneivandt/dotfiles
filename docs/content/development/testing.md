---
title: Testing
description: Select and run existing checks for a patch, from focused commands through committed verification and native-platform coverage.
---

# Testing

Pick and run the smallest checks that cover what you changed, then verify the
committed patch before pushing. To write a new regression or fixture, see
[Writing tests](writing-tests.md); to see what runs remotely after a push, read
[CI and releases](ci-and-releases.md).

Focused local checks can be your final local gate before pushing: there is no
mandatory full-suite run or second coordinator pass, and commit hooks stay
lightweight. Reuse passing results while their inputs, platform, command, and
toolchain are unchanged, and rerun when any of those change. A local pass never
proves a full host install, and a skipped check never counts as a pass.

Unless noted otherwise (or preceded by a `cd`), run every command below from
the repository root.

## Choosing coverage

| Change | Start with |
|---|---|
| Skills, agent instructions, or ordinary prose | Accuracy review plus affected metadata and source-link checks; skip Rust, site build, and browser runs |
| Docs routes, headings, rendering, navigation, search, or dependencies | Affected [site checks](#documentation-site); run build or browser tests only where they exercise the change |
| TOML, categories, or source layout | `config_drift` and local `config` stage |
| Task selection or dependencies | Affected command suite and `task_execution` |
| A concrete resource | Domain unit tests and affected lifecycle/end-to-end cases |
| Process execution | `cargo test --locked --profile ci infra::exec::` from `cli/` |
| Wrappers or hooks | [Wrapper and hook tests](#wrapper-and-hook-tests) |
| Desktop or application content | [Desktop shell](#desktop-shell) or [Application configuration](#application-configuration) |
| CI selection or workflows | Local `ci` stage and the changed job's command |
| Arch package recipes | Offline Linux recipe tests, config validation/drift, and PKGBUILD/install-script ShellCheck; verify upstream availability and native build/install in a disposable Arch home/cache |

Look at both the changed files and what tests actually read: a test can depend
on a shared helper, embedded asset, real config file, or snapshot even when its
own file was untouched (include deletions and both sides of renames). A
Markdown file embedded in the CLI is a CLI input, not ordinary prose. Broaden
your selection whenever impact is unclear or a focused check fails.

Rust edits usually need formatting, host Clippy, and the owning unit or
integration tests under the
[pinned toolchain](contributing.md#selecting-the-pinned-toolchain). Broaden to
the full Rust suite for shared config, catalog, scheduler, execution, or
lifecycle changes. Test-only edits generally need only the affected target;
Cargo or build edits need relevant compilation and dependency checks (add MSRV
checks when syntax, APIs, dependencies, or the MSRV itself could shift). Check
[Platform coverage parity](#platform-coverage-parity) to decide whether
Windows cross-target Clippy applies.

Before running checks, note briefly what changed, which checks you selected,
and what remains for CI.

### Isolation and side effects

| Kind of check | What it can change |
|---|---|
| Fixture tests | Temporary files, repositories, and subprocesses within controlled roots |
| Repository checks | Build caches and sometimes network access; CLI checks can write logs and selection metadata |
| Host integration | Real home files, packages, services, registry values, or Git history |

Never run live `update`, `remove`, package upgrades, elevation, or desktop
restarts to fill a test gap. Full install/remove and application integration
belong in disposable homes, runners, or VMs. The full commit-hook suite also
creates real commits, so a clean developer checkout is **not** a safe fixture.

## Fast local sequence

### Targeted Rust tests

Run Cargo from `cli/` with the
[pinned toolchain](contributing.md#selecting-the-pinned-toolchain), combining
related integration targets into one command:

```sh
cd cli
cargo test --locked --profile ci --test config_drift --test integration check_command
```

To narrow down one failure, list the target's tests before picking a filter:

```sh
cd cli
cargo test --locked --profile ci --test integration update_command -- --list
cargo test --locked --profile ci --test integration update_command::
```

Filters match test-name substrings, so check the reported test count (a typo
matches zero tests and still exits `0`). See the
[suite inventory](writing-tests.md#integration-test-suites) to locate a target,
or [Debugging](debugging.md) to trace a failure.

### Stage-based checks

The repository's unified cross-platform check and test runner is
[check.py](https://github.com/sneivandt/dotfiles/blob/main/check.py) at the
repository root. Pick the stages relevant to your patch, or use `--tests` /
`--lint` / `--all` to run stage groups.

```sh
python3 -B check.py --list
python3 -B check.py --strict fmt clippy test config
python3 -B check.py --tests
python3 -B check.py --lint
```

On Windows, use `python` instead of `python3`:

```powershell
python -B check.py --list
python -B check.py --strict fmt clippy test config
python -B check.py --tests
```

Omitting stage names runs the default set:

| Stage | What it checks |
|---|---|
| `fmt` | Rust formatting |
| `clippy` | Host Clippy across all targets with warnings denied |
| `test` | Full Rust test suite (excluding shell/PowerShell integration scripts) |
| `rustdoc` | Private-item docs and configured rustdoc lints |
| `config` | CLI configuration, source paths, and required config files |
| `docs` | Markdown links, skill metadata/fixtures, VitePress build, built links, and browser regressions (plus Rust doc contracts/selectors on Linux) |
| `ci` | Workflow contracts, path/release classification, aggregate gating, and actionlint (pure-Python contract/classification/gate tests plus native `actionlint` on Windows) |
| `shell` | Shared ShellCheck discovery and rules (default on Linux, opt-in on Windows) |
| `powershell` | PSScriptAnalyzer at Warning/Error severity |
| `python` | Ruff linting and formatting (`ruff format --check`) across repository Python scripts and tests |
| `lua` | Luacheck linting across Hyprland and Neovim Lua files (Linux only) |
| `structured` | Zero-dependency JSON/JSONC, TOML, and YAML validation (`check-structured-files.py`) |
| `audit` / `deny` | Dependency advisories and dependency policy |

Cargo stages use `--locked` and `--profile ci` where applicable, resolving
tools from `PATH` and failing on a mismatched Rust version rather than
switching toolchains automatically. Missing tools normally print `SKIP`;
passing `--strict` or `DOTFILES_CHECK_STRICT=1` turns skips into failures, and
any installed tool's nonzero exit (including exit code `2`) fails immediately.
See [Analyzer prerequisites](#analyzer-prerequisites) for required tools.

Opt-in stages cover isolated host-safe test suites and MSRV checks: `msrv`
installs and builds with the MSRV from `Cargo.toml`, `wrapper` runs the
platform bootstrap wrapper tests in isolated mode plus parity checks, `prompt`
runs the PowerShell prompt suite, and Linux also provides `qml` (Qt `qmllint`
and Qt Quick Test), `hooks` (staged-input, maintenance, and full commit-hook
suites), `desktop` (Hyprland and Quickshell Python tests), `shell-config`
(isolated Bash/Zsh fixtures), and `vim-config` (isolated Vim/Neovim config
fixtures). `--tests` runs every host-safe test stage for the current platform,
`--lint` runs every static-analysis and formatting stage, and `--all` runs
default plus all opt-in stages. None of them run mutation testing or live host
integration.

The Linux `docs` stage requires Python, Cargo, the Node version in
`docs/.node-version`, and the
[site dependencies and Chromium runtime](#documentation-site). Its Rust tests
check task selectors, TOML/JSON doc examples, JSON Lines output, and native
Unix transitions; its skill
[metadata validator](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/skill-maintenance/scripts/check-skills.py)
checks frontmatter structure, names, and descriptions using only the Python
standard library.

The Markdown link checker scans tracked and unignored new Markdown files
(including unstaged moves), verifying relative links and resolving this
repository's GitHub `blob/main` and `tree/main` URLs, anchors, and line ranges
against the checkout (it does not fetch external URLs). For a quick link-only
run:

```sh
python3 -B .github/workflows/scripts/check-doc-links.py .
```

## Before pushing

Inspect the full intended commit range against the refreshed destination
branch (comparing against the last successful ancestor if an earlier push
failed so pending changes stay in scope). Make sure your selected checks cover
the **committed** inputs: unstaged edits never validate a commit.

### Documentation and skills

Pick the affected checks from the
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks).
For broad site changes or uncertain impact, `python docs/check.py --committed`
runs the full site suite once on a native platform.

### Implementation and mixed patches

The pre-push coordinator is an optional conservative wrapper that applies CI's
path classifier locally (note that it can pull in extra work, such as a full
docs build on a CLI-only patch). Inspect `--plan` first and prefer focused
commands when your coverage is already clear. With Python 3.11+, run on
Windows:

```powershell
python .github\workflows\scripts\pre-push.py --plan
python .github\workflows\scripts\pre-push.py
```

Or on Linux:

```sh
python3 -B .github/workflows/scripts/pre-push.py --plan
python3 -B .github/workflows/scripts/pre-push.py
```

This cross-platform entrypoint supports both OSes. On
Windows it runs docs plus selected native Rust, config, PowerShell wrapper, and
prompt fixtures in a temporary native checkout, then delegates the Linux worker
to your default WSL distro (storing Linux snapshots in `/tmp` and WSL Cargo
artifacts in a separate Linux cache so fixtures run on a real Linux filesystem).

The Windows coordinator runs docs once and forwards that exact commit's pass
to WSL, avoiding duplicate PSScriptAnalyzer and Windows cross-target Clippy
runs while still running both Linux and Windows Rust tests. A Linux-only run
keeps Windows cross-target Clippy and notes the native Windows gap.

If you already passed `docs/check.py` for this exact commit, pass
`--docs-checked <commit>` (using the SHA it printed) so the Linux worker skips
npm setup and the `docs` stage while keeping all CLI and platform checks.

### Validation cache

The coordinator stores a per-checkout, per-platform Cargo cache under
`~/.cache/dotfiles-pre-push/`. Inspect its path, size, and free disk space
without changing anything:

```sh
python3 -B .github/workflows/scripts/pre-push.py --cache-status
```

Use `python` on Windows, or run from WSL to inspect the Linux cache. Before
compiling Rust, the coordinator locks its cache and clears its own build output
if free space is under 10 GiB (stopping if free space stays below 10 GiB).
Ordinary local `cargo` commands continue using the checkout's own `target/`.

### Baseline and prerequisites

Refresh `origin/main` before planning. By default, the checker uses
authenticated `gh` reads to verify that tip's push CI. If it did not succeed,
the checker selects full coverage from a successful ancestor so pending source
changes stay in scope even when the latest fix only edits tests. API errors,
missing auth, or a missing successful ancestor fail loudly rather than silently
narrowing coverage.

For a different destination, offline work, or retrying against a known
baseline, pass `--base <original-verified-sha>` (and optionally
`--head <commit>`), keeping that original baseline throughout recovery rather
than replacing it with a failed push's tip. Pass `--full` for conservative full
coverage. `--plan` prints the selection without running anything.

The checker exports the target commit into temporary Git checkouts and runs
CI-selected checks with strict skip handling, reusing the Cargo cache and
redirecting CLI state and logs into temporary directories.

Provide the tools required by the printed plan before running the checker: the
pinned Rust toolchain on `PATH` (plus MSRV and the Windows target when Rust
changes select them), a configured WSL distro for Windows implementation runs,
Bash/Zsh/fzf/zstd/bat/tmux for shell fixtures, Vim/Neovim for editor fixtures,
and Playwright's Chromium runtime when docs have not already been checked. The
checker only restores locked npm packages when running docs locally; it never
installs system tools or pushes commits.

Fix any selected local failures before pushing. Review the printed
`REMAINING CI` list for full builds, integration jobs, or publishing left to
CI, and follow the [exact commit's CI results](ci-and-releases.md#reading-a-run)
through completion after pushing.

### Local npm TLS failures

If a local TLS error blocks npm from restoring locked dependencies inside the
temporary snapshot, documentation checks are **unavailable** rather than failed
and do not have to block a push when every other selected local check passes
and GitHub CI will run the docs job for that exact commit.

After confirming the TLS error, record it explicitly for that exact commit (in
PowerShell):

```powershell
$commit = git rev-parse HEAD
python .github\workflows\scripts\pre-push.py --npm-tls-unavailable $commit
```

The coordinator skips npm install and docs checks, prints `UNAVAILABLE` instead
of a pass, and forwards that same status from Windows to WSL while running all
other selected checks (including docs-only commits without provisioning WSL).

Reserve this flag strictly for confirmed local npm TLS failures, never for
missing packages, auth errors, build errors, or failing tests. Never pass
`--docs-checked` for an unavailable check or disable TLS verification; report
the local gap and follow GitHub's required docs job and Pages deployment to
completion.

## Specialized checks

Pick only the recipes matching the files you edited.

### Documentation site

See [docs/README.md](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md#checks)
for local edit-loop checks, Playwright prerequisites, and live preview
commands, and [Before pushing](#before-pushing) for committed-patch checks.

### Staged hook checks

Run extended staged guards from the repository root with:

```sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

`true` and `yes` also enable full mode (export the variable before `git commit`
to run it on commit). Guards are selected by staged paths (including deletions
and both sides of renames):

| Trigger | Default pre-commit (`check-ci-guards.sh`) | Additional `DOTFILES_HOOKS_FULL=1` coverage |
|---|---|---|
| Rust/build inputs | `cargo fmt` and host Clippy (`check-rust.sh`) | Windows GNU-target Clippy when available, then full Rust tests |
| `conf/*.toml`, `symlinks/`, `system/`, or `packages/` | Fast typed configuration validation | `config_drift` integration test |
| Cargo inputs or deny policy | Wildcard dependency guard | Available `cargo-deny` checks |
| Shell, hook, or package recipe paths (`packages/*/PKGBUILD`, `packages/*/*.install`) | Available ShellCheck on affected inputs | — |
| PowerShell (`*.ps1`, `*.psm1`, `*.psd1`) paths | Available PSScriptAnalyzer on affected inputs | — |
| Python (`*.py`, `symlinks/local/bin/code-insiders`) or Lua (`*.lua`) paths | Available Ruff (lint + format check) and Luacheck on affected inputs | — |
| Structured (`*.json`, `*.toml`, `*.yml`, `*.yaml`) paths | Zero-dependency JSON/JSONC, TOML, and YAML validation | — |
| Wrappers or their tests | Wrapper parity contract (`tests/wrapper/test_parity.py`) | Linux wrapper tests |
| GitHub Actions workflows or CI policy scripts | Available `actionlint`, CI workflow contract checks, and release publishing invariants | — |
| Markdown (`*.md`) or skill (`.agents/skills/`, `AGENTS.md`) paths | Staged doc link checks and skill metadata validation | — |

Rust and CI guards share a single lazily exported index snapshot per commit
(ignoring unstaged edits and untracked files), release guards read the staged
workflow directly, and builds reuse the Cargo cache. The verifier and pattern
files themselves load from the working tree, so review edits to them separately.

Running `sh hooks/check-ci-guards.sh` runs the fast targeted guards directly
(omitting `config_drift`, `cargo-deny`, and wrapper tests without full mode).
Neither hook invocation replaces [Before pushing](#before-pushing) checks.

### Sensitive-pattern scanning

[check-sensitive.sh](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-sensitive.sh)
scans newly added lines from `HEAD` to the index (or an empty tree on an
initial commit) using case-insensitive extended regexes, redacting matches in
diagnostics and failing on malformed pattern or allowlist rules even on an
empty diff.

Update [sensitive-patterns.ini](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-patterns.ini)
to detect new patterns and
[sensitive-allowlist.ini](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-allowlist.ini)
for narrow safe spans (allowlist entries redact only the matched span before
scanning the rest of the line; add synthetic tests proving a secret elsewhere on
the same line is still caught). Existing exceptions cover Action pins,
checksums, `test.local` fixtures, and GitHub SSH syntax, not general
`example.com` addresses.

Because the scanner does not inspect history, binaries, or arbitrary encodings,
follow [Secrets](../guides/security-and-trust.md#secrets). Keep hook scripts
POSIX `sh`-compatible, cover partial staging, renames/deletions, unusual
filenames, and empty diffs, and run integration tests only via
[Wrapper and hook tests](#wrapper-and-hook-tests) rather than against your own
checkout.

### CLI validation

Run the `config` stage's checks directly against your checkout without wrapper
bootstrap:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop \
  --only config-diagnostics,symlink-sources,config-files
```

This fails on configuration diagnostics, checks symlink and permission sources
(including inactive categories), and verifies required TOML files; structural
conflicts abort earlier during loading.

Include external APM and script analyzers with:

```sh
cd cli
cargo run --locked --profile ci -- check --root .. --profile desktop --verbose
```

Pass `--overlay /path/to/private-dotfiles` to validate an overlay alongside the
main repo (keeping its output private until sanitized).

To reproduce Linux CI's APM deployment check with APM 0.37.0 on `PATH`:

```sh
cargo build --locked --profile ci --manifest-path cli/Cargo.toml
python3 -B tests/integration/test_apm.py cli/target/ci/dotfiles
```

This copies the public base packages into a temporary home and checkout; its
final frozen install must reject a deliberately edited file rather than
overwriting it.

### Analyzer prerequisites

| Check | Required tools |
|---|---|
| Local APM package validation | `apm` |
| ShellCheck | `shellcheck` and POSIX `sh` |
| PSScriptAnalyzer | PowerShell 7 (`pwsh`) and the PSScriptAnalyzer module |
| Python (Ruff) | `ruff` |
| Lua (Luacheck) | `luacheck` |
| QML (`qml` stage) | Qt 6.8+ `qmltestrunner` and `qmllint` |

Missing tools cause the affected CLI check to skip (turning into failures under
`--fail-on-skip` or `CI`). Windows PowerShell cannot substitute for `pwsh` in
PSScriptAnalyzer.

If `pwsh` is installed but the PSScriptAnalyzer module is missing or broken,
CLI checks, `check.py`, and the pre-commit hook fail (on Windows, `check.py`
skips `powershell` if PSScriptAnalyzer is missing unless `--strict` is passed).

### Wrapper and hook tests

These isolated fixtures stub out external tools instead of downloading or
installing the real CLI (for full commit-hook integration, see
[Full commit-hook suite](#full-commit-hook-suite)):

```sh
python3 -B tests/hooks/test_hook_inputs.py \
  test_staged_ci_guards test_staged_ci_guard_deletions
python3 -B tests/wrapper/test_shell_wrapper_linux.py --isolated \
  test_wrapper_preserves_runtime_context test_wrapper_uses_cargo_artifact
```

Omitting test names runs every case in `test_hook_inputs.py` (its build
metadata case needs the pinned Rust toolchain and skips otherwise).

On Windows, dot-source the wrapper suite to invoke individual isolated cases:

```powershell
. .\tests\wrapper\Test-ShellWrapper.ps1
Test-IsolatedWrapperPath
Test-CargoArtifactPath
```

Full wrapper suites can build or copy binaries or read `BINARY_PATH`, so read
their setup before running every case.

### Full commit-hook suite

Run the full pre-commit integration suite from the repository root:

```sh
python3 -B tests/hooks/test_git_hooks.py
```

The script initializes a disposable temporary Git repository, copies `hooks/`
into it, and creates its test commits there so the caller's checkout is never
modified.

### Cleanup retention and Git templates

```sh
python3 -B tests/hooks/test_maintenance.py
```

Verifies branch cleanup, protected branches, worktrees, and retention rules in
temporary repos and directories without starting services or touching real user
files.

### Desktop shell

QML checks require Qt 6.8+ (`qmllint` and `qmltestrunner`) and replace
Quickshell components with test doubles so your live desktop stays untouched:

```sh
python3 -B check.py --strict qml
```

Run the Python helper, stock-cache, and lock-script suites in isolation:

```sh
python3 -B -m unittest discover \
  -s tests/desktop/quickshell/python -p 'test_*.py'
python3 -B -m unittest discover \
  -s tests/desktop/hypr -p 'test_*.py'
```

Offscreen suites verify logic and layout rather than live rendering, focus, or
networking.

CI also launches the real Quickshell config under headless software-rendered
Sway in a disposable Arch container to verify bar space reservation, workspace
switching, output hotplug/resize/disable, and Swaylock session locking.
Passing `--native-hyprland` requires a disposable GPU-capable runner for nested
Hyprland and Hyprlock (failing loudly if unavailable). Neither container mode
tests passwords, systemd user services, physical monitors, or suspend/resume.

### Application configuration

Run the isolated shell, Vim/Neovim, and PowerShell prompt fixtures:

```sh
python3 -B tests/shell/test_shell_config.py
python3 -B tests/vim/test_vim_config.py
pwsh -NoProfile -File tests/shell/Test-Prompt.ps1
```

Each managed application also has a dedicated installed-config smoke harness
under its own directory (`tests/git/test_git.py`, `tests/shell/test_zsh.py`,
`tests/vim/test_vim.py`, `tests/vim/test_nvim.py`) that runs every check by
default or accepts specific test names.

The PowerShell suite checks first-start discovery, custom Cargo paths, literal
fragment directories, and `PATH` deduplication on reload. Install required
tools so cases actually run rather than skipping.

### CI selection fixtures

```sh
python3 -B .github/workflows/scripts/test_ci_changes.py
```

Tests path classification, aggregate `ci-success` gating, release contracts,
renames and deletions, unusual paths, and invalid ranges using temporary repos
and mocked `gh` calls (pass `ClassificationTests GateTests` to run just those
groups). Run the local `ci` stage too whenever workflow YAML or contracts
change. See [CI and releases](ci-and-releases.md) for how gating and
[release selection](ci-and-releases.md#release-selection) work remotely.

## Focused mutation testing

Mutation testing temporarily introduces small code faults to test whether your
assertions catch a flipped branch, dropped error, or broken preview. Use it
selectively before pushing when ordinary assertions still leave a real gap:

| Change or uncertainty | Local mutation choice before pushing |
|---|---|
| Data preservation, permissions/elevation, dry-run safety, or failure/partial-success propagation | Run focused mutations when a guard or preserved-state assertion could pass despite broken behavior |
| Safety tests are weakened, consolidated, or substantially rewritten | Recheck the affected production function/module, even when only tests changed |
| A known surviving mutant is being fixed | Replay that mutant or its original source range and verify it is caught |
| Prose/configuration, mechanical refactoring, or low-risk behavior already directly asserted | Ordinary focused checks are usually sufficient; do not add mutations automatically |

Mention your mutation choice briefly alongside your selected checks. Target
only the relevant file or function on its native platform rather than the whole
crate, and do not defer a known safety-test gap to the nightly run (for native
Windows paths unavailable locally, report the gap or trigger a focused manual
remote run).

Follow [Mutation testing](mutation-testing.md) to keep the original source
range, inspect survivors, and clean up generated mutation artifacts (a
zero-mutant run proves nothing).

## Platform coverage parity

When running Linux checks from Windows, keep your checkout on WSL's native
Linux filesystem (such as `~/src/dotfiles`) rather than under `/mnt/c`. Several
fixtures create temporary directories **inside the checkout** requiring Unix
permission bits and non-UTF-8 filenames that Windows mounts cannot represent
(changing `TMPDIR` alone will not relocate them).

Linux cross-target Clippy catches Windows `cfg`-gate, type, and import mistakes
without needing a second OS locally:

```sh
cd cli
cargo clippy --locked --profile ci --target x86_64-pc-windows-gnu \
  --all-targets -- -D warnings
```

It does **not** exercise Windows registry, UAC, symlink/junction, path, or
process execution; native Windows tests (locally or in exact-commit CI) must
cover those. See [Platform coverage](ci-and-releases.md#platform-coverage) for
the full matrix.

Executor tests spawn their ignored child-process fixture internally, so do not
invoke it directly with `--ignored`.

Next, see [Writing tests](writing-tests.md) when adding new regressions, or
[Mutation testing](mutation-testing.md) when verifying safety-critical
assertions.
