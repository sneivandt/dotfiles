---
title: Private overlays
description: Keep private or machine-specific configuration outside the public repository, merge overlay declarations, replace installed links, and review script safety.
---

# Private overlays

Use an overlay when a setting belongs on your machine but not in the shared
checkout. It's a separate directory or private repository. Dotfiles reads
both: the overlay can add a setting or replace a matching declaration without
changing the public files. It doesn't patch the contents of an application
file line by line.

## Create an overlay

Include only the configuration files you need:

```text
dotfiles-private/
  conf/
    git-config.toml
```

Overlay declarations are optional. They use the same
[configuration formats](configuration-basics.md) and
[category rules](profiles-and-categories.md) as the main repository. For
example, an overlay's `conf/git-config.toml` might set a preferred editor:

```toml title="dotfiles-private/conf/git-config.toml"
[base]
settings = [{ key = "core.editor", value = "nvim" }]
```

This adds `core.editor`, or replaces any main declaration for that same key,
while keeping unrelated settings untouched. Put overlay symlink sources under
the overlay's `symlinks/` directory and system fragments under its `system/`
directory; relative paths always resolve against the repository that declared
them.

### Replace an application file

To use a private Git identity without modifying the public checkout, replace
the linked Git config through an overlay. From the main checkout, these POSIX
shell commands create a new sibling overlay (pick another path if it already
exists):

```sh
mkdir ../dotfiles-private &&
mkdir -p ../dotfiles-private/conf ../dotfiles-private/symlinks/config/git &&
cp symlinks/config/git/config ../dotfiles-private/symlinks/config/git/config
```

Update `[user]` name and email in the copied file and review `[github]` and the
other defaults. Then create `../dotfiles-private/conf/symlinks.toml`:

```toml title="dotfiles-private/conf/symlinks.toml"
[base]
symlinks = ["config/git/config"]
```

Because both declarations target `~/.config/git/config`, the overlay entry
replaces the main one whole rather than merging the two files. On Windows,
create the same folder structure and copy the file using PowerShell or File
Explorer.

With a saved profile and installed launcher, preview from the main checkout:

```sh
dotfiles update --overlay ../dotfiles-private --only symlinks --dry-run
```

This preview saves the overlay path once configuration loads cleanly. Confirm
the overlay path in the header and review every planned link. If
`~/.config/git/config` does not exist yet, the preview plans the new link; if
it still points to the main checkout, expect a conflict, since dotfiles never
silently retargets an existing link.

Inspect the existing destination before touching it:

```sh
readlink "$HOME/.config/git/config"
```

In PowerShell:

```powershell
Get-Item -LiteralPath "$HOME/.config/git/config" |
    Select-Object FullName, LinkType, Target
```

