---
title: ShellCheck
description: Analyzes shell scripts in the main repository.
---

# ShellCheck

| Task | Selection |
|---|---|
| Selector | `shellcheck` |
| Commands | `check` |

## What it checks

Runs ShellCheck against shell scripts in the main repository. Install ShellCheck before
using this task. It checks script source, not the behavior of a running shell or a
private overlay script.

For changes to managed shell startup and helpers, also use the isolated [shell
tests](testing.md#choosing-coverage). Passing static analysis alone does not verify
startup, completion, or history behavior.

## Run the check

With an [installed launcher and saved profile](getting-started.md):

```sh
dotfiles check --only shellcheck --fail-on-skip
```

Missing optional tools produce a visible skip. `--fail-on-skip` makes that an
unsuccessful check instead of allowing it to look like a pass. Fix failures and unmet
prerequisites before applying configuration.

This task has no separate TOML settings and belongs only to `check`; it does not run the
update graph. See [CLI validation](testing.md#cli-validation) for a wider check of the
selected checkout.
