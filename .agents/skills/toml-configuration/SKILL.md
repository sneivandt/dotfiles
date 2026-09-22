---
name: toml-configuration
description: >
  Use when editing conf/*.toml data, typed models, loaders, category sections,
  or overlay merging. Prefer a narrower package, profile, or symlink skill for
  domain-only data changes. Not for validator implementation alone.
---

# TOML Configuration

## Parsing rules

- Model config with typed Serde structures.
- Add `#[serde(deny_unknown_fields)]` to every deserialized struct.
- Do not use `#[serde(untagged)]`; use
  `crate::infra::config::StringOrTable<T>` for string-or-table entries.
- Structural mistakes fail deserialization. Domain-invalid values that need
  aggregated diagnostics should deserialize to an explicit parsed/invalid form
  and fail semantic validation instead.
- Preserve the two-layer absence policy: the app loader requires the main config
  inventory, while reusable section loaders accept missing optional overlay
  files. `ConfigLoader` owns this policy through `read_config`; domain decoders
  consume its already parsed `ConfigDocument` and never reopen a file.
- Keep deterministic order where output or diagnostics expose it.

## Category sections

Section names are hyphen-separated category conjunctions:

```toml
[arch-desktop]
items = ["example"]
```

Every category must be active. Do not use dotted names; TOML interprets them as
nested tables. The built-in tags are `base`, `desktop`, `linux`, `windows`,
`arch`, and `wsl`; preflight rejects unknown, empty, and repeated tags. Add a
new tag to the profile resolver's known categories before using it in TOML.

Prefer `config_section!` and `SectionLoader` for ordinary category-filtered,
overlay-aware lists. Loaders return typed desired state and contain no task
behavior.

## Extend the schema completely

Follow the [aggregate loader](../../../cli/src/app/config/mod.rs) and
[category validation](../../../cli/src/app/config/preflight.rs). For a new
section, update the typed model and required inventory where applicable,
`SectionLoader` collection, config/handle inventory, semantic validation, real
data, fixtures, and owning task. Do not copy a hand-written public/overlay merge.

Preserve public-then-overlay ordering and per-entry provenance for source paths.
Overlay lists append; they are not a generic last-writer-wins override. Test
conflicting desired values rather than silently selecting one.
Use `SectionLoader::collect_views` when both active and unfiltered validation
inputs are needed. It decodes each source once and stamps origins before deriving
the views. Use `collect_filtered` for active-only lists and `collect` for registry
values. Preserve source spans when decoding; do not reparse to recover diagnostics.

Cover unknown fields, malformed types, optional overlay absence, required main
absence, category inclusion/exclusion, and merged data. Use the TOML checks in
[Testing](../../../docs/TESTING.md#choosing-coverage).

Use `config-validation` for diagnostic and drift-test details. Profile and
overlay semantics belong to their respective skills; the complete contributor
checklist lives in
[Contributing](../../../docs/CONTRIBUTING.md#change-checklists).
