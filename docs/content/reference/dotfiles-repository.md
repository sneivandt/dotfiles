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

Synchronizes the main checkout and any overlay that has a `.git` entry. Tracked local
changes prevent synchronization, while untracked files do not block the readiness check.
A detached `HEAD` makes the task inapplicable, and a missing upstream, local-only commits,
or diverged history leaves it unmet instead of resetting or rebasing your branch. Other
branch-query errors fail the task.

Apply uses Git 2.41 or later to fetch each upstream and fast-forward to the fetched
commit. If the local branch or commit changes while Dotfiles is preparing tasks, review
the checkout and retry. Any local file that would be overwritten blocks the merge,
including files Git normally ignores. When a checkout advances, Dotfiles performs a
guarded [configuration
restart](../development/architecture.md#repository-updates-are-a-restart-boundary) before
later tasks run, keeping the parent run lock intact. Updating two repositories is not a
transaction: a later failure does not undo an earlier success, so if the main checkout
advances and the overlay merge fails, the task reports one applied update and one failure.

If a branch turns out to be ahead or diverged after fetch, independent tasks can still
run from the original configuration **only if** the branches, commits, and tracked
files in all selected checkouts still match their pre-fetch state. Otherwise
the update stops so you can inspect the change before retrying.

Dry-run may query remote refs with `git ls-remote`, leaving the local repository
untouched. If the upstream commit is not present locally, dry-run reports that it cannot
verify a fast-forward. When broken configuration prevents a full update from starting,
run `dotfiles update --only repository` to refresh the checkout first.

For `update`, `--no-repo-update` skips this task even during dependency expansion. It does
**not** disable binary self-update or all network access.

## Configuration

The task uses your current Git branch and its configured upstream. The main checkout and
[private overlay](../guides/private-overlays.md) come from CLI options and saved
selection settings rather than a repository-update TOML file.
Git's system configuration remains in effect, including proxy, certificate,
credential-helper, and URL-rewrite settings.

To understand an unmet result, inspect each affected checkout:

```sh
git status --short --branch
git branch -vv
git remote -v
```

An upstream is the remote branch Git tracks for your local branch, and a fast-forward
moves the local branch straight to a descendant commit without creating a merge commit.
Git's [pull reference](https://git-scm.com/docs/git-pull) covers upstreams and
integration strategies; Dotfiles deliberately leaves merge or rebase recovery to you.

## Preview and apply

Preview synchronization of the selected checkout and overlay:

```sh
dotfiles update --only repository --dry-run
```

Review the affected branches and any fast-forward limitation. If the task reports unmet,
use the Git commands above to reconcile local changes or upstream history first. Only
[apply the reviewed update](task-reference.md#preview-and-apply) once you are ready for
changes to live linked configuration as well as tracked files.

### Recover from an interrupted merge

Before starting a merge, Dotfiles writes a recovery marker inside that worktree's
Git directory and clears it only after verifying the new `HEAD` and index.
If the merge fails, is cancelled, or the process exits unexpectedly, `update` and `remove`
refuse to proceed while the main checkout or selected overlay is marked. That check runs
before binary self-update or loading task configuration, even with `--only`,
`--no-repo-update`, or `--dry-run`. `dotfiles check` still inspects the
configuration, reports that recovery is pending, and exits with an error while a
selected checkout is marked. You can still inspect retained runs with `dotfiles log`.
A different linked worktree is unaffected, and an ordinary dirty checkout with no marker
behaves as usual.

The marker cannot protect a home link that already points into the checkout.
If `dotfiles` on `PATH` no longer starts, its launcher may be calling a missing
or partially written wrapper inside the checkout. From the intended checkout, use an
existing `./bin/dotfiles` (`.\bin\dotfiles.exe` on Windows) **only if you can
verify it is the trusted binary you previously installed** (for example, by matching
its digest to a known-good copy), and run `./bin/dotfiles log --list` instead of
the wrapper. When in doubt, use Git alone:
`git rev-parse --git-path dotfiles-merge-in-progress` locates the
marker in the affected worktree without executing any checkout script.

1. Confirm that the interrupted Dotfiles and Git processes have exited. In the
   affected checkout, run `git status --short --ignored`, `git diff`, and
   `git diff --cached`. The marker normally records the before and intended
   commit IDs; remember that a clean tracked status does not rule out a partial
   untracked or ignored file.
2. Back up anything you need **outside the checkout**, including untracked and
   ignored files. Inspect affected files, including those already linked into
   your home, and repair only what you have verified. Avoid a blanket
   reset or clean, which can discard unrelated work.
3. If Git refuses a repair or retry because `index.lock` exists, locate it with
   `git rev-parse --git-path index.lock` in the affected checkout. This is
   **Git's lock, not the recovery marker**. Stop Git clients, including
   background editor operations. Only after verifying no process owns the
   lock, move that exact orphaned lock aside to your backup; never move the
   index itself, and leave both files alone if ownership is uncertain. Recheck
   status and both diffs after repairing.
4. Once you are satisfied with the checkout and index, remove **only** the recovery
   marker at the exact path reported by Dotfiles and retry the original update.
   Removing the marker acknowledges your review; it does not repair files.

## Removal

There is no remove action for repository synchronization. Removing other integrations
does not reset Git history or restore an earlier checkout; use Git directly whenever
you want to change repository history.
