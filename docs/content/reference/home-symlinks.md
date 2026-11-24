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

Links sources from each declaring repository's `symlinks/` directory into your home
directory, preserving overlay origins.

Existing files, directories, and unrelated links are left untouched and reported
as conflicts rather than merged into the source. Back up and move conflicting files
aside before applying.

On removal, a managed directory link stays in place if copying it would relocate a
relative symlink that points outside the copied directory.

On Windows, home links run in the original user's process. After Developer Mode runs,
the task checks file-link capability: if file links are unavailable, they are skipped
without blocking directory links or already-correct links. It also rejects Git symlink
placeholders checked out as plain files.

## Configuration

Edit
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml).
Each declaration pairs a source file with a home destination. For example, both profiles
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

Edit the source file to change a Git preference. Removing the declaration stops future
updates from selecting that link, without removing an already-installed link.

Linking an entire directory automatically exposes future files added inside it, while
linking individual files keeps ownership narrow at the cost of declaring each new file.
Choose that boundary deliberately when an application writes its own files into the
same directory. Linux's
[symbolic link reference](https://man.archlinux.org/man/symlink.7.en) covers
link resolution, and [Windows and WSL](../guides/windows-and-wsl.md#developer-mode-and-symlinks)
explains Windows symlinks, junctions, and checkout requirements.

A plain string names a source and derives a dot-prefixed home target:

```toml
[base]
symlinks = ["config/git/config", "ssh/config"]
```

The table form takes both `source` and `target`, which is useful for Windows paths
or sharing one source across multiple destinations:

```toml title="conf/symlinks.toml (excerpt)"
[linux]
symlinks = [
  "vim",
  { source = "vim", target = ".config/nvim" },
]
```

Paths must stay inside their respective roots: sources under the declaring repository's
`symlinks/` and targets under home. Absolute paths, `..`, and entries naming the root
itself are rejected; `./bashrc` normalizes to the same target as `bashrc`.

Managed `.config` destinations are fixed under home and are not relocated by
`XDG_CONFIG_HOME`. When selecting those links, point `XDG_CONFIG_HOME` to `~/.config`
or leave it unset; any other value fails link installation and `dotfiles check` with an
unsupported-layout diagnostic. Removal still operates on the declared home-relative
destinations so an incompatible environment does not block recovery.

Two active entries in one repository cannot claim the same target or overlapping
parent/child targets. An overlay can override an exact main target, though it cannot
turn a parent/child conflict into a valid pair. Target comparison normalizes separators
and `.` components, ignores case on Windows, and rejects targets that alias their own
source through existing directory links.

If an existing parent link redirects a destination outside home, Dotfiles reports the
resolved path and skips the entry. Set `allow_external_target = true` on the table entry
only when you intend to manage that location, keeping `target` relative to home; the
opt-in applies to both installation and removal.

Even with that opt-in, Dotfiles never places a home entry inside the public or overlay
`symlinks/` tree. If a parent directory is still linked into an old layout,
move that link aside and create a real home directory before retrying rather than
deleting the directory the link points to.

### Glob patterns

Use a standalone `*` path segment to link entries individually. For example, if
you maintain theme files under `symlinks/config/example-app/themes/`:

```toml title="conf/symlinks.toml (illustrative example)"
[base]
symlinks = ["config/example-app/themes/*"]
```

An entry named `dark.toml` maps to `~/.config/example-app/themes/dark.toml`.
The application name is illustrative; the pattern requires real source entries.

This syntax is intentionally narrower than shell globs:

- `*` matches one full path segment, including dot-prefixed names.
- Partial patterns like `*.yml` and recursive `**` are rejected.
- Expansion does not descend through symlinked directories.
- Zero matches, or a matched filename that is not valid UTF-8, is an error.
- Matches are sorted and checked for target conflicts after category filtering.

An explicit target can include captures as long as `source` and `target` have the
same number of `*` segments, which are substituted in order:

```toml title="conf/symlinks.toml (alternative target)"
[base]
symlinks = [
  { source = "config/example-app/themes/*", target = ".local/share/example-app/themes/*" },
]
```

This alternative sends the same source entries to
`~/.local/share/example-app/themes/`. Pick the path the application actually reads;
explicit targets do not receive an extra leading dot.

## Preview and apply

Preview the selected source-to-home links:

```sh
dotfiles update --only symlinks --dry-run
```

Review each source and home destination, especially explicit targets and paths
redirected by existing parent links. Back up and move conflicting ordinary content
aside before [applying the reviewed change](task-reference.md#preview-and-apply),
and use [Inspect a home path](../guides/commands-and-usage.md#inspect-a-home-path)
whenever a mapping looks surprising. Application behavior can change as soon as
the link is created.

## Removal

Removal replaces matching managed links with local copies of their current source
contents (`copy <home path> from <source>`). Existing ordinary files and
directories stay untouched, and missing targets stay missing unless you pass
`--restore-missing`. Keep the original profile, overlay, sources, and declarations
available until removal completes.

Copy and safety behavior:

- Links pointing directly to their configured checkout entries become independent
  copies; links reached through another home path are left alone.
- Hard links within a copied directory continue sharing a file inside that copy.
- Nested symlinks are preserved, and a directory link stays in place if copying it
  would relocate a relative link pointing outside the copied tree.
- Ownership is rechecked right before replacement, so a link retargeted after
  inspection is preserved. If another process replaces a target during removal,
  keep the new entry and review the recovery path Dotfiles prints.
- On Linux, copies are staged privately, preserve sparse-file holes when the
  source filesystem reports those extents, and copy source permissions, `user.*`
  extended attributes, and ACLs (`system.posix_acl_*` and `system.nfs4_acl`)
  while skipping host-managed security labels and capabilities. Removal leaves
  the managed link in place if a staged entry ends up with a different owner or
  group from its source, or if a preserved ACL or extended attribute cannot be
  copied; it never requests elevated privileges to change ownership.
- On Windows, removal leaves the managed link in place if it cannot verify that
  the copy matches the source's access policy, or if the source is encrypted
  (including entries inside a linked directory). With `--restore-missing`, the
  target stays absent on refusal. Do not loosen source permissions to force removal.

```sh
dotfiles remove --only symlinks --dry-run
```

Review the preview before re-running without `--dry-run`. Add `--restore-missing`
to both preview and apply only if you also want source copies created at absent
targets. Remember that removal detaches live sources rather than restoring a backup.
