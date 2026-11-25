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

Each active entry in a private overlay's `conf/scripts.toml` becomes a task for
`update` and `remove`. Its configured name is the console label, its selector is
`script-<normalized-name>`, and each script records its own result and output. The main
repository's `conf/scripts.toml` is never loaded.

Script tasks are discovered at startup and again after a repository restart, never
added dynamically while other tasks are running.

## Configuration

```toml title="dotfiles-private/conf/scripts.toml (example)"
[[linux.scripts]]
name = "Configure private workstation"
path = "scripts/configure-workstation.sh"
description = "Apply workstation settings that are not already in place"
```

`name` and `path` are required; `description` is optional. Names must be non-empty and
produce distinct selectors. This example normalizes to `script-configure-private-workstation`
by lowercasing ASCII alphanumeric words and joining them with hyphens. Putting it under
`[[linux.scripts]]` keeps this POSIX shell script off Windows; use `[[windows.scripts]]`
with a `.ps1` file for Windows-only work.

Paths must stay inside the overlay even after resolving symlinks, and scripts run with
the overlay root as their working directory. `.ps1` matching is case-insensitive and
runs under PowerShell (`pwsh`, or Windows PowerShell when available on Windows); other
extensions require POSIX `sh` on the target platform.

## Script protocol

Implement every mode before enabling a script:

| Invocation | Required behavior |
|---|---|
| `--check` | Inspect only. Exit 0 when configured, 1 when work is needed, or another non-zero value on failure. |
| No flag | Apply needed changes; repeated runs must leave already-correct state alone. Fail with a non-zero exit on errors. |
| `--dryrun` | Describe needed changes without applying them. Note the spelling. |
| `--remove` | Conservatively remove the script's managed state. |

Update runs `--check` first and calls apply or `--dryrun` only when the check returns 1.
Remove uses the opposite convention: 0 means there is state to remove and 1 means nothing
to remove. A remove dry-run checks and reports removal without invoking `--remove` or
`--dryrun`. Missing or unreadable scripts fail, including files that disappear between
inspection and execution.

For a script that can leave partial state behind, set `check_remove = true` on its
entry and implement `--check-remove`. Return 0 whenever managed state needs cleanup
(including after a failed apply) and 1 only when it is absent; any other exit code
stops removal. The probe must never modify state.

## Preview and remove

With an [active overlay](../guides/private-overlays.md#select-and-preview), target the
selector produced by the declaration above:

```sh
dotfiles update --only script-configure-private-workstation --dry-run
dotfiles remove --only script-configure-private-workstation --dry-run
```

Both commands run the script's check mode, and an update preview can also run
`--dryrun` (a removal preview does not). Review the script before running either
command, since its safety and completeness depend entirely on its own code.

Check that the reported work matches the script's intended scope, and fix protocol
issues in an isolated fixture before retrying on a real machine. Only [apply the
reviewed change](task-reference.md#preview-and-apply) once every mode is implemented,
and inspect the resulting state afterward rather than treating exit code 0 as proof.

The Rust CLI cannot force a script's check to be read-only or roll back a partially
applied script. Test every mode in an isolated fixture before enabling it on a real
machine; a regular preview invokes the script directly and is not a sandbox.
[Private overlays](../guides/private-overlays.md) covers the selection and trust
boundary, and [Writing tests](../development/writing-tests.md#confine-every-writable-path)
describes the isolation expected for contributor tests.

Keep the original overlay, script, and profile available until removal finishes, and
keep private values out of script output since it can appear in the console and run log.
