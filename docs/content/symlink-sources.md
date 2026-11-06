---
title: Symlink sources
description: Checks declared symlink and permission sources across active and inactive categories.
---

# Symlink sources

| Task detail | Value |
|---|---|
| Selector | `symlink-sources` |
| Commands | `check` |
| Scope and tools | Declared link and permission sources, including inactive categories |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Checks symlink declarations, glob expansion, and file-permission sources, including
entries in inactive categories. This catches missing tracked content before a different
profile or platform selects it.

Review the declarations in [Home symlinks](home-symlinks.md#configuration) and [File
permissions](file-permissions.md#configuration). A source check does not prove that
existing home destinations can be replaced or that the selected applications work.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only symlink-sources
```

Read the result and fix reported declaration or source problems before applying
configuration. This check reports problems; it does not repair them.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
