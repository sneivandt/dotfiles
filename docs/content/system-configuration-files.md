---
title: System configuration files
description: Structured merges into files below /etc.
---

# System configuration files

| Task | Selection |
|---|---|
| Selector | `system-files` |
| Commands | `update` |

## What it does

On Linux outside CI, merges declared `system/` fragments into files below `/etc`. TOML,
INI and PAM have different ownership rules; review the [configuration
format](#configuration) before editing.

Malformed or non-regular destinations fail. Missing PAM service files are not created. A
merge preserves the destination's owner, group, and permissions. New files use
`root:root` and mode `0644`. Dry-run leaves destinations unchanged.

## Configuration

Edit
[conf/system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml).
Use structured fragments for administrator-owned files, not home symlinks:

```toml
[linux]
files = [{ source = "codex/requirements.toml", merge = "toml" }]

[arch]
files = [{ source = "pacman.conf", merge = "ini" }]

[arch-desktop]
files = [{ source = "pam.d/login", merge = "pam" }]

[wsl]
files = [{ source = "wsl.conf", merge = "ini" }]
```

`source` and `merge` are required. Sources belong under the declaring repository's
`system/`. The optional `target` defaults to `/etc/<source>` and must be an absolute
path below `/etc`. Both paths must omit `..`. The destination must resolve within
`/etc`. Its parent directories there must be owned by root, with writes restricted to
root. Duplicate active destinations within a repository are errors.

| Strategy | What the fragment manages |
|---|---|
| `toml` | Keys, recursively merging tables |
| `ini` | Assigned keys and bare flags within sections, including Pacman options |
| `pam` | Module rules, placed at the end of their existing authentication, account, password, or session stack |

Unrelated settings are preserved. A TOML update can remove comments and change
formatting. TOML and INI fragments are validated even when the destination is absent.
INI cannot assign different values to the same key within one section. Identical
repeated assignments are allowed.

PAM needs an existing service file and a stack of the same type as each fragment rule. A
rule with the same module path in that stack is replaced. If the same module name
appears under different paths in the stack, review the installed rule and make the
fragment use the intended path before retrying. Blank lines and full-line comments in
PAM fragments are not inserted. The [PAM merge implementation and
tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/system_file.rs)
cover detailed rule matching and placement.

**These are privileged changes, not a reversible patch.** An overlay replaces the whole
entry for a matching destination, including source and strategy. Removing a declaration
does not restore previous content.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only system-files --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Removing a declaration leaves the merged file in place. This task does not save an undo
patch or restore its earlier contents. Use a reviewed backup or make a deliberate edit
that preserves unrelated administrator settings.
