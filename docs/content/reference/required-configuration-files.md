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

Verifies that every required main TOML file exists and warns when the repository's
`hooks/` directory is missing. A required file can be empty, as long as it is present
even on a platform that does not use its task.

The [configuration file index](../guides/configuration-basics.md#files) lists the
required files. TOML parsing and field validation run while configuration loads,
before this task starts.

If startup fails because a required file is missing, restore the tracked file first;
`--only config-files` cannot get past the loader, and you should not copy a private
overlay file into the public checkout as a workaround. Unlike a missing TOML file, a
missing `hooks/` directory produces only a warning here. Presence checks do not
validate semantics; run [Configuration diagnostics](configuration-diagnostics.md) for
that.

## Run the check

```sh
dotfiles check --only config-files
```

Restore any missing tracked files before retrying. To validate declarations
and source paths together, run
[CLI validation](../development/testing.md#cli-validation).
