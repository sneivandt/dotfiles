---
title: APM plugin checks
description: Validates local APM plugins without deploying them.
---

# APM plugin checks

An APM plugin is a package of agent content with its own manifest and
packaging rules. This task asks APM to validate local plugin packages before
deployment. It checks package shape, not whether an instruction is useful,
a tool is trustworthy, or every deployment target works.

| Task detail | Value |
|---|---|
| Selector | `apm-plugins` |
| Commands | `check` |
| Scope and tools | Local main-repository plugins; requires APM |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Runs `apm pack --dry-run --verbose` for local plugins in the main repository. It needs
the APM CLI. It does not deploy plugin content or validate every remote package and
private overlay script.

Use [Agent packages](agent-packages.md) for deployment behavior and [Agent
configuration](agent-configuration.md#validate-preview-and-apply) for the
package-maintenance workflow.

The local sources are beneath
[symlinks/apm/plugins/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/plugins/).
Fix a reported manifest or packaging error in that source plugin, not in an
installed or generated copy. The
[APM command reference](https://microsoft.github.io/apm/reference/) explains
packaging commands. A successful local pack check does not resolve remote
dependencies, establish authentication, or replace a reviewed deployment
preview.

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
