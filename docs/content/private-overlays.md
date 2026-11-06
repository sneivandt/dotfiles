---
title: Private overlays
description: Keep private or machine-specific configuration separate, select an overlay, and understand merging, scripts, and cleanup.
---

# Private overlays

An overlay adds private or machine-specific configuration without putting it
in the main dotfiles repository. Use a separate private repository or directory
with the same configuration layout. The main checkout remains the shared base.

An overlay adds declarations and replaces matching entries in the base
configuration. It does not patch the contents of application files. Keep
private content and credentials out of public examples, fixtures, logs, and
agent messages.

## Create an overlay

Only include the files you need:

```text
dotfiles-private/
  conf/
    git-config.toml
```

Overlay configuration files are optional. They use the same
[formats](configuration-basics.md) and
[category rules](profiles-and-categories.md) as the main repository. For example,
an overlay's `conf/git-config.toml` could contain:

```toml title="dotfiles-private/conf/git-config.toml"
[base]
settings = [{ key = "core.editor", value = "nvim" }]
```

This adds `core.editor`, or replaces a main declaration for that key, without
discarding other settings. Symlink sources belong under the overlay's
`symlinks/`; system-file fragments belong under its `system/`. Their paths stay
relative to the repository that declared them, not the main checkout.

### Replace an application file

For a private Git identity, replace the linked Git configuration rather than
editing the public source. From the main checkout, these POSIX shell commands
create a new sibling overlay; use a different directory if it already exists:

```sh
mkdir ../dotfiles-private
mkdir -p ../dotfiles-private/conf ../dotfiles-private/symlinks/config/git
cp symlinks/config/git/config ../dotfiles-private/symlinks/config/git/config
```

Edit the copied file's `[user]` name and email, and review `[github]` and the
other defaults. Create `../dotfiles-private/conf/symlinks.toml` containing:

```toml title="dotfiles-private/conf/symlinks.toml"
[base]
symlinks = ["config/git/config"]
```

The same home destination makes this entry replace the base link. The source
now comes from the overlay; its contents are not merged with the original.
On Windows, create the same directory layout and copy the source with PowerShell
or Explorer.

With a saved profile and installed launcher, preview from the main checkout:

```sh
dotfiles update --overlay ../dotfiles-private --only symlinks --dry-run
```

Look for `~/.config/git/config` pointing into `dotfiles-private`, and review all
other selected links too. Back up existing targets before removing `--dry-run`.
After applying, run `git config --show-origin --get user.name` to verify the
selected identity. Keep this overlay available for future runs.

## Select and preview

From your main checkout, inspect an overlay in the sibling directory
`../dotfiles-private`. These commands use your saved profile; if you have not
selected one, follow [Getting started](getting-started.md) first.

```sh
dotfiles list --overlay ../dotfiles-private
dotfiles update --overlay ../dotfiles-private --dry-run
```

`list` does not run tasks or save the selection. The update preview saves an
explicit overlay path for future runs, even though it does not apply ordinary
configuration changes. Check the resolved path and profile in the header.
After reviewing the result, normal `dotfiles update` runs use the saved overlay.

Check that the directory exists and contains the intended files. Missing
optional configuration contributes nothing, so a mistyped path can look like
an empty overlay rather than producing a missing-file error.

::: warning Review overlay scripts before previewing
`--check` and `--dryrun` execute overlay code with your access. They can use the
network, expose private data, or mutate files if implemented incorrectly.
A private repository and a dry-run flag do not sandbox that code.
:::

### Selection and persistence

Selection priority is:

1. `--overlay <PATH>`
2. Nonempty `DOTFILES_OVERLAY`
3. Repository-local Git setting `dotfiles.overlay`

Relative paths resolve from the **directory where you run the command**. A
path passed with `--overlay` to update, remove, or check is saved
as an absolute path. Environment selections are not saved. Existing saved
relative paths retain their working-directory meaning until replaced.
Read-only listing never saves a selection. Check for a persistence warning if a
later run does not remember the path.

Passing a linked Git worktree (`.git` is a file) with `--overlay` requires
confirmation, defaulting to no. Update, remove, and check decline it in
non-interactive mode, including CI. `list` can ask when both input and error
output are terminals. The prompt applies to the explicit flag, not every use
of a saved overlay. Prefer a stable primary checkout for an unattended overlay;
an ordinary `.git` directory does not need that confirmation.

## Merge rules

An active overlay entry replaces a main entry when both manage the same
package, destination, or setting. The table lists what must match for each kind
of entry; matching section names or array positions do not cause replacement:

| Configuration | Entries match by |
|---|---|
| Packages | Package name; case-insensitive on Windows |
| Symlinks | Normalized home target after glob expansion |
| Global Git settings | Key; section/variable ignore case, subsection does not |
| Agent settings | Harness and exact, case-sensitive key |
| Registry | Case-insensitive path and value name |
| Permissions | Normalized home-relative path |
| Systemd units | Scope and exact unit name |
| System configuration files | Normalized destination path |
| VS Code extensions | Case-insensitive extension ID |

Unmatched main entries remain, followed by overlay entries. Missing or empty
overlay files contribute nothing; they do not erase the base. Inactive overlay
categories cannot override active main entries.

Replacement uses the **entire overlay entry**, with source paths still relative
to the overlay. It does not merge the contents of two symlink source files.
Each repository's declarations are checked for conflicts before replacements
are applied.

APM YAML fragments have their own
[configuration-fragment rules](agent-configuration.md#configuration-fragments).
Their filename ordering is separate from TOML overlay precedence.

### Conflicting desired state

Within either repository, active Git settings and Windows registry entries
cannot assign different values to the same target. Identical assignments are
allowed. Conflicts stop loading before tasks run, even with `--only` or
`--dry-run`; an overlay cannot hide an invalid base.

Git values compare literally; registry values must agree in both type and
data. Diagnostics use `git.conflicting-values` or `registry.conflicting-values`.
Inactive Git categories and registry entries off Windows do not participate
in these active-value checks. Agent settings also check
[overlapping parent/child keys](agent-settings.md#configuration).

## Overlay scripts

Use a script only for work that the existing declarations cannot express.
Declare it in the overlay's `conf/scripts.toml`; the main repository cannot
define script tasks. Each active entry becomes a separate task for update
and remove.

[Overlay scripts](overlay-scripts.md) contains the declaration format, selector
rules, required check/apply/preview/remove modes, and failure behavior. Review
the script before previewing: its check and preview flags are a cooperative
contract, not a sandbox. Keep private values out of its output and logs.

## Change or stop using an overlay

Pass a different `--overlay` to select and save another path. To forget the
saved selection, run this from the main checkout:

```sh
git config --local --unset dotfiles.overlay
```

Also clear `DOTFILES_OVERLAY` if it is set, because it takes precedence over
the saved value.

**Forgetting an overlay does not undo its changes.** Keep its declarations
and sources available while running any intended
[removal tasks](task-reference.md#remove-tasks). Packages, settings, services, and
merged system files generally remain; remove is not a universal rollback.

For selection problems, see
[Overlay is ignored](troubleshooting.md#overlay-is-ignored).
