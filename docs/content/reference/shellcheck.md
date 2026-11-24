---
title: ShellCheck
description: Lint repository shell scripts with ShellCheck, understand file discovery and configuration, and interpret shell portability diagnostics.
---

# ShellCheck

[ShellCheck](https://www.shellcheck.net/wiki/) is a static analyzer for supported
shell dialects. It finds source-level problems such as unsafe expansion and
quoting mistakes without running the script's setup actions. It is not a
general validator for every shell language or application configuration file.

| Task detail | Value |
|---|---|
| Selector | `shellcheck` |
| Commands | `check` |
| Scope and tools | Main-repository shell scripts; requires ShellCheck |
| Task dependencies | None; configuration loads before task selection |
| Implementation | [app/validation/checks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/validation/checks.rs) |

## What it checks

Runs ShellCheck against supported shell scripts in the main repository. Install
ShellCheck first; the task inspects script source only and does not test a running
shell or private overlay scripts.

When changing managed shell startup or helper scripts, also run the isolated
[shell tests](../development/testing.md#choosing-coverage), since passing static
analysis alone does not cover startup, completion, or history behavior.

Findings report an `SC` rule number you can look up in the upstream wiki before fixing
the script or adding a narrow suppression. Shared discovery rules select supported
shell files rather than every file in the checkout, and zsh-specific startup behavior
needs its own tests. Keep each script's intended shell and shebang aligned instead of
changing the dialect just to quiet a warning.

## Run the check

With an [installed launcher and saved profile](../guides/getting-started.md):

```sh
dotfiles check --only shellcheck --fail-on-skip
```

Install ShellCheck and POSIX `sh` before running this check; `--fail-on-skip` turns a
missing analyzer into a failure. Fix the reported script and rerun, and run the
[shell fixtures](../development/testing.md#application-configuration) whenever a change
affects startup behavior.
