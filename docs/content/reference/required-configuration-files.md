---
title: Required configuration files
description: Check required conf TOML files in the main repository before updates and distinguish missing files from configuration parsing failures.
---

# Required configuration files

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

The [configuration file index](../guides/configuration-basics.md#files) lists the required files.
Parsing and field validation happen while loading configuration, before this task can
run.

If startup reports a required file missing, restore the tracked file before
retrying; `--only config-files` cannot get past the loader. Do not copy a
private overlay file into the public checkout as a workaround. A missing
`hooks/` directory produces a warning in this task, unlike a missing required
TOML file. File presence alone does not establish semantic correctness:
use [Configuration diagnostics](configuration-diagnostics.md) for that.

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only config-files
```

Restore missing tracked files before retrying. To check their declarations
and source paths too, use [CLI validation](../development/testing.md#cli-validation).
