---
title: File permissions
description: Declare Unix permission modes for home-directory paths, preview chmod changes, and understand symlink and removal limits.
---

# File permissions

Unix permission modes control owner, group, and other users' read, write, and
execute access. For directories, execute grants traversal rather than running
the directory as a program. This task applies declared modes to home content;
it does not manage file ownership or Windows access-control lists.

| Task detail | Value |
|---|---|
| Selector | `file-permissions` |
| Commands | `update` |
| Availability | Platform supports Unix modes |
| [Requires success](task-reference.md#scheduling-model) | [Home symlinks](home-symlinks.md) |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [files/chmod.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/chmod.rs) |

## What it does

Applies Unix modes to existing configured home targets after the symlink task succeeds.
Directories recurse and gain traversal bits wherever the mode grants read access;
ordinary files have execute bits cleared unless explicitly configured otherwise.
Missing content is not created. Changes through links can affect the checkout.

## Configuration

Edit [conf/chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml).

The Linux default sets `~/.ssh/config` to `600`. If an earlier update changed
permissions under `~/.config/zsh`, restore the affected files' intended modes.

For example, restrict an existing SSH directory and its configuration. Extend
the existing category's array:

```toml title="conf/chmod.toml (example)"
[linux]
permissions = [
  { mode = "700", path = "ssh" },
  { mode = "600", path = "ssh/config" },
]
```

Both fields are required. `mode` is a three- or four-digit octal string. `path` uses the
dot-prefixed home convention: `ssh/config` means `~/.ssh/config`. `./` components
normalize; absolute paths, parent traversal and home itself are invalid.

Missing targets are not created. A directory entry applies recursively: directories
gain execute bits for each permission class with read access, and ordinary files have
execute bits cleared. Give executable files their own entries. Explicit descendant
entries are excluded from recursive ancestors regardless of ordering or parallel
execution, including when a parent is reached through a symlink. Parent modes must
still allow owner traversal for separate descendant entries to be checked.
Declarations requesting different modes for the same readable resolved destination fail
before the task changes any permissions. Destinations hidden by inaccessible directories
are checked again after access is restored, before applying modes within that tree.
Keep one mode for each destination.
Files with multiple hard links are refused, including within recursive trees:
changing their mode would also change every other name for the same file.
Give the managed file an independent copy before retrying. The external-target
opt-in does not bypass this check.

When a directory lacks owner read or traversal access, apply temporarily grants
owner read and traversal to inspect its contents. Directory modes are applied
after their descendants, so restrictive modes such as `000` do not interrupt
recursive changes. If inspection fails, finds a hard-linked file or reveals
conflicting declarations, it restores the original directory modes without changing
file permissions. An apply error also restores temporary access, but does not undo
permission changes completed before that error.

Parent access is repaired before separately declared descendants are validated,
including descendants reached through aliases. Preview never changes modes;
an inaccessible descendant is reported as unknown rather than assumed safe.
An inaccessible subtree can still reveal a blocker during apply.

For example, `600` permits only the owner to read and write a file; `700`
also permits the owner to traverse a directory but excludes other users.
Dotfiles accepts octal strings, not symbolic expressions such as `u+rw`. The
[chmod manual](https://man.archlinux.org/man/chmod.1.en) explains mode bits,
including the special bits represented by a fourth octal digit.

Permission changes through managed links can affect the source checkout. Other
destinations outside home and the public or overlay `symlinks/` trees are blocked. Set
`allow_external_target = true` on a permission entry only when you intend to change that
external file or directory tree. Previews show the resolved destination.

## Preview and apply

Preview the modes and resolved destinations:

```sh
dotfiles update --only file-permissions --dry-run
```

Check recursive directory entries and any explicit executable descendants.
If a destination resolves outside the allowed roots, inspect the link chain
before considering the external-target opt-in. After [applying the reviewed
change](task-reference.md#preview-and-apply), inspect the affected files' modes;
a missing target still needs its content supplied separately.

## Removal

Permission changes remain after `dotfiles remove`. Dotfiles does not save the previous
modes. Restore permissions deliberately from your own records rather than assuming a
declaration can undo an earlier change.
