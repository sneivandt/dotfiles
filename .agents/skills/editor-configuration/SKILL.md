---
name: editor-configuration
description: >
  Use for shared Vim/Neovim configuration under symlinks/vim/, lazy.nvim plugin
  declarations, bootstrap pins, lockfiles, mappings, or editor startup tests.
  Not for VS Code extension resources or generic Rust editor tooling.
---

# Editor Configuration

User-facing entry points, bootstrap, and parser notes live in
[Vim and Neovim](../../../docs/content/guides/application-settings.md#vim-and-neovim).

## Non-obvious invariants

- Keep `symlinks/vim/vimrc` working in plain Vim without third-party plugins.
- Ordinary `nvim` startup can clone `lazy.nvim` and install missing plugins—
  launching the installed editor is not a read-only check.
- Keep Tree-sitter parser installation explicit; do not trigger downloads on
  ordinary startup.
- Treat `lazy-lock.json` changes as dependency updates: changing the
  `lazy.nvim` bootstrap pin in `lua/lazy-bootstrap.lua` must update its
  matching lockfile entry without advancing unrelated plugins.

## Verification

Run the plugin-free harness in
[`.github/workflows/scripts/linux/vim_tests/`](../../../.github/workflows/scripts/linux/vim_tests/)
(see [Application configuration](../../../docs/content/development/testing.md#application-configuration)).
Exercise bootstrap-pin changes against an absent manager in an isolated data
path, and keep `lazy-lock.json` untouched during verification unless updating
revisions is part of the change.
