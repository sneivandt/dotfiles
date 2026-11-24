---
title: Home symlinks
description: Link home-directory configuration to tracked sources, resolve existing-file conflicts, and preserve current contents when removing managed symlinks.
---

# Home symlinks

A symbolic link makes a home path point to a file or directory in the checkout
instead of holding a separate copy. Once the link is installed, an edit to the
tracked file is visible at the home path without another `dotfiles update`.
A Git pull that changes the file has the same effect. Some applications reload
the new contents immediately; others read them the next time they start.

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

Existing files, directories, and unrelated links are left in place and reported
as conflicts. The task doesn't merge existing application settings into the
source. Back up and move conflicts aside before applying.

Removal leaves a managed directory link in place if copying it would relocate a relative
link that points outside the directory being copied.

On Windows, home links stay in the original user's process.
The task checks file-link capability after Developer Mode; unavailable file links
are skipped without preventing directory links or already-correct links from running.
It also rejects Git symlink placeholders checked out as ordinary files.

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

Managed `.config` destinations are fixed beneath home; `XDG_CONFIG_HOME` does
not relocate them. When selecting those links, use `~/.config` or leave the
variable unset. A different value fails link installation and `dotfiles check`
with an unsupported-layout diagnostic. Removal still operates on the declared
home-relative destinations so an incompatible environment does not block recovery.

Two active entries within one repository cannot own the same target or overlapping
parent/child targets. An overlay can replace an exact main target, but cannot turn a
parent/child conflict into a valid pair. Target comparison normalizes separators and `.`
components, and ignores case on Windows. Targets that alias their own source through
existing directory links are rejected.

If an existing parent link redirects the destination outside home, Dotfiles reports the
resolved destination and skips the entry. To manage that location intentionally, set
`allow_external_target = true` in the table entry. Keep `target` relative to home. The
opt-in applies to installation and removal.

The opt-in never allows a home entry to be placed inside the public or overlay
`symlinks/` tree. If a parent directory is still linked into an old layout,
move that link aside and create a real home directory before retrying. Do not
delete the directory the link points to.

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
contents (`copy <home path> from <source>`). Existing ordinary files and
directories are preserved. Missing targets stay missing unless you add
`--restore-missing`. Keep the original profile, overlay, sources, and declarations
available until removal finishes.

Copy and safety behavior:

- Links pointing directly to their configured checkout entries become independent
  copies; links reached through another home path are preserved.
- Hard links within a copied directory still share a file inside that copy.
- Nested symlinks are preserved. A directory link is left in place if copying it
  would relocate a relative link pointing outside the copied tree.
- Ownership is checked again before replacement, so a link retargeted after
  inspection is preserved. If another process replaces a target during removal,
  keep the new entry and review the recovery path printed by dotfiles.
- On Linux, copies are staged privately, preserve sparse-file holes when the
  source filesystem reports those extents, and copy source permissions, `user.*`
  extended attributes, and ACLs (`system.posix_acl_*` and `system.nfs4_acl`)
  while skipping host-managed security labels and capabilities. Removal leaves
  the managed link in place if a staged entry has a different owner or group from
  its source, or if a preserved ACL or extended attribute cannot be copied; it
  does not request elevated privileges to change ownership.
- On Windows, removal leaves the managed link in place if it cannot verify that
  the copy has the same access policy as the source, or if the source is
  encrypted (including entries inside a linked directory). With
  `--restore-missing`, the target stays absent on refusal. Do not loosen source
  permissions to force removal.

```sh
dotfiles remove --only symlinks --dry-run
```

Review the preview before repeating the command without `--dry-run`. Add
`--restore-missing` to the preview and apply only if you also want source copies at
absent targets. This detaches live sources; it does not restore a backup.
