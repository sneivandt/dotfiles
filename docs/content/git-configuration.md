---
title: Git configuration
description: Customize Git identity, preferences, and aliases, and understand push protection and automatic local branch cleanup.
---

# Git configuration

Dotfiles supplies personal Git settings and templates on Linux and Windows.
They affect Git beyond this repository. This guide assumes the shared settings
are installed; use [Getting started](getting-started.md) for installation.
Repository contributor checks are documented separately in [Git hooks](git-hooks.md).

## Settings and identity

Edit tracked sources in your checkout, not installed copies. The links can make
edits effective immediately, without another dotfiles update.

| Change | Source beneath symlinks/ |
|---|---|
| Identity, defaults, includes, and template directory | [config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config) |
| Command aliases | [config/git/aliases](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases) |
| Push protection and branch cleanup | [config/git/templates/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git/templates/) |

Review the supplied identity and defaults before using them. Keep private identity
details and employer-specific settings in a [private overlay](private-overlays.md),
not public sources.

Desired global key/value overrides use
[conf/git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml).
The [Git settings format](configuration-reference.md#git-settings) explains
their platform selection and Windows autocrlf cleanup. Avoid contradicting
the linked settings with a second declaration.

## Shortcuts

Common shortcuts include:

| Command | Purpose |
|---|---|
| `git st` | Show concise status and branch information |
| `git dis` | Review staged changes |
| `git lo` | Show a compact commit graph |
| `git gr <pattern>` | Search tracked content |

Run `git alias` to list all configured aliases.

Alias definitions belong in the linked Git files. Desired global key/value
overrides belong in [Git settings](configuration-reference.md#git-settings).

If a preference does not take effect, inspect its effective value and source
in the affected repository:

```sh
git config --show-origin --get core.autocrlf
```

Replace the key as needed. Other global files, included files, and repository
settings can override the linked configuration; a correct symlink alone does
not prove which value Git uses.

## Git templates

Git copies template entrypoints when initializing or cloning a repository.
Their shared helpers resolve from the managed template directory at runtime.
The shared Git configuration selects them through `init.templateDir`.
They are **not** installed by the repository's
[Git hooks task](git-hooks.md#installation-and-removal).

### Merged-branch cleanup

`post-merge` and `post-rewrite` after a rebase run the shared
`clean-merged-branches` helper. **It automatically deletes eligible local
branches** merged into `origin/HEAD`, while preserving:

- The current branch and the default branch.
- `main`, `master`, `dev`, and `develop`.
- Branches checked out in any worktree.
- Any branch Git refuses to delete safely with `git branch -d`.

It does not delete remote branches. Without `origin/HEAD`, cleanup does nothing.
An amend does not trigger it.
Existing repositories need a newly introduced template entrypoint, such as
`post-rewrite`, installed explicitly; do not overwrite a repository-specific
hook to do so.

### Push protection

The template pre-push hook blocks branch updates to `main`, `master`, `develop`,
and the destination remote's default branch. It prefers that remote's cached
HEAD, then tries a read-only, noninteractive `ls-remote --symref` lookup.
If discovery fails, the named protections still apply.

From the repository you want to protect, add exact branch names, not globs.
These commands change that repository's local Git configuration:

```bash
git config --add dotfiles.protectedBranch release/stable
git config --add dotfiles.protectedBranch dev
```

GitHub remotes whose URL owner matches `github.user` (or `github.username`) are
exempt. This is a local configured-name check, not a GitHub permission check.
It is separate from both CI and the repository's pre-push validation script.
