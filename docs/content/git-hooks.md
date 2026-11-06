---
title: Git hooks
description: Repository commit and maintenance hooks.
---

# Git hooks

[Git hooks](https://git-scm.com/docs/githooks) are programs Git invokes at
events such as committing. This task installs the dotfiles repository's
contributor entrypoints. It does not install hooks into every repository or
manage the personal Git templates described in
[Application settings](application-settings.md#git-templates).

| Task detail | Value |
|---|---|
| Selector | `git-hooks` |
| Commands | `update`, `remove` |
| Availability | Repository hook sources |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Dotfiles repository](dotfiles-repository.md) during update |
| Implementation | [git/hooks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/git/hooks.rs) |

## What it does

Copies extensionless files from `hooks/` after repository synchronization, honoring
`core.hooksPath` and the common hooks directory of linked worktrees. When the
destination is already the tracked source, it is neither copied over itself nor removed.

Foreign or locally modified hooks are preserved and reported as conflicts, even when
their filenames match managed hooks. Back them up and move them aside before installing
a replacement. Dotfiles does not combine hooks. [Hooks](contributing.md#git-hooks)
explains what the installed hooks do.

## Configuration

The sources are the extensionless files in
[hooks/](https://github.com/sneivandt/dotfiles/tree/main/hooks/). There is no TOML
declaration. Git decides the destination through its hooks configuration.

Inspect the hook destination before installing:

```sh
git config --show-origin --get core.hooksPath
git rev-parse --git-common-dir
dotfiles update --only git-hooks --dry-run
```

No output and status 1 from the first command means no override is configured. Dotfiles
honors a repository-local `core.hooksPath`, resolving relative values from the
repository root; otherwise it uses the common Git directory's `hooks/`, including
worktrees. An inherited shared override blocks installation until you choose a
repository-local destination.

Installation preserves foreign or locally modified hooks and reports a conflict. Back
them up and move them aside before retrying. Matching legacy copies can be adopted;
subsequent updates use a sibling `.dotfiles-owned` hash record to check ownership. Keep
that record with the installed hook. Extensionless entrypoints are copied, while `.sh`
helpers and pattern files run from the checkout. Reinstall after changing the copied
`pre-commit`; helper edits take effect immediately.

Copying the entrypoint does not copy its toolchain. A hook can run while an
optional analyzer is unavailable, so inspect its reported skips and use the
documented pre-push checks for complete coverage. If a changed entrypoint
seems not to run, compare the installed copy and ownership record with the
tracked source before changing Git's hook configuration.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only git-hooks --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Removal deletes hooks whose ownership can be verified. Modified hooks, unrelated
filenames, and a destination that is the tracked source itself are preserved. Preview
with `dotfiles remove --only git-hooks --dry-run` before applying.
