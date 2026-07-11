---
name: module-organization
description: Repo-specific Rust module organization preferences for this dotfiles project. Use when adding, moving, or reorganizing modules in cli/src/.
---

# Module Organization

Use this skill when adding, moving, splitting, or reorganizing Rust modules in
this repository.

## Preferences

- Use the standard Rust module layout; do not use custom `#[path = "..."]`
  module path settings for production modules.
- `#[path = "..."]` is permitted only for externalized test modules behind
  `#[cfg(test)]` when following established repository test layouts.
- Use `mod.rs` for module wiring and public re-exports.
- Keep most implementation logic out of `mod.rs`.
- Put domain-specific logic in separate, focused files with names that describe
  the responsibility.
- Keep module boundaries easy to navigate: `mod.rs` should show what exists, not
  become the place where everything lives.
- If a module grows multiple responsibilities, split it by domain rather than by
  arbitrary size alone.
- Follow the existing module layout in `cli/src/`, especially the pattern of
  small `mod.rs` files plus focused implementation files.
