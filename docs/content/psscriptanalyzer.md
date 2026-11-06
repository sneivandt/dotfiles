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
Install the analyzer module for PowerShell 7 and make `pwsh` available on PATH;
Windows PowerShell cannot substitute for it. The check does not apply Windows
settings or run a private overlay's scripts.

Use the [testing guide](testing.md#choosing-coverage) to select native Windows and
script-runtime checks when changing behavior. Static analysis is not a substitute for
those tests.

Use the reported filename, line, and rule name to locate a finding in the
tracked source. Review the rule's intent before suppressing it, particularly
for argument handling or compatibility with Windows PowerShell. A clean
analysis result cannot prove Windows process quoting, UAC, or registry
behavior. Those need the isolated runtime checks selected for the change.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only psscriptanalyzer --fail-on-skip
```

With `--fail-on-skip`, a missing `pwsh` executable fails the command. If `pwsh`
is present but the analyzer module is missing or broken, the task fails too.
Fix the reported prerequisite or rule finding, then rerun.
