# Contributing

Changes should preserve the separation between wrappers, declarative desired
state, domain behavior, and application orchestration.

## Development setup

Requirements:

- Git
- the Rust toolchain declared by the repository
- PowerShell on Windows
- ShellCheck for shell-script checks
- PowerShell 7 and the PSScriptAnalyzer module for PowerShell-script checks

Build and run the CLI:

```bash
cargo build --manifest-path cli/Cargo.toml
cargo run --manifest-path cli/Cargo.toml -- --root . test
```

On Windows, the wrapper can build and invoke the local binary in one step:

```powershell
.\dotfiles.ps1 --build test
```

## Before editing

1. Identify the owning layer and read the nearest implementation.
2. Treat `conf\` and the command/task catalogs as authoritative.
3. Check whether an existing resource, provider, task, or operation can be
   reused.
4. Preserve user-owned working-tree changes and private overlay content.

Do not add command behavior to `dotfiles.sh` or `dotfiles.ps1`; wrappers only
bootstrap and forward.

## Change patterns

### Configuration-only change

1. Edit the appropriate `conf\*.toml`.
2. Keep platform and role entries in the narrowest category.
3. If a conditional symlink changes, update `conf\manifest.toml`.
4. Run the configuration drift test and `dotfiles test`.
5. Preview the affected task with `--dry-run`.

### New config-backed resource

Implement the full vertical slice:

```text
config type -> loader -> validator -> conf entry -> resource/provider
-> task -> command registration -> exports -> tests
```

Static install or uninstall tasks must be registered in
`cli\src\app\catalog.rs`. Command-specific tasks belong in that command's task
list.

### New task

Define:

- stable task identity
- clear display name
- command membership, including update-only behavior when applicable
- applicability
- same-domain dependencies
- elevation policy
- dry-run-safe execution

Add cross-domain dependency edges in the application catalog. Do not rely on
catalog insertion order.

### New workflow

Use an operation when state must converge as one coherent workflow rather than
as independent records. The operation should provide explicit current-state,
preview, and apply behavior.

### Platform-specific change

Prefer platform capability methods and adapters over direct operating-system
checks scattered through task logic. Ensure the other platform still compiles
by guarding imports, types, and calls at the right boundary.

## Code quality

- Propagate failures with context; do not return success for invalid input.
- Keep mutations idempotent.
- Ensure `--dry-run` does not perform the mutation.
- Avoid broad exception handling or silent fallbacks.
- Reuse typed configuration and engine helpers.
- Add comments only where behavior is not self-explanatory.
- Keep generated artifacts and private files out of commits.

## Targeted checks

Use the narrowest existing check that covers the change:

```bash
cargo fmt --manifest-path cli/Cargo.toml -- --check
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path cli/Cargo.toml
cargo test --manifest-path cli/Cargo.toml --test config_drift
```

CI reproductions use the repository's `ci` Cargo profile:

```bash
cargo test --profile ci --manifest-path cli/Cargo.toml
cargo clippy --profile ci --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift
```

See [Testing](TESTING.md) for script, wrapper, hook, and integration checks.

## CI and publishing

`.github\workflows\ci.yml` is the pull-request and push gate. Its build, lint,
test, security, validation, wrapper, hook, and integration jobs are reflected
in a final `ci-success` gate. Coverage is informational.

Release and Docker publishing run only after successful same-repository pushes
to `main`. Publishing builds use release mode; ordinary CI uses the `ci`
profile. Recurring integration logic belongs in
`.github\workflows\scripts\`, not large inline workflow blocks.

## Documentation changes

Update the guide closest to the behavior. If a task is added, removed, renamed,
changes command membership, or is rewired, update [Task reference](TASKS.md).
Keep the root README as a landing page and place detailed guidance in `docs\`.
