---
title: Configuration diagnostics
description: Reports all configuration diagnostics, including warnings that make the check unsuccessful.
---

# Configuration diagnostics

| Task | Selection |
|---|---|
| Selector | `config-diagnostics` |
| Commands | `check` |

## What it checks

Validates the combined configuration for the selected profile and overlay. Diagnostics
include invalid declarations and conflicts between entries. Warning-severity diagnostics
also fail this task, so address the reported cause before applying changes.

All required TOML files must parse before task selection. `--only config-diagnostics`
cannot bypass malformed configuration. See [Configuration
basics](configuration-basics.md#loading-and-reload-behavior).

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only config-diagnostics
```

Read the result and fix reported declaration or source problems before applying
configuration. This check reports problems; it does not repair them.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
