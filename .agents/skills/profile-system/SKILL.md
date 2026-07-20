---
name: profile-system
description: >
  Profile resolution and category selection for this dotfiles repo. Use when
  changing profile definitions, selection priority, persistence, active or
  excluded categories, or profile-to-sparse-checkout integration.
---

# Profile System

Profiles control which files are checked out and which config sections are
processed. Profile resolution is in `cli/src/app/config/profiles.rs`.

## Currently Configured Profiles

- **`base`**: Minimal core config without desktop apps (excludes desktop)
- **`desktop`**: Full config with desktop apps (includes desktop)

Platform categories (`linux`, `windows`, `arch`) are auto-detected — users only choose between `base` and `desktop`.

## How Profiles Work

1. **Profile resolution**: `profiles::resolve()` computes `active_categories` and `excluded_categories`
2. **Platform auto-detection**: `Platform::excludes_category()` auto-adds/excludes platform categories (`linux`, `windows`, `arch`)
3. **Always-active**: `base` is always in `active_categories`
4. **Config filtering**: `toml_loader::filter_by_categories(sections, active_categories)` includes sections where ALL categories are active
5. **Sparse checkout**: `manifest.toml` uses the same AND logic, but filters against `excluded_categories`

## Profile Selection Priority

```
CLI `-p` (`--profile`) > DOTFILES_PROFILE env var > git config dotfiles.profile > interactive prompt
```

`profiles::resolve_from_args()` persists interactive selection to repository-local git config as
`dotfiles.profile`. Preserve the priority order and keep persistence failure
visible without turning a valid selection into a failed install.

`Profile` carries its name plus active and excluded categories.
`active_categories` always includes `base`; platform categories are added or
excluded by platform capability, while users choose the role profile.

## Section Naming Convention

- **Profile names** (in `profiles.toml`): e.g., `[base]`, `[desktop]`
- **Config sections** (all other TOML files): Hyphen-separated — `[arch-desktop]`
  - AND logic: all categories must be in `active_categories`
- **Platform categories** (`linux`, `windows`, `arch`): Auto-detected, use in config sections for platform-specific items (e.g., `[linux]` for chmod entries)

## Adding a New Profile Definition

Add to `conf/profiles.toml`:
```toml
[my-profile]
include = ["mycategory"]
exclude = []
```

The loader can read additional profile definitions dynamically from
`profiles.toml` via `load_definitions()`; architecture is not hardcoded to only
two names.

Before adding a new profile, review and update assumptions across:

- tests and snapshots that assume only `base`/`desktop`
- user-facing docs and examples
- config/validation rules tied to known profile names
- sparse-checkout behavior and manifest coverage for new category combinations
