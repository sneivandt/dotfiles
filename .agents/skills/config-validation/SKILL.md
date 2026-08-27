---
name: config-validation
description: >
  Use for ConfigValidator aggregation, domain validate() functions, diagnostic
  codes/severity, test-command validation tasks, or cli/tests/config_drift.rs.
  Not for ordinary TOML parsing alone.
---

# Config Validation

## Validation boundary

- Serde rejects unknown keys and structural/type errors.
- Domain `validate()` functions return semantic `Vec<Diagnostic>`.
- Startup and reload both validate before publishing config and display
  diagnostics consistently.
- Warning and error diagnostics both fail `dotfiles check`; severity communicates
  meaning and rendering.

Each diagnostic has a source, item, stable dotted code, severity, and actionable
message. Reuse `Validator` and its `check_each`, `warn`, `warn_if`, `check`, and
`check_error` helpers.

## Add a rule

1. Add or extend the domain's `validate()` function.
2. Choose a stable dotted code.
3. Use error severity for structurally unsafe or unusable values.
4. Wire the validator into `ConfigValidator::validate_all()`.
5. Test valid, invalid, and boundary cases.

Validation tasks live under `cli/src/app/validation/`; the `check` command owns
their inventory. Missing required tools must follow existing task policy, not
silently pass.

## Cross-file drift

Use `cli/tests/config_drift.rs` for invariants across real `conf/` and
`symlinks/` data, such as manifest ownership and source existence. Keep these
tests self-contained, report every offending item, and use directory-prefix
coverage rules consistently.

Use `toml-configuration` when the schema or loader also changes.
