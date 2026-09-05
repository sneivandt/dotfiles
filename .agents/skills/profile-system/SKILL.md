---
name: profile-system
description: >
  Use for conf/profiles.toml, profile resolution in cli/src/app/config/profiles.rs
  and profiles/, selection precedence/persistence, or active/excluded category
  computation. Not for ordinary edits to an existing category's package list.
---

# Profile System

Profiles choose role categories; platform capabilities add or exclude platform
categories. Resolution starts with `base`, adds profile/platform categories,
then applies exclusions and sorts/deduplicates the resulting sets.

## Resolution contract

Selection priority is:

```text
--profile > DOTFILES_PROFILE > repository dotfiles.profile > interactive prompt
```

Interactive selection is persisted to repository-local Git config. A persistence
failure is visible but does not invalidate an otherwise valid selection.

`Profile` carries its name, active categories, and excluded categories.
Configuration sections match when every hyphen-separated category is active.

Non-interactive resolution without a selection is an error, not an implicit
default profile. An invalid higher-priority selection must not silently fall
back. Read-only discovery uses `resolve_read_only()` and never prompts for a
profile or persists a choice.

## Changing profiles

- Define profiles in `conf/profiles.toml`; do not hardcode the known names into
  loaders.
- Keep platform category detection separate from user profile selection.
- Preserve selection precedence and non-interactive behavior.
- Review config sections, validators, docs, and tests that assume the existing
  profiles.
- Add resolution, persistence, and category-set tests, including exclusion
  precedence, custom categories, missing selection, and read-only discovery.

Start with [profile selection](../../../cli/src/app/config/profiles.rs) and
[category resolution](../../../cli/src/app/config/profiles/resolution.rs).
Use `toml-configuration` when category parsing changes.
