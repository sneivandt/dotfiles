---
title: Home symlinks
description: Link home-directory configuration to tracked sources, resolve existing-file conflicts, and preserve current contents when removing managed symlinks.
---

# Home symlinks

A symbolic link makes a home-directory path refer to a source in the checkout
instead of holding a separate copy. Editing that tracked source can change
the application's configuration immediately. This task creates those links;
it does not merge an existing application's settings into the source.

| Task detail | Value |
|---|---|
| Selector | `symlinks` |
| Commands | `update`, `remove` |
| Availability | Configured links |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Windows Developer Mode](windows-developer-mode.md) during update |
| Implementation | [files/symlinks.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/files/symlinks.rs) |

## What it does

Links sources from the declaring repository's `symlinks/` into home. Overlay entries
retain their origin.

Existing files, directories, and unrelated links are left in place and reported as
conflicts. Back up and move them aside before applying.

Removal leaves a managed directory link in place if copying it would relocate a relative
link that points outside the directory being copied.

Linked sources stay live. On Windows, the task independently assesses elevation and
rejects Git symlink placeholders checked out as ordinary files. See
[Symlinks](#configuration) for path and glob rules.

## Configuration

Edit
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml).
Each declaration selects a source file and a destination. For example, both profiles
select this entry from
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml):

```toml title="conf/symlinks.toml"
[base]
symlinks = ["config/git/config"]
```

| Part | In this example |
|---|---|
| Declaration | `"config/git/config"` in `conf/symlinks.toml` |
| Source to review or edit | [symlinks/config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config) |
| Destination in your home | `~/.config/git/config` |
| What applying does | Creates a link from the home destination to the source in this checkout |

To change a Git preference, edit the source. To stop selecting this link, change the
declaration; that does not remove an already-installed link.

Linking an entire directory also exposes future files added beneath it.
Linking individual files gives narrower ownership but needs declarations for
new files. Choose that boundary deliberately when sharing a directory with
files an application writes itself. Linux's
[symbolic link reference](https://man.archlinux.org/man/symlink.7.en) explains
link resolution; [Windows and WSL](../guides/windows-and-wsl.md#developer-mode-and-symlinks)
explains Windows symlinks, junctions, and checkout requirements.

A string names a source and derives a dot-prefixed home target:

```toml
[base]
symlinks = ["config/git/config", "ssh/config"]
```

The table form requires both `source` and `target`. It is useful for Windows locations
or for sharing one source across several destinations:

```toml title="conf/symlinks.toml (excerpt)"
[linux]
symlinks = [
  "vim",
  { source = "vim", target = ".config/nvim" },
]
```

Paths must stay below their respective roots: sources inside the declaring repository's
`symlinks/`, targets inside home. Absolute paths, `..`, and entries naming the root
itself are invalid. `./bashrc` normalizes to the same target as `bashrc`.

Two active entries within one repository cannot own the same target or overlapping
parent/child targets. An overlay can replace an exact main target, but cannot turn a
parent/child conflict into a valid pair. Target comparison normalizes separators and `.`
components, and ignores case on Windows. Targets that alias their own source through
existing directory links are rejected.

If an existing parent link redirects the destination outside home, Dotfiles reports the
resolved destination and skips the entry. To manage that location intentionally, set
`allow_external_target = true` in the table entry. Keep `target` relative to home. The
opt-in applies to installation and removal.

See [Home symlinks](#what-it-does) and [Remove tasks](task-reference.md#remove-tasks)
before taking over an existing application's configuration.

### Glob patterns

Use a complete `*` segment to link entries individually. For example, suppose
you maintain theme files under `symlinks/config/example-app/themes/`:

```toml title="conf/symlinks.toml (illustrative example)"
[base]
symlinks = ["config/example-app/themes/*"]
```

An entry named `dark.toml` becomes `~/.config/example-app/themes/dark.toml`.
The application name is illustrative; the pattern requires real source entries.

This is intentionally smaller than shell glob syntax:

- `*` matches one path segment, including dot-prefixed names.
- Partial patterns such as `*.yml` and recursive `**` are invalid.
- Expansion does not descend through symlinked directories.
- No matches, or a matched filename that is not valid UTF-8, is an error.
- Matches are sorted and checked for target conflicts after category filtering.

An explicit target may contain captures. If it does, source and target must contain the
same number of `*` segments, substituted in order:

```toml title="conf/symlinks.toml (alternative target)"
[base]
symlinks = [
  { source = "config/example-app/themes/*", target = ".local/share/example-app/themes/*" },
]
```

This alternative sends the same source entries to
`~/.local/share/example-app/themes/`. Choose the location the application
actually reads; an explicit target does not get another leading dot.

## Preview and apply

Preview the selected source-to-home links:

```sh
dotfiles update --only symlinks --dry-run
```

Check each source and home destination, especially explicit targets and paths
redirected by existing parent links. Back up and move aside conflicting
ordinary content before [applying the reviewed
change](task-reference.md#preview-and-apply). Use [Inspect a home
path](../guides/commands-and-usage.md#inspect-a-home-path) to diagnose a surprising mapping.
Application behavior can change as soon as the link is created.

## Removal

Removal replaces matching managed links with local copies of their current source
contents. Existing ordinary files and directories are preserved. Missing targets stay
missing unless you add `--restore-missing`. Keep the original profile, overlay, sources,
and declarations available until removal finishes.

Nested symlinks are preserved. A directory link is left in place if copying it would
relocate a relative link that points outside the copied tree. Ownership is checked again
before replacement, so a link retargeted after inspection is preserved. File copies are
staged privately on Unix and receive source permissions before publication.

```sh
dotfiles remove --only symlinks --dry-run
```

Review the preview before repeating the command without `--dry-run`. Add
`--restore-missing` to the preview and apply only if you also want source copies at
absent targets. This detaches live sources; it does not restore a backup.
