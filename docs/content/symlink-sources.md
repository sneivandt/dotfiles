---
title: Symlink sources
description: Checks declared symlink and permission sources across active and inactive categories.
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

Checks symlink declarations, glob expansion, and file-permission sources, including
entries in inactive categories. This catches missing tracked content before a different
profile or platform selects it.

Review the declarations in [Home symlinks](home-symlinks.md#configuration) and [File
permissions](file-permissions.md#configuration). A source check does not prove that
existing home destinations can be replaced or that the selected applications work.

When a path is reported missing, check both the declaration and its declaring
repository's source tree. Fix the source spelling or include the missing
tracked content; creating an installed home file is not a substitute for a
missing symlink source. Permission entries can map through a declared link
to its source, so moving a parent directory can affect more than one finding.
Glob syntax is deliberately limited to complete `*` segments, as described in
[Glob patterns](home-symlinks.md#glob-patterns).

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only symlink-sources
```

Correct missing paths in the declaration or source tree and rerun. Use
[CLI validation](testing.md#cli-validation) to include declaration diagnostics
and required-file checks.
