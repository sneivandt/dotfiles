---
title: Symlink sources
description: Check symlink sources and permission paths across active and inactive categories, expand supported globs, and diagnose missing tracked files.
---

# Symlink sources

A declaration names content that must exist in the source checkout. This task
checks those link and permission inputs, including categories the current
machine does not select. It helps catch a rename or incomplete commit before
another platform uses the affected configuration.

| Task detail | Value |
|---|---|
| Selector | `symlink-sources` |
| Commands | `check` |
| Scope and tools | Declared link and permission sources, including inactive categories |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Validates symlink declarations, glob expansion, and file-permission sources across
both active and inactive categories, catching missing tracked files before another
profile or platform depends on them.

Review the declaration rules in [Home symlinks](home-symlinks.md#configuration) and
[File permissions](file-permissions.md#configuration). Passing a source check confirms
the repository inputs exist; it does not prove that existing home targets can be
replaced or that the selected applications work.

When a path is reported missing, inspect both the declaration and its declaring
repository's source tree, then fix the spelling or add the missing tracked file rather
than creating an installed copy in home. Because permission entries can resolve through
a declared symlink to its source, moving a parent directory can trigger more than one
finding. Remember that glob syntax only allows standalone `*` segments, as covered in
[Glob patterns](home-symlinks.md#glob-patterns).

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only symlink-sources
```

Fix any missing paths in the declaration or source tree and rerun, or run
[CLI validation](../development/testing.md#cli-validation) to include declaration
diagnostics and required-file checks.