Confirm it is the old link you intend to replace. Back up its resolved contents
and recorded target using the
[backup procedure](application-settings.md#back-up-and-restore-a-file), then
move the symlink itself aside rather than the file it points to (and preserve
ordinary files or unrelated links instead of assuming they are managed). Re-run
the preview until it plans the overlay link cleanly, then drop `--dry-run` to
apply. Verify both parts afterward:

1. Run `dotfiles explain .config/git/config` to confirm `Source` points into
   the overlay and `Update: already linked` reports a match.
2. In a repository that uses this identity, run
   `git config --show-origin --get user.name` and check both the **value** and
   origin (Git may display the home symlink path rather than its resolved
   source, and a repository-local value can still override global settings).

Keep the overlay directory in place while its links are active, and keep your
backup until both checks pass.

## Select and preview

From the main checkout, inspect an overlay in `../dotfiles-private` (assuming a
profile is already saved; see [Getting started](getting-started.md) otherwise):

::: warning Review overlay scripts before previewing
A full update preview can run the overlay's setup scripts in `--check` and
`--dryrun` modes. Read them first: they run with your user permissions, and
neither flag restricts what a script can do.
:::

```sh
dotfiles list --overlay ../dotfiles-private
dotfiles update --overlay ../dotfiles-private --dry-run
```

Use `list` to inspect tasks without saving the selection. Check the resolved
overlay and profile in the update preview header.

Missing optional config files in an overlay simply contribute nothing. If an
overlay file or its `conf/` directory is a broken symlink, however, loading
stops with an error rather than silently falling back to the main checkout. Fix
the broken target or remove the stale link before retrying.

### Selection and persistence

Dotfiles resolves the overlay path in this order:

1. `--overlay <PATH>` on the command line
2. A nonempty `DOTFILES_OVERLAY` environment variable
3. The repository-local Git setting `dotfiles.overlay`

Relative paths resolve from your **current working directory**, and the target
must be an existing directory. Passing `--overlay` to `update`, `remove`, or
`check` persists its absolute path once configuration loads (`update --only
repository` saves it at startup without loading TOML). `DOTFILES_OVERLAY`,
`list`, and `explain` affect only the current invocation.

<details>
<summary>Linked worktrees and read-only overlays</summary>

- **Linked Git worktrees:** Passing a linked worktree (where `.git` is a file)
  to `--overlay` asks for interactive confirmation (defaulting to no) and is
  rejected in unattended or CI runs. Prefer a primary checkout for unattended
  overlays.
- **Locking and read-only overlays:** `update`, `remove`, and `check`
  coordinate Git overlays with account-local locks. You can inspect or apply a
  read-only overlay with `update --no-repo-update`, though syncing it with Git
  requires write access to its Git metadata.

</details>

## Merge rules

When an active overlay entry targets the same package, destination, or setting
as a main entry, the overlay entry replaces the main one. Matching is by the
effective resource key below, not by section header or array index:

| Configuration | Entries match by |
|---|---|
| Packages | Package name (case-insensitive on Windows) |
| Symlinks | Normalized home target after glob expansion |
| Global Git settings | Key (case-insensitive section and variable, case-sensitive subsection) |
| Agent settings | Harness and exact, case-sensitive key |
| Registry | Case-insensitive key path and value name |
| Permissions | Normalized home-relative path |
| Systemd units | Scope and exact unit name |
| System configuration files | Normalized destination path |
| VS Code extensions | Case-insensitive extension ID |

Unmatched main entries stay in order, followed by new overlay entries. System
configuration files are the one ordering exception: a replaced destination
keeps its original position from the main config so prerequisite files still
apply before files that activate them, while brand-new destinations append at
the end. Missing or empty overlay files never erase base entries, and inactive
overlay categories cannot override active main entries.

Every replacement takes the **entire overlay entry** (with sources still
relative to the overlay) rather than merging two symlink files line by line.
Each repository is validated on its own before replacements are merged.

APM YAML fragments follow their own filename-based
[fragment merge rules](agent-configuration.md#configuration-fragments) rather
than TOML overlay precedence.

### Conflicting desired state

Within either repository, active declarations cannot assign conflicting values
to the same target (identical duplicates are allowed). Conflicts stop loading
before any task runs, even with `--only` or `--dry-run`, and an overlay cannot
mask an invalid base configuration:

- **Git settings (`git.conflicting-values`):** matching normalized keys must
  have identical literal values.
- **Windows registry entries (`registry.conflicting-values`):** matching
  case-insensitive key paths and value names must agree in both type and data
  (stops loading on Windows; reported by `dotfiles check` off Windows).
- **Systemd units (`systemd.conflicting-values`):** matching scope and unit
  names must agree on both `enabled` and `graphical`.
- **Agent settings (`agent-settings.conflicting-values`):** matching harness
  and exact keys must have identical values, and
  [overlapping parent/child keys](../reference/agent-settings.md#configuration)
  must agree both within a repository and after merging.

Inactive categories for Git settings, systemd units, and agent settings are
excluded from these active-value conflict checks.

## Overlay scripts

Reach for an overlay script only when none of the built-in declaration types
can express the work. Declare scripts in the overlay's `conf/scripts.toml` (the
main repository cannot define script tasks); each active entry becomes its own
task in `update` and `remove`.

See [Overlay scripts](../reference/overlay-scripts.md) for the schema, selector
naming, required `--check`, apply, `--dryrun`, and `--remove` modes, and
failure handling. Keep secrets out of script output: it can end up in logs.

## Change or stop using an overlay

**If you want to detach managed links or run a script's removal action, do
that before switching overlays.** [Removal](../reference/task-reference.md#remove-tasks)
uses the current declarations and sources. Keep the old overlay intact until
you've checked the result. Packages, settings, services, and merged system
files remain in place unless you change them separately.

Select a different overlay by passing `--overlay` to `update`, `remove`, or
`check`. To use just the main checkout, clear the saved setting there:

```sh
git config --local --unset dotfiles.overlay
```

Unset `DOTFILES_OVERLAY` as well if it is set, since the environment variable
overrides the saved Git value.

If the wrong overlay is still selected, see
[Overlay is ignored](troubleshooting.md#overlay-is-ignored).
