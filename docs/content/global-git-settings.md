---
title: Global Git settings
description: Global Git values and Windows autocrlf cleanup.
---

# Global Git settings

| Task detail | Value |
|---|---|
| Selector | `git` |
| Commands | `update` |
| Availability | Configured values; Windows also cleans up undeclared autocrlf |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [git/git_config.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/git/git_config.rs) |

## What it does

Applies `conf/git-config.toml` values to **global** Git configuration. Repeated
occurrences of managed keys are handled without altering unrelated keys. Profile/overlay
persistence instead uses repository-local configuration.

On Windows, an undeclared global `core.autocrlf` is removed, even with no other
configured Git settings. An explicit active declaration overrides that cleanup. On other
platforms, empty settings mean no work.

## Configuration

Edit
[conf/git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml).

```toml
[windows]
settings = [
  { key = "core.longpaths", value = "true" },
]
```

Both fields are strings, including booleans written as `"true"` or `"false"`. These
settings affect **global** Git configuration. Avoid contradicting the shared files under
[symlinks/config/git/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git).
Included files remain unchanged. Managed values are written to the global file after its
includes so scalar settings take precedence. This does not remove inherited entries for
multivalue settings such as `credential.helper`.

On Windows, the task also removes global `core.autocrlf` when no active declaration
manages it, even if `settings` is empty. Declare that key explicitly if you want the
task to manage a value instead. This cleanup is separate from the repository-local Git
values used to remember profiles and overlays.

For Git aliases such as `git gr` and `git contributors`, see [Application
settings](application-settings.md#git).

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only git --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Global Git values remain after their declarations are removed. This task has no remove
action. Change or unset the keys deliberately with Git, taking included files and
repository-local overrides into account.
