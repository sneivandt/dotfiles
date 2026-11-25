---
title: Global Git settings
description: Apply declared global Git configuration and resolve private overlay overrides.
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
| Availability | Configured values |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [git/git_config.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/git/git_config.rs) |

## What it does

Applies values from `conf/git-config.toml` to **global** Git configuration (set
`GIT_CONFIG_GLOBAL` to target a different file). By default it writes to the user's
`.gitconfig` rather than the shared `~/.config/git/config` linked from the checkout,
updating repeated occurrences of managed keys without touching unrelated entries.
Profile and overlay selections stay in repository-local configuration.

Each update publishes the selected settings together, so if staging one setting fails,
the global file keeps its original contents until you fix the error and retry.

On Unix, updates stage content privately and preserve the existing file's permissions,
creating new config files as owner-only. Dotfiles honors Git's `.lock` file: if another
process holds it, wait for it to finish and retry, and investigate any stale lock before
removing it.

When no active Git settings are declared, the task reports not applicable and leaves
the global config untouched.

## Configuration

Edit
[conf/git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml).

```toml
[windows]
core.longpaths = "true"
```

Declare keys directly under category sections as dotted keys or nested tables;
leaf values must be strings, including booleans written as `"true"` or `"false"`.
Because these entries write to **global** Git configuration, avoid contradicting
the shared files under
[symlinks/config/git/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git).
Included files stay untouched. Managed values are written to the global file after its
includes so scalar settings win, though this does not clear inherited entries for
multivalue settings such as `credential.helper`.

For Git aliases such as `git gr` and `git contributors`, see [Application
settings](../guides/application-settings.md#git).

To see why a setting is not taking effect, run this inside the affected repository:

```sh
git config --show-origin --show-scope --get-all core.longpaths
```

Replace the key with the setting you are diagnosing. The output shows included files and
repository overrides alongside the managed global value; no output with exit status 1
means the key is unset. Git's
[configuration reference](https://git-scm.com/docs/git-config) explains precedence,
multivalue settings, and supported keys.

## Preview and apply

Preview the global keys that need changing:

```sh
dotfiles update --only git --dry-run
```

Review the proposed values and resolve configuration read errors rather than treating
them as absent keys. After [applying the reviewed change](task-reference.md#preview-and-apply),
run the origin-and-scope command above inside an affected repository to distinguish the
stored global value from repository or included overrides.

## Removal

Global Git values stay in place after their declarations are removed because this task
has no remove action. Change or unset keys deliberately with Git, keeping included files
and repository-local overrides in mind.
