# Testing and validation

Tests cover domain logic, task orchestration, commands, configuration drift,
wrappers, hooks, installation, Linux, and Windows.

## Fast local sequence

From the repository root, run the default Linux verification stages:

```bash
sh .github/workflows/scripts/linux/check.sh
```

On Windows:

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

These scripts define the local verification sequence and use the same `ci`
Cargo profile as CI. A stage reports `SKIP` instead of failing when its tool is
missing. The default stages do not include the opt-in MSRV check, integration
jobs, coverage, or mutation testing.

| Stage | Covers |
|---|---|
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --all-targets -D warnings` |
| `test` | `cargo test` |
| `config` | `dotfiles check --root .` (repository validator) |
| `docs` | Relative Markdown links and heading anchors resolve; documented task selectors exist (Linux script only) |
| `shell` | ShellCheck over wrappers, hooks, and CI scripts |
| `powershell` | PSScriptAnalyzer over all `.ps1`/`.psm1` |
| `audit` | `cargo audit` |
| `deny` | `cargo deny check all` |
| `msrv` | Compile against the MSRV in `cli/Cargo.toml` (opt-in) |

Run a subset, list the stages, or include the opt-in ones:

```bash
sh .github/workflows/scripts/linux/check.sh fmt clippy
sh .github/workflows/scripts/linux/check.sh --list
sh .github/workflows/scripts/linux/check.sh --all
```

The Windows script has no `docs` stage. CI runs documentation consistency on
Linux.

Run Cargo directly when you need one Rust check:

```bash
cd cli
cargo test --profile ci
cargo clippy --profile ci --all-targets -- -D warnings
```

## Integration test suites

The Rust integration tests under `cli/tests/` cover distinct boundaries:

| Test target | Focus |
|---|---|
| `behavioral_ci` | Cross-cutting behaviors that protect CI assumptions |
| `config_drift` | Alignment among real configuration, symlink, and catalog state |
| `domain_boundaries` | Architectural dependency boundaries |
| `e2e_apply` | End-to-end convergence against controlled state |
| `install_command` | Install selection and command composition |
| `task_execution` | Scheduler, dependencies, and result behavior |
| `test_command` | Validation task construction and outcomes |
| `uninstall_command` | Conservative uninstall composition and behavior |

Run one suite:

```bash
cd cli
cargo test --test config_drift
```

Run one named test:

```bash
cd cli
cargo test --test install_command test_name
```

## CLI validation

`dotfiles check` is the user-facing repository validator. It checks:

- loader warnings
- symlink source existence
- required TOML files
- APM plugins when APM is available
- shell scripts when ShellCheck is available
- PowerShell scripts whenever `pwsh` is available; the PSScriptAnalyzer module
  must also be installed or the check fails

```bash
dotfiles check --root . --verbose
```

When an overlay is involved, always validate the combined configuration:

```bash
dotfiles check --root . --overlay C:\path\to\private-dotfiles
```

## Dry-run testing

Dry-run is part of the mutation contract. Preview the smallest affected task
set, then inspect applicability and planned actions:

```bash
dotfiles install --root . --only symlinks --dry-run --verbose
dotfiles install --root . --update-pins --only apm,apm-update --dry-run --verbose
dotfiles uninstall --root . --dry-run --verbose
```

A dry run must not change files, package state, registry values, unit state, or
generated manifests.

## Wrapper and hook tests

CI-maintained integration scripts live under:

```text
.github\workflows\scripts\linux\
.github\workflows\scripts\windows\
```

They cover wrapper forwarding and download behavior, install/uninstall flows,
configuration, application availability, Git hooks, static analysis, and
platform-specific cases. Shared Linux helpers live under
`scripts/linux/lib/`.

The pre-commit checks can also be run directly:

```bash
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
sh hooks/check-ci-guards.sh
sh hooks/pre-commit
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

The full hook integration script creates real commits and refuses to run in a
dirty checkout. Run it in a fresh scratch repository:

```bash
mkdir -p /tmp/hooktest/hooks && cp -a hooks/. /tmp/hooktest/hooks/
cd /tmp/hooktest && git init -q
ln -sf /tmp/hooktest/hooks/pre-commit .git/hooks/pre-commit
DIR=/path/to/dotfiles sh /path/to/dotfiles/.github/workflows/scripts/linux/test-git-hooks.sh
```

A failed case can leave its fixture committed, so do not reuse the scratch
repository for another run.

## CI gates

The main CI workflow includes:

- formatting and linting
- ShellCheck and PowerShell analysis
- documentation consistency (runs even for docs-only changes)
- configuration validation
- `cargo-audit` and `cargo-deny`
- Linux and Windows builds
- minimum-supported Rust checks
- Rust test suites
- wrapper, hook, install, uninstall, and application integration tests

The Linux and Windows coverage jobs run all Cargo targets and upload HTML
reports. Coverage is informational and intentionally does not gate
`ci-success`.

Pull requests also run changed-code mutation testing when Rust code changes and
upload the `cargo-mutants` report. Mutation results are informational and do not
gate `ci-success`.

## Platform coverage parity

CI coverage differs by platform. Anything listed as Linux-only below is not
validated on Windows.

| Area | Linux | Windows | Notes |
|---|---|---|---|
| Build, Clippy, tests | yes | yes | |
| All-target coverage report | yes | yes | Informational HTML artifact |
| Profile dry-run and `dotfiles check` | yes | yes | `base` and `desktop` |
| Install/uninstall round-trip | yes | yes | |
| Wrapper | yes | yes | `dotfiles.sh` / `dotfiles.ps1` |
| Application: git | yes | yes | Windows also asserts the `core.autocrlf` override |
| Application: zsh, vim, nvim | yes | n/a | Excluded from the Windows profile |
| Application: volume initialization | yes | n/a | PipeWire/PulseAudio integration is Linux-only |
| Git hook sensitive-data check | yes | no | Hooks are POSIX `sh`; not run on Windows |
| ShellCheck, PSScriptAnalyzer | yes | n/a | Both run on the Linux runner |
| `cargo audit`, `cargo deny`, MSRV | yes | n/a | Platform-independent |

Cross-target Clippy catches many Windows compile errors from Linux, but it
cannot test Windows runtime behavior. For Rust changes that can break Windows
compilation, run:

```bash
cd cli
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
```

If the target or toolchain is unavailable, record the omitted check. Use the
Windows CI jobs for changes to Windows-specific paths.

The Windows git application test asserts the final effective configuration
outside any repository. Installation removes the obsolete managed
`core.autocrlf` entry from `~/.gitconfig`, leaving the platform include under
`~/.config/git` authoritative.

## Choosing coverage

| Change | Minimum focused validation |
|---|---|
| TOML data | `config_drift` plus `dotfiles check` |
| Task metadata or dependencies | relevant command suite plus `task_execution` |
| Resource behavior | domain unit tests plus affected command/e2e suite |
| Environment-dependent behavior | unit tests injecting a fixed environment, not process-global variables |
| Wrapper | platform wrapper integration script |
| Hook | hook script and Git hook integration test |
| Cross-platform Rust | tests/check on the host plus the repository's cross-platform sequence |
| Documentation | `check.sh docs` (link and task-selector consistency) |
| CI workflow | local script or narrow command used by the changed job |

Escalate to the full suite when shared engine behavior, catalog composition, or
configuration loading changes.
