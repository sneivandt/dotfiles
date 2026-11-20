---
title: Dotfiles repository
description: Synchronize the dotfiles checkout and private overlay with fast-forward Git updates while preserving local changes and divergent branches.
---

# Dotfiles repository

The checkout is both the source of your configuration and the live target of
many home-directory links. Updating it can therefore change application
behavior before any later task runs. This task advances an eligible Git branch
to its upstream without discarding local work; it does not update the CLI binary.

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

Apply uses Git 2.41 or later to fetch each upstream and fast-forward to the commit
reported by that fetch. If the local branch or commit changes during the fetch,
review the checkout and retry. A local file that would be overwritten blocks the merge,
including a file Git normally ignores. A successfully changed checkout triggers
the guarded [configuration
restart](../development/architecture.md#repository-updates-are-a-restart-boundary) before later tasks
run. The parent keeps the run lock. Updating two repositories is not a transaction: a
later failure does not undo an earlier success. If the main checkout advances
but the overlay merge fails, the repository task reports one applied update
and one failure.

If a branch is ahead or diverged after fetch, independent tasks can still run
from the original configuration **only if** the branches, commits, and tracked
files in all selected checkouts still match their pre-fetch state. Otherwise
the update stops; review the change before retrying.

Dry-run may query remote refs with `git ls-remote`. If the upstream commit is missing
locally, dry-run reports that it cannot verify a fast-forward. It leaves the local
repository unchanged. If broken configuration prevents update from starting, use
`dotfiles update --only repository` to refresh the checkout before retrying.

For update, `--no-repo-update` removes this task even from dependency expansion. It does
**not** disable binary self-update or all network access.

## Configuration

The task uses the current Git branch and its configured upstream. The main checkout and
[private overlay](../guides/private-overlays.md) are selected through CLI options and saved
selection settings, not a repository-update TOML file.
Git's system configuration remains in effect, including proxy, certificate,
credential-helper, and URL-rewrite settings.

To understand an unmet result, inspect each affected checkout:

```sh
git status --short --branch
git branch -vv
git remote -v
```

An upstream is the branch Git associates with your local branch. A fast-forward
moves that branch to a descendant commit without creating a merge commit.
Git's [pull reference](https://git-scm.com/docs/git-pull) explains upstreams and
integration choices; dotfiles deliberately does not choose merge or rebase
recovery for you.

## Preview and apply

Preview synchronization of the selected checkout and overlay:

```sh
dotfiles update --only repository --dry-run
```

Check the affected branches and any fast-forward limitation. If the task is
unmet, use the Git inspection commands above to resolve local changes or
upstream history before retrying. Only [apply the reviewed
update](task-reference.md#preview-and-apply) when you are ready for changes to
live linked configuration as well as tracked files.

### Recover from an interrupted merge

Before it starts a merge, Dotfiles writes a recovery marker in that worktree's
Git directory. It clears the marker only after verifying the new HEAD and index.
If the merge fails, is cancelled, or the process dies, the next update that
selects `repository` stops before other tasks run. A different linked worktree
is not marked. An ordinary dirty checkout with no marker keeps its usual behavior.

1. Make sure the interrupted Dotfiles and Git processes have exited. In the
   affected checkout, run `git status --short --ignored`, `git diff`, and
   `git diff --cached`. The marker normally records the before and intended
   commit IDs. Even a clean tracked status does not rule out a partial
   untracked or ignored file.
2. Save anything you need **outside the checkout**, including untracked and
   ignored files. Inspect affected files, including those already linked into
   your home, and repair only what you have verified. Don't use a blanket
   reset or clean; either can discard unrelated work.
3. Once satisfied with the checkout and index, remove **only** the recovery
   marker at the exact path reported by Dotfiles and retry the original update.
   Removing the marker acknowledges your review; it does not repair files.
   Don't skip `repository` to apply an unreviewed checkout.

## Removal

There is no remove action for repository synchronization. Removing other integrations
does not reset Git history or restore an earlier checkout. Use Git to review and
deliberately change repository history separately.
