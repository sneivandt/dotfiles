---
title: PSScriptAnalyzer
description: Analyzes PowerShell scripts in the main repository.
---

# PSScriptAnalyzer

| Task detail | Value |
|---|---|
| Selector | `psscriptanalyzer` |
| Commands | `check` |
| Scope and tools | Main-repository PowerShell scripts; requires PowerShell and PSScriptAnalyzer |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Runs PSScriptAnalyzer against PowerShell scripts in the main repository. It needs
PowerShell and the analyzer module. It does not apply Windows settings or run a private
overlay's scripts.

Use the [testing guide](testing.md#choosing-coverage) to select native Windows and
script-runtime checks when changing behavior. Static analysis is not a substitute for
those tests.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only psscriptanalyzer --fail-on-skip
```

Missing optional tools produce a visible skip. `--fail-on-skip` makes that an
unsuccessful check instead of allowing it to look like a pass. Fix failures and unmet
prerequisites before applying configuration.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
