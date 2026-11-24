---
title: PSScriptAnalyzer
description: Lint repository PowerShell scripts with PSScriptAnalyzer, review warning and error diagnostics, and identify analyzer prerequisites.
---

# PSScriptAnalyzer

[PSScriptAnalyzer](https://learn.microsoft.com/en-us/powershell/utility-modules/psscriptanalyzer/overview)
is PowerShell's rule-based static analyzer. This task uses it to inspect
repository scripts; it does not execute their configuration actions. Having
PowerShell installed is not enough if the analyzer module is unavailable.

| Task detail | Value |
|---|---|
| Selector | `psscriptanalyzer` |
| Commands | `check` |
| Scope and tools | Main-repository PowerShell scripts; requires PowerShell 7 (`pwsh`) and PSScriptAnalyzer |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Runs PSScriptAnalyzer against PowerShell scripts in the main repository.
Install the analyzer module for PowerShell 7 and make `pwsh` available on `PATH`;
Windows PowerShell cannot substitute for it. The check does not apply Windows
settings or inspect a private overlay's scripts.

When changing script behavior, use the
[testing guide](../development/testing.md#choosing-coverage) to pick native Windows and
script-runtime checks, since static analysis alone cannot prove Windows process
quoting, UAC, or registry behavior.

Each finding includes the filename, line number, and rule name so you can trace it in
the tracked source. Review a rule's intent before suppressing it, especially for
argument handling or Windows PowerShell compatibility.

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only psscriptanalyzer --fail-on-skip
```

With `--fail-on-skip`, a missing `pwsh` executable fails the command, and a missing or
broken analyzer module fails even without that flag. Fix the reported prerequisite or
rule finding, then rerun.
