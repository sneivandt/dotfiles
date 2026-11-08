---
title: Overlay scripts
description: Declare private setup and removal scripts, understand their selectors, and implement the task protocol.
---

# Overlay scripts

Overlay scripts are the escape hatch for private setup that the built-in
declarations cannot express. A script is executable code with its own
inspection, application, preview, and removal behavior. Prefer an existing
package, file, service, or settings declaration when it already owns the work.

| Task detail | Value |
|---|---|
| Selector | `script-<normalized-name>` for each active declaration |
| Commands | `update`, `remove` |
| Availability | Active private overlay script declarations |
| Task dependencies | None |
| Implementation | [overlay/scripts.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/overlay/scripts.rs) |

## What it does

Each active entry in the private overlay's `conf/scripts.toml` becomes a task for
`update` and `remove`. Its configured name is the console label, and the selector is
`script-<normalized-name>`. Each script has its own result and captured output. The main
repository's `conf/scripts.toml` is not loaded.

Tasks are discovered at startup and again after a repository restart. They are not added
while other tasks are running. An internal reporting task records the script count
without executing them.

## Configuration

```toml title="dotfiles-private/conf/scripts.toml (example)"
[[linux.scripts]]
name = "Configure private workstation"
path = "scripts/configure-workstation.sh"
description = "Apply workstation settings that are not already in place"
```

`name` and `path` are required; `description` is optional. Names must be nonempty and
produce distinct selectors. This example becomes `script-configure-private-workstation`:
ASCII alphanumeric words are lowercased and joined with hyphens. The `linux` category
keeps this POSIX shell script off Windows; use `[[windows.scripts]]` and a `.ps1` file
for a Windows-only task.

Paths must stay inside the overlay, including after symlink resolution. Scripts run with
the overlay root as their working directory. `.ps1` matching is case-insensitive and
selects PowerShell (`pwsh`, or Windows PowerShell when available on Windows); other
extensions need POSIX `sh` on the selected platform.

## Script protocol

Implement every mode before enabling a script:

| Invocation | Required behavior |
|---|---|
| `--check` | Inspect only. Exit 0 when configured, 1 when work is needed, or another nonzero value on failure. |
| No flag | Apply needed changes; repeated runs must leave already-correct state alone. Fail with a nonzero exit on errors. |
| `--dryrun` | Describe needed changes without applying them. Note the spelling. |
| `--remove` | Conservatively remove the script's managed state. |

Update checks first and applies or previews only when the check returns 1. Remove uses
the same check: 0 means something to remove, 1 means nothing. A remove dry run checks
and reports removal but invokes neither `--remove` nor `--dryrun`. Missing or unreadable
scripts fail, including disappearance between inspection and execution.

## Preview and remove

With an [active overlay](../guides/private-overlays.md#select-and-preview), use the selector
produced by the declaration above:

```sh
dotfiles update --only script-configure-private-workstation --dry-run
dotfiles remove --only script-configure-private-workstation --dry-run
```

These commands execute the script's checks. An update preview can also execute its
`--dryrun` mode; a removal preview does not. Review the script before either command.
Its safety and completeness depend on its implementation.

Compare the reported work with the script's intended ownership boundary.
Fix protocol errors in an isolated fixture before retrying against the
machine. Only [apply the reviewed change](task-reference.md#preview-and-apply)
once every mode is implemented; inspect the script's actual managed state
afterward rather than treating a zero exit status as an independent audit.

The Rust CLI cannot make a script's check read-only or undo a partially applied
script. Test every protocol mode in an isolated fixture before selecting it
for a real machine. A normal preview is not that fixture: it actually invokes
the script. [Private overlays](../guides/private-overlays.md) explains the selection and
trust boundary; [Writing tests](../development/writing-tests.md#confine-every-writable-path)
explains the isolation expected for contributor tests.

Keep the original overlay, script, and profile available until removal finishes. Keep
private values out of output, which can enter the console and run log.
