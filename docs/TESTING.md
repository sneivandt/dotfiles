# Testing and Validation

The project tests domain logic, task orchestration, command behavior,
configuration drift, wrappers, hooks, installation, and both supported host
families.

## Fast local sequence

From the repository root:

```bash
cargo fmt --manifest-path cli/Cargo.toml -- --check
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path cli/Cargo.toml
cargo run --manifest-path cli/Cargo.toml -- --root . test
```

Use the `ci` profile when reproducing CI-specific compilation or test behavior:

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
dotfiles --root . update --only APM --dry-run --verbose
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
