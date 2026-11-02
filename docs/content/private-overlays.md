---
title: Private overlays
description: Keep private or machine-specific configuration separate, select an overlay, and understand merging, scripts, and cleanup.
---

# Private overlays

An overlay adds private or machine-specific configuration without putting it
in the main dotfiles repository. Use a separate private repository or directory
with the same configuration layout. The main checkout remains the shared base.

An overlay **extends** the base; it is not a separate profile, a generic file
patch, or a security boundary. Never copy private content or credentials into
public examples, fixtures, logs, or agent messages.

## Create an overlay

Only include the files you need:

```text
dotfiles-private/
  conf/
    git-config.toml
  symlinks/
  system/
  scripts/
```

Overlay configuration files are optional. They use the same
[formats](configuration-reference.md) and
[category rules](profiles-and-categories.md) as the main repository. For example,
an overlay's `conf/git-config.toml` could contain:

```toml title="dotfiles-private/conf/git-config.toml"
[base]
settings = [{ key = "core.editor", value = "nvim" }]
```

This replaces the matching Git setting without discarding the other base
settings. Symlink sources belong under the overlay's `symlinks/`; system-file
fragments belong under its `system/`. Their paths stay relative to the
repository that declared them, not the main checkout.

## Select and preview

After [getting started](getting-started.md), inspect a sibling overlay using
your selected profile:

```sh
dotfiles list --overlay ../dotfiles-private
dotfiles update --overlay ../dotfiles-private --dry-run
```

`list` does not run tasks or save the selection. The update preview saves an
explicit overlay path for future runs, even though it does not apply ordinary
configuration changes. Check the resolved path and profile in the header.
After reviewing the result, normal `dotfiles update` runs use the saved overlay.

:::caution[Review overlay scripts before previewing]
`--check` and `--dryrun` execute overlay code with your access. They can use the
network, expose private data, or mutate files if implemented incorrectly.
A private repository and a dry-run flag do not sandbox that code.
:::

### Selection and persistence

Selection priority is:

1. `--overlay <PATH>`
2. Nonempty `DOTFILES_OVERLAY`
3. Repository-local Git setting `dotfiles.overlay`

Relative paths resolve from the **invoking working directory**. Explicit
selections in update, remove, and check are saved as absolute paths. Existing
saved relative paths retain their working-directory meaning until replaced.
Read-only `list` never saves a selection.

Explicitly selecting a linked Git worktree (`.git` is a file) requires
confirmation, defaulting to no. Non-interactive execution declines it, including
CI on an attached terminal. Prefer a stable primary checkout for an unattended
overlay; an ordinary `.git` directory does not need that confirmation.

## Merge rules

An active overlay entry replaces a main entry with the same **resource
identity**, not the same section name or array position:

| Configuration | Replacement identity |
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

Replacement retains the **entire overlay entry** and its source origin. It
does not merge the contents of two symlink source files. Validation retains
both repositories' declarations and checks within-source conflicts before
replacement.

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
[overlapping parent/child keys](configuration-reference.md#agent-harness-settings).

## Overlay scripts

Use scripts only for work that declarative configuration cannot express.
Only an overlay's `conf/scripts.toml` is loaded; the main repository cannot
declare these tasks.

```toml title="dotfiles-private/conf/scripts.toml" {3}
[[linux.scripts]]
name = "Configure private workstation"
path = "scripts/configure-workstation.sh"
description = "Apply workstation settings that are not already in place"
```

`name` and `path` are required; `description` is optional. Names must be nonempty
and produce distinct selectors. This example becomes
`script-configure-private-workstation`: ASCII alphanumeric words are lowercased
and joined with hyphens. The `linux` category keeps this POSIX shell script off
Windows; use `[[windows.scripts]]` and a `.ps1` file for a Windows-only task.

Paths must stay inside the overlay, including after symlink resolution.
Scripts run with the overlay root as their working directory. `.ps1` matching
is case-insensitive and selects PowerShell (`pwsh`, or Windows PowerShell
when available on Windows); other extensions need POSIX `sh` on the selected
platform.

Implement every protocol mode:

| Invocation | Required behavior |
|---|---|
| `--check` | Inspect only. Exit 0 when configured, 1 when work is needed, or another nonzero value on failure. |
| No flag | Apply needed changes; repeated runs must leave already-correct state alone. Fail with a nonzero exit on errors. |
| `--dryrun` | Describe needed changes without applying them. Note the spelling. |
| `--remove` | Conservatively remove the script's managed state. |

Update checks first and applies or previews only when the check returns 1.
Remove uses the same check: 0 means something to remove, 1 means nothing.
A remove dry run checks and reports removal but invokes neither `--remove`
nor `--dryrun`. Missing or unreadable scripts fail, including disappearance
between inspection and execution.

Keep private values out of output, which can enter the console and run log.
See [Dynamic overlay tasks](task-reference.md#dynamic-overlay-tasks) for discovery
and task selection.

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
[An overlay appears to be ignored](troubleshooting.md#an-overlay-appears-to-be-ignored).
