---
title: System configuration files
description: Merge TOML, INI, and PAM fragments into administrator-owned Linux files below /etc while preserving unrelated settings and file ownership.
---

# System configuration files

Some settings belong to administrator-owned files rather than a user's home.
This task merges selected fragments into those files while preserving unrelated
settings. It is not a whole-file symlink or a text patch: the selected format
determines which keys or rules the fragment owns.

| Task detail | Value |
|---|---|
| Selector | `system-files` |
| Commands | `update` |
| Availability | Linux with configured fragments, outside CI |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [system/system_files.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/system_files.rs) |

## What it does

On Linux outside CI, merges declared `system/` fragments into files below `/etc`. TOML,
INI and PAM have different ownership rules; review the [configuration
format](#configuration) before editing.

Malformed or non-regular destinations fail. Missing PAM service files are not created. A
merge preserves the destination's owner, group, and permissions. New files use
`root:root` and mode `0644`. Assessment and dry-run never elevate to read protected
destinations. If permissions prevent inspection, preview reports that privileged
assessment is required; apply assesses and merges inside a scoped root process.
Original and merged contents stay in its private staging directory beside the
destination, not in user-owned temporary files or command output.

## Configuration

Edit
[conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml).
Use structured fragments for administrator-owned files, not home symlinks.
For an illustrative application that reads `/etc/example-app/settings.toml`,
add this declaration:

```toml title="conf/system-files.toml (example)"
[linux]
files = [{ source = "example-app/settings.toml", merge = "toml" }]
```

Create the corresponding fragment in the declaring repository:

```toml title="system/example-app/settings.toml (example)"
[logging]
level = "warn"
```

Applying this example manages `logging.level`, preserving unrelated destination
keys. The application and setting are illustrative, not files that dotfiles
requires every machine to have. Use the real application's format and keys.

`source` and `merge` are required. Sources belong under the declaring repository's
`system/`. The optional `target` defaults to `/etc/<source>` and must be an absolute
path below `/etc`. Both paths must omit `..`. The destination must resolve within
`/etc`. Its parent directories there must be owned by root, with writes restricted to
root. Duplicate active destinations within a repository are errors.

| Strategy | What the fragment manages |
|---|---|
| `toml` | Keys, recursively merging tables |
| `ini` | Assigned keys and bare flags within sections, including Pacman options |
| `pam` | Update existing module rules in place or append new rules to their stack |

Unrelated settings are preserved. TOML updates preserve comments and formatting
around existing keys; managed values and new keys can change formatting.
TOML and INI fragments are validated even when the destination is absent.
INI cannot assign different values to the same key within one section. Identical
repeated assignments are allowed. Fragment keys and section names must be nonempty,
and section headers must have matching, non-nested brackets. Bare flags such as
`Color` remain valid inside a section.

PAM needs an existing service file and a stack of the same type as each fragment rule.
An existing rule keeps its position. If several rules in a stack use the same
module path, Dotfiles preserves all of them when one matches the fragment.
Otherwise, it stops so you can review which installed rule should change.
Dotfiles also refuses to add rules to a stack with a numeric jump declared in the
service file, such as `success=1`. Review the installed stack before retrying.

If the same module name appears under different paths in the stack, make the
fragment use the intended path before retrying. Dotfiles skips blank lines and
full-line comments in fragments. A fragment can repeat an identical rule but can
assign only one rule per facility and module path.
The [PAM merge implementation and
tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/system_file.rs)
cover detailed rule matching and placement.

**These are privileged changes, not a reversible patch.** An overlay replaces the whole
entry for a matching destination, including source and strategy. Removing a declaration
does not restore previous content.

Choose the strategy from the destination's actual format, not its filename
alone. In particular, PAM (Pluggable Authentication Modules) rules control
authentication and session behavior; their order is significant. Read the
[PAM configuration manual](https://man.archlinux.org/man/pam.d.5.en) before
changing those fragments, and keep a recovery path that does not depend on
the login mechanism you are editing. If a previous update moved a rule and broke
authentication, use a working administrator session or a recovery environment to
restore a known-good copy of the service file. For WSL's specific file and
restart requirements, use [Windows and WSL](../guides/windows-and-wsl.md).

## Preview and apply

Preview which administrator-owned destinations need a merge:

```sh
dotfiles update --only system-files --dry-run
```

Check the destination paths, then review each fragment and the current file
together; the preview identifies work, not a complete merged-file diff.
Resolve malformed content or unsafe destination paths before retrying.
Back up affected files and keep a separate recovery path for authentication
changes before [applying the privileged
merge](task-reference.md#preview-and-apply). Inspect the merged settings
afterward; removal will not restore the backup for you.

## Removal

Removing a declaration leaves the merged file in place. This task does not save an undo
patch or restore its earlier contents. Use a reviewed backup or make a deliberate edit
that preserves unrelated administrator settings.
