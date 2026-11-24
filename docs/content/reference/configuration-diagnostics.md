---
title: Configuration diagnostics
description: Report all configuration diagnostics, including warnings that make the check unsuccessful.
---

# Configuration diagnostics

Valid TOML can still contain conflicting, unsupported, or misconfigured
declarations. This task reports semantic diagnostics across your loaded profile
and overlay (it does not verify package availability or whether this host can
apply every declaration).

| Task detail | Value |
|---|---|
| Selector | `config-diagnostics` |
| Commands | `check` |
| Scope and tools | Loaded configuration |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Aggregates semantic diagnostics across the combined main and overlay
configuration for the selected profile, including invalid declarations and
entry conflicts. Both error- and warning-severity findings fail this task, even
when `update` would allow a warning to continue, so fix the underlying cause
rather than skipping the check.

Required TOML files must parse before task filtering runs, so `--only
config-diagnostics` cannot bypass syntax errors (see
[Loading and reload behavior](../guides/configuration-basics.md#loading-and-reload-behavior)).
Each finding prints its severity, source, item, diagnostic code, and message so
you can jump straight to the offending declaration. For missing tracked files
on disk rather than declaration conflicts, run
[Symlink sources](symlink-sources.md).

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only config-diagnostics
```

Resolve each reported finding in its source file and rerun. For combined source
and analyzer checks, see
[CLI validation](../development/testing.md#cli-validation).
