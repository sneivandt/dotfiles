---
title: Dotfiles repository
description: Fast-forward updates of the checkout and overlay.
---

# Dotfiles repository

| Task detail | Value |
|---|---|
| Selector | `repository` |
| Commands | `update` |
| Availability | Eligible Git checkout |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [repository/update.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/repository/update.rs) |

## What it does

Synchronizes the main checkout and an overlay with a `.git` entry. Tracked local changes
prevent synchronization; untracked files are ignored by that readiness check. Detached
HEAD is inapplicable. A missing upstream, local-only commits, or diverged history leaves
the task unmet. It does not reset or rebase the branch. Other branch-query errors remain
failures.

Apply fetches and fast-forwards. A local file that would be overwritten blocks the
merge, including a file Git normally ignores. A successfully changed checkout triggers
the guarded [configuration
restart](architecture.md#repository-updates-are-a-restart-boundary) before later tasks
run. The parent keeps the run lock. Updating two repositories is not a transaction: a
later failure does not undo an earlier success.

Dry-run may query remote refs with `git ls-remote`. If the upstream commit is missing
locally, dry-run reports that it cannot verify a fast-forward. It leaves the local
repository unchanged. If broken configuration prevents update from starting, use
`dotfiles update --only repository` to refresh the checkout before retrying.

For update, `--no-repo-update` removes this task even from dependency expansion. It does
**not** disable binary self-update or all network access.

## Configuration

The task uses the current Git branch and its configured upstream. The main checkout and
[private overlay](private-overlays.md) are selected through CLI options and saved
selection settings, not a repository-update TOML file.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only repository --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

There is no remove action for repository synchronization. Removing other integrations
does not reset Git history or restore an earlier checkout. Use Git to review and
deliberately change repository history separately.
