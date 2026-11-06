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

Runs ShellCheck against shell scripts in the main repository. Install ShellCheck before
using this task. It checks script source, not the behavior of a running shell or a
private overlay script.

For changes to managed shell startup and helpers, also use the isolated [shell
tests](testing.md#choosing-coverage). Passing static analysis alone does not verify
startup, completion, or history behavior.

Findings include an `SC` rule number. Follow that rule in the upstream wiki
to understand the warning, then correct the script or make a narrowly
justified suppression. The repository's shared discovery rules select
supported shell inputs, not every file beneath the checkout; zsh-specific
startup behavior needs its own tests. Keep the script's intended shell and
shebang consistent rather than changing dialect just to silence an analyzer.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only shellcheck --fail-on-skip
```

Install ShellCheck and POSIX `sh` before running this check. With
`--fail-on-skip`, a missing analyzer fails the command. Fix the named script
and rerun; use the [shell fixtures](testing.md#application-configuration)
when the change also affects startup behavior.
