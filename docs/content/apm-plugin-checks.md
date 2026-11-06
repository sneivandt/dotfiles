---
title: APM plugin checks
description: Validates local APM plugins without deploying them.
---

# APM plugin checks

| Task | Selection |
|---|---|
| Selector | `apm-plugins` |
| Commands | `check` |

## What it checks

Runs `apm pack --dry-run --verbose` for local plugins in the main repository. It needs
the APM CLI. It does not deploy plugin content or validate every remote package and
private overlay script.

Use [Agent packages](agent-packages.md) for deployment behavior and [Agent
configuration](agent-configuration.md#validate-preview-and-apply) for the
package-maintenance workflow.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only apm-plugins --fail-on-skip
```

Missing optional tools produce a visible skip. `--fail-on-skip` makes that an
unsuccessful check instead of allowing it to look like a pass. Fix failures and unmet
prerequisites before applying configuration.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
