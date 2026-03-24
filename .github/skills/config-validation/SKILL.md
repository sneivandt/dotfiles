---
name: config-validation
description: >
  Configuration validation system: the Validator builder, per-module validate()
  functions, the test command, and config drift integration tests. Use when
  adding validation rules or extending the test command.
metadata:
  author: sneivandt
  version: "1.0"
---

# Config Validation

Validation happens at two levels: **runtime** (the `test` command) and
**compile-time integration tests** (config drift checks).

## Runtime: The `test` Command

`commands/test.rs` runs six validation tasks:

| Task | What it checks |
|---|---|
| `ValidateConfigWarnings` | Fails when `Config::validate()` emits warnings |
| `ValidateSymlinkSources` | Every symlink source file exists on disk |
| `ValidateConfigFiles` | Required config files (`profiles.toml`, `symlinks.toml`, `packages.toml`, `manifest.toml`) exist |
| `ValidateManifestSync` | `symlinks.toml` and `manifest.toml` expose the same non-base category sections |
| `RunShellcheck` | Shell scripts pass shellcheck (skipped if unavailable) |
| `RunPSScriptAnalyzer` | PowerShell scripts pass PSScriptAnalyzer (skipped if unavailable) |

All live in `cli/src/phases/validation.rs` and implement the `Task` trait.

## Per-Module `validate()` Functions

Each config module in `cli/src/config/` exposes a `validate()` function
returning `Vec<ValidationWarning>`. These are aggregated by `Config::validate()`:

```rust
pub fn validate(&self, platform: Platform) -> Vec<ValidationWarning> {
    let root = &self.root;
    let mut warnings = Vec::new();
    warnings.extend(symlinks::validate(&self.symlinks, root));
    warnings.extend(packages::validate(&self.packages, platform));
    warnings.extend(registry::validate(&self.registry, platform));
    warnings.extend(chmod::validate(&self.chmod, platform));
    warnings.extend(systemd_units::validate(&self.units, platform));
    warnings.extend(vscode_extensions::validate(&self.vscode_extensions));
    warnings.extend(copilot_plugins::validate(&self.copilot_plugins));
    warnings.extend(git_config::validate(&self.git_settings));
    warnings
}
```

### ValidationWarning

```rust
pub struct ValidationWarning {
    pub source: String,   // e.g., "packages.toml"
    pub item: String,     // e.g., "git"
    pub message: String,  // e.g., "package name is empty"
}
```

### Validator Builder

`config/helpers/validation.rs` provides a fluent builder:

```rust
let warnings = Validator::new("packages.toml")
    .check_each(&packages, |pkg| &pkg.name, |pkg| {
        vec![
            check(pkg.name.trim().is_empty(), "package name is empty"),
            check(pkg.name.contains(' '), "package name contains spaces"),
        ]
    })
    .finish();
```

Key API:
- `Validator::new(source)` — captures the TOML filename once
- `.check_each(items, label_fn, check_fn)` — validates each item; `check_fn` returns `Vec<Option<String>>`
- `.warn(item, message)` — standalone warning
- `.warn_if(condition, item, message)` — conditional warning
- `.finish()` — consumes the builder and returns collected warnings
- `check(condition, message)` — free function returning `Some(message)` if condition is `true`

### Adding Validation to a Config Module

1. Add a `pub fn validate(items: &[MyType], ...) -> Vec<ValidationWarning>` function
2. Use the `Validator` builder for consistency
3. Wire it into `Config::validate()` in `config/mod.rs`

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

### Adding a New Drift Test

1. Add a `#[test]` function in `config_drift.rs`
2. Load real config files from `repo_root().join("conf")`
3. Assert cross-file invariants
4. Use descriptive assertion messages listing the offending items

## Rules

- Use the `Validator` builder for all per-module validation — avoid manual `Vec::push()` loops
- Every config module should have a `validate()` function
- Wire new validators into `Config::validate()` in `config/mod.rs`
- Config drift tests read real files — keep them self-contained with private TOML types
- Validation tasks in `phases/validation.rs` follow the standard `Task` trait pattern
