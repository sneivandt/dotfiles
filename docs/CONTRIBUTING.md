# Contributing

Keep wrappers, declarative desired state, domain behavior, and application
orchestration in their existing layers.

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

1. Identify the owning layer and read the closest implementation.
2. Use `conf/` and the command and task catalogs as the source of truth.
3. Check whether an existing resource, provider, task, or operation can be
   reused.
4. Preserve user-owned working-tree changes and private overlay content.

Do not add command behavior to `dotfiles.sh` or `dotfiles.ps1`; wrappers only
bootstrap and forward.

## Change patterns

### Configuration-only change

1. Edit the appropriate `conf/*.toml`.
2. Keep platform and role entries in the narrowest category.
3. If a conditional symlink changes, update `conf/manifest.toml`.
4. Run the configuration drift test and `dotfiles test`.
5. Preview the affected task with `--dry-run`.

### New config-backed resource

Implement the full vertical slice:

```text
config type -> loader -> validator -> conf entry -> resource/provider
-> task -> command registration -> exports -> tests
```

Static install or uninstall tasks must be registered in
`cli/src/app/catalog.rs`. Command-specific tasks belong in that command's task
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
checks scattered through task logic. Guard imports, types, and calls at the
right boundary so the other platform still compiles.

## Code quality

- Propagate failures with context; do not return success for invalid input.
- Keep mutations idempotent.
- Keep `--dry-run` free of mutations.
- Avoid broad exception handling or silent fallbacks.
- Reuse typed configuration and engine helpers.
- Add comments only where behavior is not self-explanatory.
- Keep generated artifacts and private files out of commits.

## Targeted checks

Run the default local verification stages:

```bash
sh .github/workflows/scripts/linux/check.sh
```

On Windows:

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

The runner uses the same `ci` Cargo profile as CI. Stages whose tools are not
installed report `SKIP` rather than failing. MSRV is opt-in, and CI also has
integration, coverage, and mutation jobs outside this runner. See
[Testing](TESTING.md) for the stage list.

While iterating, use the narrowest stage that covers the change:

```bash
sh .github/workflows/scripts/linux/check.sh fmt clippy
cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift
```

See [Testing](TESTING.md) for script, wrapper, hook, and integration checks.

## CI and publishing

`.github/workflows/ci.yml` is the pull-request and push gate. Its build, lint,
test, security, validation, wrapper, hook, and integration jobs are reflected
in a final `ci-success` gate. Coverage is informational.

Release and Docker publishing run only after successful same-repository pushes
to `main`. Publishing builds use release mode; ordinary CI uses the `ci`
profile. Recurring integration logic belongs in
`.github/workflows/scripts/`, not large inline workflow blocks.

Releases use tags in the form `vYYYY.MM.DD-N`. `N` starts at 1 and increments
for each additional release on the same day. The `version` job resolves the tag
once and passes it to every build job, so all artifacts use the same version.
Release runs are serialized to prevent duplicate tags. `release.yml` also
accepts `workflow_dispatch` for a manual publish, but requires the run ID of a
completed, successful `CI` push run for `main`; the release always builds that
run's tested commit.

The CLI's self-update path only recognizes this tag format. Binaries built
before it was adopted cannot parse current release tags and will stop
self-updating; replace them by deleting the binary and re-running a wrapper.

## Documentation changes

Update the guide closest to the behavior. If a task is added, removed, renamed,
changes command membership, or is rewired, update [Task reference](TASKS.md).
Keep the root README as a landing page and place detailed guidance in `docs/`.

CI checks documentation on every change, including documentation-only changes.
Relative Markdown links must resolve, and every selector in
[Task reference](TASKS.md) must exist in the CLI. Run both checks locally with:

```bash
sh .github/workflows/scripts/linux/check.sh docs
```
