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

On Linux outside CI, merges declared `system/` fragments into files under `/etc`.
TOML, INI, and PAM follow different ownership rules, so review the
[configuration format](#configuration) before editing fragments.

Malformed or non-regular destinations fail, and missing PAM service files are never
created. Merging preserves an existing file's owner, group, and permissions, while
newly created files default to `root:root` and mode `0644`. Assessment and dry-run
never elevate to read protected files: if permissions block inspection, preview notes
that privileged assessment is needed and apply assesses and merges inside a scoped
root process. Both original and merged contents remain in a private staging directory
beside the destination rather than in user-owned temp files or command output.

## Configuration

Edit
[conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml).
Use structured fragments rather than home symlinks for administrator-owned files.
For an illustrative application that reads `/etc/example-app/settings.toml`,
add this declaration:

```toml title="conf/system-files.toml (example)"
[linux]
files = [{ source = "example-app/settings.toml", merge = "toml" }]
```

Create the matching fragment in the declaring repository:

```toml title="system/example-app/settings.toml (example)"
[logging]
level = "warn"
```

Applying this example manages `logging.level` while keeping unrelated keys intact.
The application and key are illustrative rather than files Dotfiles requires every
machine to have; substitute the real application's format and keys.

`source` and `merge` are required. Sources live under the declaring repository's
`system/`, and the optional `target` defaults to `/etc/<source>` (and must be an
absolute path under `/etc`). Neither path may contain `..`. The destination must
resolve within `/etc`, with parent directories owned by root and writable only by root.
Duplicate active destinations in one repository are rejected, and an active destination
cannot be a parent directory of another active destination even across repositories.

| Strategy | What the fragment manages |
|---|---|
| `toml` | Keys, recursively merging tables |
| `ini` | Assigned keys and bare flags within sections, including Pacman options |
| `pam` | Updates existing module rules in place or appends new rules to their stack |
| `create-only` | Installs exact text at a missing path; accepts identical files and refuses to replace different ones |

Unrelated settings stay intact. TOML merges preserve comments and formatting around
existing keys, though managed values and newly added keys may reformat. Both TOML and
INI fragments are validated even when the destination does not exist yet.
INI fragments cannot assign conflicting values to the same key in one section
(identical repeated assignments are permitted). Keys and section names must be
non-empty, section headers need matching non-nested brackets, and bare flags such as
`Color` are valid inside a section. If an installed INI file has a malformed section
header, repair that line before retrying because Dotfiles cannot tell which section
owns the lines after it.

The Arch `paccache` timer uses a dedicated `create-only` drop-in and environment file.
Because systemd accumulates `EnvironmentFile=` directives, treating them as plain INI
keys could drop administrator settings; the drop-in instead adds a file that sets only
`PACCACHE_ARGS` without resetting the list.

Ordering and overrides follow these rules:

- The environment file is installed before the drop-in, so an interrupted run leaves
  the service using its previous configuration.
- An overlay can replace the environment file without moving it after the drop-in,
  since system-file overrides keep their main declaration's position.
- Once the drop-in is active, a missing policy file skips cleanup rather than falling
  back to other retention defaults.

If the task reports a conflict, compare the installed files under
`/etc/systemd/system/paccache.service.d/` with their `system/` sources. An older
managed `retention.conf` included an empty `EnvironmentFile=` reset: back up that file
outside the active `.conf` directory before replacing it with the tracked
`retention.conf`, and move any local overrides you still need into a separate drop-in.
Retry the task, then
[reload the service manager and preview the retention policy](../guides/package-management.md).

PAM merges require an existing service file containing a stack of the same type as each
fragment rule. Unrecognized facilities or control names in fragments are rejected, and
existing rules keep their position. When several rules in a stack share a module path,
Dotfiles leaves all of them untouched if one matches the fragment, and otherwise stops
so you can decide which rule should change. It also refuses to append rules to a stack
with a numeric jump (such as `success=1`) until you review the installed stack.

If the same module appears under different paths in a stack, specify the exact path in
your fragment before retrying. Dotfiles ignores blank lines and full-line comments in
fragments; a fragment may repeat an identical rule but can assign only one rule per
facility and module path.
The [PAM merge implementation](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/system_file/merge_pam.rs)
and its [tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/tests/system_file_pam.rs)
cover detailed rule matching and placement.

**These are privileged changes, not reversible patches.** An overlay replaces the
entire entry for a matching destination, including its source and strategy, and
removing a declaration never restores previous contents.

Pick the merge strategy from the file's actual format rather than its extension alone.
In particular, PAM (Pluggable Authentication Modules) rules govern login and session
authentication, and their order matters. Read the
[PAM configuration manual](https://man.archlinux.org/man/pam.d.5.en) before editing
PAM fragments, and keep a recovery path that does not depend on the login flow you are
changing. If a rule change ever breaks authentication, use an existing administrator
session or recovery environment to restore a known-good copy. For WSL-specific system
files and restart rules, see [Windows and WSL](../guides/windows-and-wsl.md).

## Preview and apply

Preview which administrator-owned destinations need merging:

```sh
dotfiles update --only system-files --dry-run
```

Review the destination paths along with each fragment and current file; preview reports
which files need work rather than a full merged diff. Fix any malformed content or
unsafe destination path before retrying, back up affected files, and keep a separate
recovery path for authentication changes before [applying the privileged
merge](task-reference.md#preview-and-apply). Inspect the merged files afterward, since
removal will not restore your backup.

## Removal

Removing a declaration leaves the merged file untouched because this task does not save
an undo patch or restore earlier contents. Restore from a reviewed backup or edit the
file directly while keeping unrelated administrator settings intact.
