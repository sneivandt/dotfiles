---
title: File permissions
description: Unix modes for declared home paths.
---

# File permissions

| Task | Selection |
|---|---|
| Selector | `file-permissions` |
| Commands | `update` |

## What it does

Applies Unix modes to existing configured home targets after the symlink task succeeds.
Directories recurse and retain traversal bits; ordinary files have execute bits cleared
unless explicitly configured otherwise. Missing content is not created. Changes through
links can affect the checkout.

## Configuration

Edit [conf/chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml).

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/zsh" },
]
```

Both fields are required. `mode` is a three- or four-digit octal string. `path` uses the
dot-prefixed home convention: `ssh/config` means `~/.ssh/config`. `./` components
normalize; absolute paths, parent traversal and home itself are invalid.

Missing targets are not created. A directory entry applies recursively: directories
retain traversal access and ordinary files have execute bits cleared. Give executable
files their own entries. Explicit descendant entries override recursive ancestors
regardless of ordering or parallel execution.

Permission changes through managed links can affect the source checkout. Other
destinations outside home and the public or overlay `symlinks/` trees are blocked. Set
`allow_external_target = true` on a permission entry only when you intend to change that
external file or directory tree. Previews show the resolved destination.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only file-permissions --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Permission changes remain after `dotfiles remove`. Dotfiles does not save the previous
modes. Restore permissions deliberately from your own records rather than assuming a
declaration can undo an earlier change.
