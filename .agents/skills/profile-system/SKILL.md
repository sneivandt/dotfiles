---
name: profile-system
description: >
  Use for profile resolution in cli/src/app/config/profiles.rs and
  cli/src/app/config/profiles/,
  selection precedence/persistence, or active category computation. Not for
  ordinary edits to an existing category's package list.
---

# Profile System

The CLI has built-in `base` and `desktop` profiles. Resolution starts with
`base`, adds `desktop` for the desktop profile, then adds detected platform
categories and sorts/deduplicates the result.

## Resolution contract

Selection priority is:

```text
--profile > DOTFILES_PROFILE > repository dotfiles.profile > interactive prompt
```

Interactive selection is persisted to repository-local Git config. A persistence
failure is visible but does not invalidate an otherwise valid selection.

`Profile` carries its name and active categories. Configuration sections match
when every hyphen-separated category is active.

Non-interactive resolution without a selection is an error, not an implicit
default profile. An invalid higher-priority selection must not silently fall
back. Read-only discovery uses `resolve_read_only()` and never prompts for a
profile or persists a choice.

## Changing profiles

- Keep profile names and descriptions in the built-in inventory in
  `profiles.rs`; keep category resolution in `profiles/resolution.rs`.
- Keep platform category detection separate from user profile selection.
- Preserve selection precedence and non-interactive behavior.
- Update the preflight category list when adding a built-in category. Custom
  categories are not supported.
- Review config sections, validators, docs, completions, and tests that assume
  the existing profiles.
- Add resolution, persistence, category-set, missing-selection, and read-only
  discovery tests.

Start with [profile selection](../../../cli/src/app/config/profiles.rs) and
[category resolution](../../../cli/src/app/config/profiles/resolution.rs).
Use `toml-configuration` when category parsing changes.
