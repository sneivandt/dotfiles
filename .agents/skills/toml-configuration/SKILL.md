---
name: toml-configuration
description: >
  Use for typed models, loaders, category sections, or overlay merging for
  conf/*.toml. Not for validator implementation alone or non-TOML runtime
  behavior.
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
- Use `load_required_config()` unless absence intentionally means empty state.
- Keep deterministic order where output or diagnostics expose it.

## Category sections

Section names are hyphen-separated category conjunctions:

```toml
[arch-desktop]
items = ["example"]
```

Every category must be active. Do not use dotted names; TOML interprets them as
nested tables. Ordinary config filters against active categories; manifest
ownership uses the same conjunction semantics.

Prefer `config_section!` and `SectionLoader` for ordinary category-filtered,
overlay-aware lists. Loaders return typed desired state and contain no task
behavior.

## Complete a config change

1. Update the type and loader.
2. Add or update semantic validation.
3. Update the real `conf/*.toml`.
4. Wire the resource/operation and task.
5. Export and register static tasks where needed.
6. Add parser, validator, and cross-file drift tests.
7. Review profile, manifest, and overlay impact.

Use `config-validation` for diagnostic and drift-test details.
