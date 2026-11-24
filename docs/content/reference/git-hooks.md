---
title: Git hooks
description: Install repository commit and maintenance hooks, honor core.hooksPath and linked worktrees, and preserve foreign or modified hooks.
---

# Git hooks

[Git hooks](https://git-scm.com/docs/githooks) are programs Git invokes at
events such as committing. This task installs the dotfiles repository's
contributor entrypoints. It does not install hooks into every repository or
manage the personal Git templates described in
[Application settings](../guides/application-settings.md#git-templates).

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
destination is the tracked source itself, Git runs that file directly and removal
leaves it untouched. On Linux, update flags any hook Git cannot execute: restore
execute permission with `chmod +x hooks/pre-commit`, or point a repository-local
`core.hooksPath` at a separate copy directory.

Foreign or locally modified hooks are preserved and reported as conflicts, even when
their filenames match managed hooks. Back them up and move them aside before installing
a replacement; Dotfiles never merges hooks. [Contributing](../development/contributing.md#git-hooks)
describes what each installed hook checks.

## Configuration

The sources are the extensionless files under
[hooks/](https://github.com/sneivandt/dotfiles/tree/main/hooks/), with no TOML
declaration. Git chooses the destination through its hooks configuration.

Inspect the hook destination before installing:

```sh
git config --show-origin --get core.hooksPath
git rev-parse --git-common-dir
dotfiles update --only git-hooks --dry-run
```

No output with exit status 1 from the first command means no override is set. Dotfiles
honors a repository-local `core.hooksPath` (resolving relative values from the
repository root) and otherwise uses the common Git directory's `hooks/`, including in
linked worktrees. An inherited shared override blocks installation until you set a
repository-local destination.

Installation preserves foreign or locally modified hooks and reports a conflict; back
them up and move them aside before retrying. Matching legacy copies can be adopted, and
subsequent updates verify ownership with a sibling `.dotfiles-owned` hash record, so
keep that record alongside the installed hook. Only extensionless entrypoints are copied,
while `.sh` helpers and pattern files run directly from the checkout. Reinstall after
editing the copied `pre-commit`; helper edits take effect right away.

Copying the entrypoint does not install its toolchain. A hook can run even when an
optional analyzer is missing, so watch for reported skips and use the documented
pre-push checks for full coverage. If a changed entrypoint does not seem to run,
compare the installed copy and its ownership record against the tracked source, and
verify that your user can execute files in the hooks directory before altering Git's
hook configuration.

## Preview and apply

Preview the hook copies after checking the destination above:

```sh
dotfiles update --only git-hooks --dry-run
```

Confirm that the hook names and destination belong to this checkout.
If ownership or an inherited hooks path blocks installation, resolve that
conflict before [applying the change](task-reference.md#preview-and-apply).
Compare the installed entrypoints with their tracked sources afterward; a
successful copy does not guarantee that every optional analyzer is installed.

## Removal

Removal deletes only hooks whose ownership can be verified, leaving modified hooks,
unrelated filenames, and a destination pointing at the tracked source itself intact.
Preview with `dotfiles remove --only git-hooks --dry-run` before applying.

A missing `hooks/` directory does not block removal of recorded copies, whereas a
broken link or unreadable directory stops removal until you fix the directory error.

Removal stages each hook to a sibling `.dotfiles-removing` path and verifies the moved
entry before deleting it. If removal is interrupted, check that path: restore it to the
original hook name if the hook is missing, or if both exist, keep the current hook and
move the saved entry somewhere safe before retrying.
