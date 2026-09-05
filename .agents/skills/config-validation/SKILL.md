---
name: config-validation
description: >
  Use for Config::validate aggregation, domain validate() functions, diagnostic
  codes/severity, check-command validation tasks, or cli/tests/config_drift.rs.
  Not for ordinary TOML parsing alone.
---

# Config Validation

## Validation boundary

- Serde rejects unknown keys and structural/type errors.
- Domain `validate()` functions return semantic `Vec<Diagnostic>`.
- Startup loads and reports diagnostics before publishing the immutable config
  snapshot; a repository restart repeats this in a fresh process, not a mutable
  in-process reload.
- Warning and error diagnostics both fail `dotfiles check`; severity communicates
  meaning and rendering. Reporting a diagnostic during ordinary startup does
  not itself abort every mutation: preserve structural preflight checks and
  resource-level safety guards.

Each diagnostic has a source, item, stable dotted code, severity, and actionable
message. Reuse `Validator` and its `check_each`, `warn`, `warn_if`, and
`check_conflicts` methods, plus the free `check` / `check_error` helpers.
Their predicate is true when the input is invalid, not when it passes.

## Add a rule

1. Add or extend the domain's `validate()` function.
2. Define a stable `DiagnosticCode::new(domain, rule)` constant.
3. Use error severity for structurally unsafe or unusable values.
4. Wire the validator into `Config::validate()`.
5. Test valid, invalid, and boundary cases, asserting code, severity, source,
   and item identity as well as the actionable message.

Validation tasks live under `cli/src/app/validation/`; the `check` command owns
their inventory. Missing required tools must follow existing task policy, not
silently pass.

## Cross-file drift

Use `cli/tests/config_drift.rs` for invariants across real `conf/` and
`symlinks/` data, such as source existence and cross-file ownership. Keep these
tests self-contained and report every offending item.

Do not replace `validation_symlinks` / `validation_chmod` with the active-profile
lists: source checks intentionally include inactive categories. Test merged
public/overlay conflicts using synthetic fixtures, not private content.

Start with [validation helpers](../../../cli/src/infra/config/validation.rs),
[structural preflight](../../../cli/src/app/config/preflight.rs), and
[check tasks](../../../cli/src/app/validation/checks.rs).
Use `toml-configuration` when the schema or loader also changes.
