---
name: config-validation
description: >
  Configuration validation system: the Validator builder, per-module validate()
  functions, the test command, and config drift integration tests. Use when
  adding validation rules or extending the test command.
---

# Config Validation

Validation happens at two levels: **runtime** (the `test` command) and
**compile-time integration tests** (config drift checks).

## Runtime: The `test` Command

`cli/src/app/commands/test.rs` runs seven validation tasks:

| Task | What it checks |
|---|---|
| `ValidateConfigWarnings` | Fails when `Config::validate()` emits diagnostics |
| `ValidateSymlinkSources` | Every symlink source file exists on disk |
| `ValidateConfigFiles` | Required config files (`profiles.toml`, `symlinks.toml`, `packages.toml`, `manifest.toml`) exist |
| `ValidateManifestSync` | `symlinks.toml` and `manifest.toml` expose the same non-base category sections |
| `ValidateApmPlugins` | Local APM plugins pass `apm pack --dry-run --verbose` when APM is available |
| `RunShellcheck` | Shell scripts pass shellcheck (skipped if unavailable) |
| `RunPSScriptAnalyzer` | PowerShell scripts pass PSScriptAnalyzer (skipped if unavailable) |

Implementations live under `cli/src/app/validation/` and implement the `Task`
trait. Keep `cli/src/app/commands/test.rs` as the authoritative task list.

## Per-Module `validate()` Functions

Each domain config module exposes a `validate()` function returning
`Vec<Diagnostic>`. `Config::validate()` delegates aggregation to the internal
`ConfigValidator` helper in `app/config/mod.rs`:

```rust
pub fn validate(&self, platform: Platform) -> Vec<Diagnostic> {
    ConfigValidator::new(self, platform).validate_all().finish()
}
```

### Diagnostic

```rust
pub struct Diagnostic {
    pub source: String,       // e.g., "packages.toml"
    pub item: String,         // e.g., "git"
    pub severity: Severity,   // Warning or Error
    pub code: &'static str,   // e.g., "package.empty-name"
    pub message: String,      // e.g., "package name is empty"
}
```

Both severities fail `dotfiles test`; severity classifies the finding and
controls rendering. Codes are stable, concise identifiers for the validation
rule.

### Validator Builder

`infra/config/validation.rs` provides a fluent builder:

```rust
let diagnostics = Validator::new("packages.toml")
    .check_each(&packages, |pkg| &pkg.name, |pkg| {
        [
            check(pkg.name.trim().is_empty(), "package.empty-name", "package name is empty"),
            check(pkg.name.contains(' '), "package.spaces", "package name contains spaces"),
        ]
    })
    .finish();
```

Key API:
- `Validator::new(source)` — captures the TOML filename once
- `.check_each(items, label_fn, check_fn)` — validates each item; `check_fn`
  returns `CheckItem` values containing code, severity, and message
- `.warn(code, item, message)` — standalone warning diagnostic
- `.warn_if(condition, code, item, message)` — conditional warning diagnostic
- `.finish()` — consumes the builder and returns collected diagnostics
- `check(condition, code, message)` — warning check
- `check_error(condition, code, message)` — structurally invalid or unsafe check

### Adding Validation to a Config Module

1. Add a `pub fn validate(items: &[MyType], ...) -> Vec<Diagnostic>` function
2. Use the `Validator` builder for consistency
3. Assign a stable dotted code to every rule and classify unsafe/structurally
   invalid values as `Severity::Error`
4. Wire it into `ConfigValidator::validate_all()` in `app/config/mod.rs`

## Integration Tests: Config Drift

`cli/tests/config_drift.rs` reads the real `conf/` files and checks cross-file
consistency:

| Test | What it catches |
|---|---|
| `non_base_symlink_sections_have_manifest_sections` | Non-base symlink section missing from `manifest.toml` |
| `non_base_symlink_sources_covered_by_manifest` | Symlink source not covered by any manifest path |
| `manifest_paths_exist_in_symlinks_dir` | Manifest path pointing to non-existent file in `symlinks/` |

These tests use private TOML structs (not library types) to stay self-contained.
The `is_covered_by()` helper handles directory prefix matching (trailing `/`).

With `DOTFILES_HOOKS_FULL=1`, `hooks/check-ci-guards.sh` runs the shell config
validators whenever staged changes touch `conf/*.toml` or `symlinks/`, plus
`cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift`
so manifest/symlink drift is caught before CI.

### Adding a New Drift Test

1. Add a `#[test]` function in `config_drift.rs`
2. Load real config files from `repo_root().join("conf")`
3. Assert cross-file invariants
4. Use descriptive assertion messages listing the offending items

## Rules

- Use the `Validator` builder for all per-module validation — avoid manual `Vec::push()` loops
- Every config module should have a `validate()` function
- Wire new validators into `ConfigValidator::validate_all()` in `app/config/mod.rs`
- Config drift tests read real files — keep them self-contained with private TOML types
- Validation tasks in `app/validation/mod.rs` follow the standard `Task` trait pattern
