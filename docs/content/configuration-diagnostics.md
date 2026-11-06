---
title: Configuration diagnostics
description: Reports all configuration diagnostics, including warnings that make the check unsuccessful.
---

# Configuration diagnostics

Configuration can be valid TOML but still describe conflicting or unsupported
state. This task reports those semantic diagnostics for the selected profile
and overlay. It is not a package-availability check or proof that the desired
configuration can be applied on this machine.

| Task detail | Value |
|---|---|
| Selector | `config-diagnostics` |
| Commands | `check` |
| Scope and tools | Loaded configuration |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Validates the combined configuration for the selected profile and overlay. Diagnostics
include invalid declarations and conflicts between entries. Warning-severity diagnostics
also fail this task, so address the reported cause before applying changes.

All required TOML files must parse before task selection. `--only config-diagnostics`
cannot bypass malformed configuration. See [Configuration
basics](configuration-basics.md#loading-and-reload-behavior).

Each finding includes a severity, source, item, diagnostic code, and message.
Use the source and item to find the declaration, then resolve the reported
conflict or invalid value. A warning is still a failed check here, even when
startup permits the command to continue. Do not suppress it by removing the
validation task from a wider check.

If parsing fails before a task row appears, fix the required file first.
If the problem is a missing tracked path rather than a declaration conflict,
[Symlink sources](symlink-sources.md) gives the complementary source check.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only config-diagnostics
```

Fix each finding in its named source, then rerun the check. For source-file
checks and analyzers too, use [CLI validation](testing.md#cli-validation).
