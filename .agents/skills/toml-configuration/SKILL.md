---
name: toml-configuration
description: >
  TOML structure, category filtering, and loader conventions for conf/ and
  cli/src/config/. Use when adding or changing declarative configuration.
---

# TOML Configuration

Configuration under `conf/` is declarative desired state and is deserialized
with Serde. Keep parsing, validation, resource wiring, and real config changes
in sync.

## Stable Conventions

- Use typed Serde models; do not add ad hoc text parsing.
- Use `#[serde(untagged)]` when an entry intentionally supports both a concise
  string and structured metadata.
- Keep deterministic section ordering where output or diagnostics depend on it.
- Include trailing commas in multiline arrays.
- Prefer `config_section!` for ordinary category-filtered lists.
- Use `load_optional_config()` only when a missing file legitimately means
  empty configuration; otherwise use `load_required_config()`.

## Category Sections

Most files use table names as category expressions:

```toml
[arch-desktop]
items = [
  "example",
]
```

Category names are lowercased and split on `-`. Matching uses AND semantics:
every category in the section name must be present in the category set supplied
by the caller.

- Normal config loaders filter against `active_categories`.
- `manifest.toml` filters against `excluded_categories`.
- Do not use dotted table names for category expressions; dots create nested
  TOML tables.

Built-in categories are `base`, `desktop`, `linux`, `windows`, and `arch`.
Profiles may also define custom categories. Coordinate new categories with
profiles, manifest handling, validation, tests, and documentation.

## Loader Pattern

Keep one clear path from file to desired state:

1. Deserialize sections into typed values.
2. Convert sections into category plus item collections.
3. Filter with the shared category helper.
4. Merge overlay entries through `SectionLoader` when that config surface
   supports overlays.
5. Return typed values without embedding task behavior in the loader.

`cli/src/config/mod.rs` owns top-level loading. Domain modules under
`cli/src/config/` own their formats and validators. Consult the target module
and the real file under `conf/` rather than relying on a duplicated inventory in
this skill.

## Change Checklist

For a new or changed config-backed surface:

1. Update the config type and loader.
2. Add or update validation.
3. Update the real `conf/*.toml` file.
4. Wire the resource or operation and its task.
5. Register static tasks and export modules where required.
6. Add focused parser, validation, and drift coverage.
7. Review profile, manifest, and overlay implications.

## Validation

- Use `config-validation` for validator and drift-test conventions.
- Run the repository test command for real-config validation.
- Use `cross-platform-verification` when Rust code changes.

## Rules

- Configuration describes desired state; orchestration stays in tasks.
- Invalid or unsafe paths must produce explicit validation diagnostics.
- Preserve strong typing across strings, numbers, booleans, and structured
  values.
- Do not duplicate exhaustive config-file inventories in skills.
