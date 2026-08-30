---
name: profile-system
description: >
  Use for profile resolution in cli/src/app/config/profiles.rs, profile
  selection precedence/persistence, or active/excluded category computation.
---

# Profile System

Profiles choose role categories; platform capabilities add or exclude platform
categories. `base` is always active.

## Resolution contract

Selection priority is:

```text
--profile > DOTFILES_PROFILE > repository dotfiles.profile > interactive prompt
```

Interactive selection is persisted to repository-local Git config. A persistence
failure is visible but does not invalidate an otherwise valid selection.

`Profile` carries its name, active categories, and excluded categories.
Configuration sections match when every hyphen-separated category is active.

## Changing profiles

- Define profiles in `conf/profiles.toml`; do not hardcode the known names into
  loaders.
- Keep platform category detection separate from user profile selection.
- Preserve selection precedence and non-interactive behavior.
- Review config sections, validators, docs, and tests that assume the existing
  profiles.
- Add resolution, persistence, and category-set tests.

Use `toml-configuration` when category parsing changes.
