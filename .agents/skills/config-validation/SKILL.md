---
name: config-validation
description: >
  Configuration validation for this dotfiles repo. Use when adding or changing
  Validator rules, per-module validate() functions, test-command validation
  tasks, or config drift integration tests.
---

# Config Validation

Validation happens at two levels: **runtime** (the `test` command) and
**compile-time integration tests** (config drift checks).

## Runtime: The `test` Command

Validation tasks live under `cli/src/app/validation/` and implement `Task`.
Keep `cli/src/app/commands/test.rs` as the authoritative task list rather than
duplicating its inventory elsewhere. New tasks must report whether a required
tool is missing according to the existing command policy instead of silently
passing.

## Two Layers: Serde Strictness and Semantic Validation

Deserialization rejects structurally wrong configuration (unknown keys, type
mismatches) and fails the load outright. `Config::validate` reports semantic
diagnostics and is advisory. Keep the boundary: a misspelled key belongs to the
Serde layer via `deny_unknown_fields`, not to a validator.

## Load and Reload Must Behave Identically

`Config::load` and `Config::validate` are separate calls, so any code path that
loads configuration must also validate it. Both the startup path
(`commands/runner.rs::load_config`) and the mid-run path
(`app/reload.rs::ReloadConfig`) validate and then call
`app::validation::display_diagnostics`.

Reload diagnostics stay advisory, matching startup. Escalating only on reload
would make the outcome depend on run timing rather than on config content. When
adding a new path that swaps configuration into the store, validate before
publishing.

## Per-Module `validate()` Functions

Each domain config module exposes a `validate()` function returning
`Vec<Diagnostic>`. `Config::validate()` delegates aggregation to the internal
`ConfigValidator` helper in `app/config/mod.rs`:

```rust
pub fn validate(&self, platform: Platform) -> Vec<Diagnostic> {
    ConfigValidator::new(self, platform).validate_all().finish()
}
```

Each `Diagnostic` identifies its source and item, carries a stable dotted code,
classifies the finding as warning or error, and includes a user-facing message.
Both severities fail `dotfiles test`; severity controls rendering and meaning.

### Validator Builder

`infra/config/validation.rs` provides the fluent builder. Key API:
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
consistency. Treat that file as the authoritative inventory rather than
duplicating its test list here. It covers, non-exhaustively: symlink section to
`manifest.toml` section coverage in both directions, every non-base symlink
source being covered by some manifest path, and every manifest path existing
under `symlinks/`.

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
