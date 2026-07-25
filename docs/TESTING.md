# Testing and Validation

The project tests domain logic, task orchestration, command behavior,
configuration drift, wrappers, hooks, installation, and both supported host
families.

## Fast local sequence

From the repository root, run every check CI runs:

```bash
sh .github/workflows/scripts/linux/check.sh
```

On Windows:

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

This is the single source of truth for local verification. It uses the `ci`
Cargo profile throughout, so a local pass and a CI pass mean the same thing.
Any stage whose tool is missing reports `SKIP` instead of failing.

| Stage | Covers |
|---|---|
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --all-targets -D warnings` |
| `test` | `cargo test` |
| `config` | `dotfiles --root . test` (repository validator) |
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

Individual Cargo commands remain available when you want to bypass the runner:

```bash
cargo test --profile ci --manifest-path cli/Cargo.toml
cargo clippy --profile ci --manifest-path cli/Cargo.toml --all-targets -- -D warnings
```

## Integration test suites

The Rust integration tests under `cli\tests\` cover distinct boundaries:

| Test target | Focus |
|---|---|
| `behavioral_ci` | Cross-cutting behaviors that protect CI assumptions |
| `config_drift` | Alignment among config, manifest, symlink, and catalog state |
| `domain_boundaries` | Architectural dependency boundaries |
| `e2e_apply` | End-to-end convergence against controlled state |
| `install_command` | Install selection and command composition |
| `task_execution` | Scheduler, dependencies, and result behavior |
| `test_command` | Validation task construction and outcomes |
| `uninstall_command` | Conservative uninstall composition and behavior |

Run one suite:

```bash
cargo test --manifest-path cli/Cargo.toml --test config_drift
```

Run one named test:

```bash
cargo test --manifest-path cli/Cargo.toml --test install_command test_name
```

## CLI validation

`dotfiles test` is the user-facing repository validator. It checks:

- loader warnings
- symlink source existence
- required TOML files
- sparse manifest synchronization
- APM plugins when APM is available
- shell scripts when ShellCheck is available
- PowerShell scripts whenever `pwsh` is available; the PSScriptAnalyzer module
  must also be installed or the check fails

```bash
dotfiles --root . test --verbose
```

When an overlay is involved, always validate the combined configuration:

```bash
dotfiles --root . --overlay C:\path\to\private-dotfiles test
```

## Dry-run testing

Dry-run is part of the mutation contract, not merely a display feature. Preview
the smallest affected task set and inspect both applicability and planned
actions:

```bash
dotfiles --root . install --only symlinks --dry-run --verbose
dotfiles --root . update --only apm,apm-update --dry-run --verbose
dotfiles --root . uninstall --dry-run --verbose
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
`scripts\linux\lib\`.

The pre-commit checks can also be run directly:

```bash
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

## CI gates

The main CI workflow includes:

- formatting and linting
- ShellCheck and PowerShell analysis
- configuration validation
- `cargo-audit` and `cargo-deny`
- Linux and Windows builds
- minimum-supported Rust checks
- Rust test suites
- wrapper, hook, install, uninstall, and application integration tests

The coverage job is informational and intentionally does not gate
`ci-success`.

## Platform coverage parity

CI is not symmetric across platforms, and the asymmetry is deliberate. Anything
listed as Linux-only below is *not* validated on Windows, so regressions in
those areas surface only after release.

| Area | Linux | Windows | Notes |
|---|---|---|---|
| Build, Clippy, tests | yes | yes | |
| Profile dry-run and `dotfiles test` | yes | yes | `base` and `desktop` |
| Install/uninstall round-trip | yes | yes | |
| Wrapper | yes | yes | `dotfiles.sh` / `dotfiles.ps1` |
| Application: git | yes | yes | Windows also asserts the `core.autocrlf` override |
| Application: zsh, vim, nvim | yes | n/a | Excluded from the Windows profile |
| Git hook sensitive-data check | yes | no | Hooks are POSIX `sh`; not run on Windows |
| ShellCheck, PSScriptAnalyzer | yes | n/a | Both run on the Linux runner |
| `cargo audit`, `cargo deny`, MSRV | yes | n/a | Platform-independent |

Cross-target Clippy catches most compile-time Windows breakage from Linux, but
it does not validate runtime Windows behavior. When changing Windows-specific
code paths, rely on the Windows CI jobs rather than local Linux checks alone.

The git application test asserts values contributed by files under
`~/.config/git`, not git's final effective value. Git reads `~/.gitconfig`
after `$XDG_CONFIG_HOME/git/config`, so anything set there wins; GitHub's
Windows runners ship a `~/.gitconfig` that would otherwise mask the
`core.autocrlf` override. Scoping to the files this repository installs keeps
the assertion about the dotfiles rather than about the machine.

## Choosing coverage

| Change | Minimum focused validation |
|---|---|
| TOML data | `config_drift` plus `dotfiles test` |
| Task metadata or dependencies | relevant command suite plus `task_execution` |
| Resource behavior | domain unit tests plus affected command/e2e suite |
| Wrapper | platform wrapper integration script |
| Hook | hook script and Git hook integration test |
| Cross-platform Rust | tests/check on the host plus the repository's cross-platform sequence |
| CI workflow | local script or narrow command used by the changed job |

Escalate to the full suite when shared engine behavior, catalog composition, or
configuration loading changes.
