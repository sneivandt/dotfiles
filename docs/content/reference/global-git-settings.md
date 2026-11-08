---
title: Global Git settings
description: Apply declared global Git configuration, resolve private overlay overrides, and clean up undeclared global core.autocrlf on Windows.
---

# Global Git settings

Git settings have system, user, and repository scopes. This task manages
selected user-wide values, so a change can affect repositories other than
dotfiles. It is separate from linking the shared Git configuration and from
installing this repository's contributor hooks.

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

On Unix, updates stage content privately and preserve the existing file's access
mode; new configuration files are owner-only. Dotfiles respects Git's `.lock` file;
if another writer holds it, let that operation finish and retry. Investigate
a stale lock before removing it.

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
settings](../guides/application-settings.md#git).

To investigate a value that does not take effect, run this inside the affected
repository:

```sh
git config --show-origin --show-scope --get-all core.autocrlf
```

Replace the key with the setting you are diagnosing. The output identifies
included files and repository overrides as well as the managed global value;
no output with exit status 1 means the key is absent. Git's
[configuration reference](https://git-scm.com/docs/git-config) defines precedence,
multivalue settings, and each supported key.

## Preview and apply

Preview the global keys that need changing:

```sh
dotfiles update --only git --dry-run
```

Check the proposed values, including any Windows `core.autocrlf` removal.
Resolve configuration read errors rather than treating them as absent keys.
After [applying the reviewed change](task-reference.md#preview-and-apply), use
the origin-and-scope query above in an affected repository to distinguish the
stored global value from repository or included overrides.

## Removal

Global Git values remain after their declarations are removed. This task has no remove
action. Change or unset the keys deliberately with Git, taking included files and
repository-local overrides into account.
