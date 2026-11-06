---
title: Configuration files
description: Checks that required main configuration files are present.
---

# Configuration files

| Task | Selection |
|---|---|
| Selector | `config-files` |
| Commands | `check` |

## What it checks

Checks the required main TOML files and warns when the repository's `hooks/` directory
is absent. A required file may be empty, but it must exist even on a platform that does
not use its task.

The [configuration file index](configuration-basics.md#files) lists the required files.
Parsing and field validation happen while loading configuration, before this task can
run.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only config-files
```

Read the result and fix reported declaration or source problems before applying
configuration. This check reports problems; it does not repair them.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
