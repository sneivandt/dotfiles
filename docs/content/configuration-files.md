---
title: Configuration files
description: Checks that required main configuration files are present.
---

# Configuration files

The main checkout has a required set of configuration files, even when a
platform has no entries for some domains. Keeping that structure makes an
empty declaration distinguishable from an incomplete checkout. Optional
overlay files follow different loading rules.

| Task detail | Value |
|---|---|
| Selector | `config-files` |
| Commands | `check` |
| Scope and tools | Main checkout |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Checks the required main TOML files and warns when the repository's `hooks/` directory
is absent. A required file may be empty, but it must exist even on a platform that does
not use its task.

The [configuration file index](configuration-basics.md#files) lists the required files.
Parsing and field validation happen while loading configuration, before this task can
run.

If startup reports a required file missing, restore the tracked file before
retrying; `--only config-files` cannot get past the loader. Do not copy a
private overlay file into the public checkout as a workaround. A missing
`hooks/` directory produces a warning in this task, unlike a missing required
TOML file. File presence alone does not establish semantic correctness:
use [Configuration diagnostics](configuration-diagnostics.md) for that.

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
