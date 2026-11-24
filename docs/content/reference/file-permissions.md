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

Applies Unix modes to existing configured home targets once the symlink task succeeds.
Directory entries recurse, granting traversal bits wherever the mode grants read access,
while clearing execute bits on ordinary files unless a separate entry configures them.
Missing files are never created, and changes applied through symlinks can affect files
in the checkout.

## Configuration

Edit [conf/chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml).

The Linux default sets `~/.ssh/config` to `600`. If an earlier update changed
permissions under `~/.config/zsh`, restore those files' intended modes manually.

To restrict an existing SSH directory and its configuration, extend the category's
`permissions` array:

```toml title="conf/chmod.toml (example)"
[linux]
permissions = [
  { mode = "700", path = "ssh" },
  { mode = "600", path = "ssh/config" },
]
```

Both fields are required. `mode` is a three- or four-digit octal string, and `path` uses
the dot-prefixed home convention (`ssh/config` means `~/.ssh/config`). `./` components
normalize, while absolute paths, parent traversal, and home itself are rejected.

Missing targets are skipped rather than created. Permission convergence follows these rules:

- **Recursive directories:** A directory entry applies recursively. Directories
  gain execute bits for each permission class that has read access, and ordinary
  files have execute bits cleared. Recursion skips sockets, named pipes, and
  other special files; give executable files their own entries.
- **Explicit descendants:** Separate descendant entries are excluded from
  recursive ancestors regardless of ordering or parallel execution, even when
  a parent is reached through a symlink. Parent modes still need owner traversal
  so descendant entries can be checked.
- **Conflicting modes:** Declarations that request different modes for the same
  readable resolved destination fail before any permissions change. Destinations
  hidden inside inaccessible directories are checked again once access is restored,
  before any modes change inside that tree. Keep one mode per destination.
- **Multiple hard links:** Files with multiple hard links are refused,
  including inside recursive trees, because changing their mode would also
  affect every other name for the same file. Give the managed file an
  independent copy before retrying; the external-target opt-in does not bypass
  this check.

When a directory lacks owner read or traversal access, apply temporarily grants
both to inspect its contents. Directory modes are applied after their descendants
so restrictive modes like `000` do not block recursive updates. If inspection fails,
hits a hard-linked file, or uncovers conflicting declarations, Dotfiles restores the
original directory modes without changing file permissions. An apply error also
restores temporary access, though it does not roll back permission changes completed
before that error.

Parent access is repaired before separately declared descendants are validated,
including descendants reached through aliases. Preview never modifies modes:
an inaccessible descendant is reported as unknown rather than assumed safe,
so an inaccessible subtree can still surface a blocker during apply.

For example, `600` lets only the owner read and write a file, while `700`
also lets the owner traverse a directory while excluding other users.
Dotfiles accepts octal strings rather than symbolic expressions like `u+rw`. The
[chmod manual](https://man.archlinux.org/man/chmod.1.en) explains mode bits,
including the special bits represented by a fourth octal digit.

Permission changes through managed links can update files in the source checkout.
Other destinations outside home and the public or overlay `symlinks/` trees are blocked.
Set `allow_external_target = true` on a permission entry only when you intentionally
want to change that external file or directory tree; previews show the resolved
destination.

## Preview and apply

Preview the modes and resolved destinations:

```sh
dotfiles update --only file-permissions --dry-run
```

Check recursive directory entries and any explicit executable descendants.
If a destination resolves outside the allowed roots, inspect the link chain
before considering the external-target opt-in. After [applying the reviewed
change](task-reference.md#preview-and-apply), verify the affected files' modes;
any missing target still needs its content created separately.

## Removal

Permission changes remain after `dotfiles remove` because Dotfiles does not record
previous modes. Restore permissions deliberately from your own records rather than
expecting a declaration removal to undo an earlier change.
