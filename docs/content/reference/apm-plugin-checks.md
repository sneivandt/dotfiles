---
title: APM plugin checks
description: Validate local Agent Package Manager plugins with apm pack in dry-run mode without installing or deploying their content.
---

# APM plugin checks

Validates local APM plugin packages in the main repository before deployment.
It checks package structure and manifests via APM itself, not whether an
instruction is effective, a tool is safe, or every deployment target works.

| Task detail | Value |
|---|---|
| Selector | `apm-plugins` |
| Commands | `check` |
| Scope and tools | Local main-repository plugins; requires APM |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Runs `apm pack --dry-run --verbose` against each local plugin under
[symlinks/apm/plugins/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/plugins/).
It never deploys plugin files and does not validate remote packages or private
overlay scripts. Fix any reported error in the tracked plugin source rather
than an installed or generated copy.

See the [APM command reference](https://microsoft.github.io/apm/reference/) for
packaging details, [Agent packages](agent-packages.md) for deployment behavior,
and [Validate, preview, and apply](../guides/agent-configuration.md#validate-preview-and-apply)
for the maintenance workflow (a clean pack check does not replace reviewing a
deployment preview).

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only apm-plugins --fail-on-skip
```

Install `apm` first so `--fail-on-skip` passes instead of failing on a missing
binary, fix any errors in the named plugin, and rerun before
[previewing deployment](../guides/agent-configuration.md#validate-preview-and-apply).
