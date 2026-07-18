---
name: overlay-scripts
description: >
  Overlay configuration and convention-based script task workflow. Use when
  changing private overlay loading, script resources, or dynamic task injection.
---

# Overlay Scripts

An overlay repository extends the main declarative config without placing
private state in this repository. Keep overlay discovery, config merging,
script execution, and dynamic task injection as separate responsibilities.

## Overlay Resolution

Resolve the overlay once at startup in this priority order:

1. `--overlay`
2. `DOTFILES_OVERLAY`
3. repository-local `dotfiles.overlay` git config

Resolution and persistence live in
`cli/src/domains/overlay/config/overlay.rs`. Do not repeatedly resolve the path
inside loaders or tasks.

## Config Merging

`SectionLoader` loads the main config and the matching overlay file, then
appends overlay entries under the same category rules. Overlay files extend the
main desired state; they do not replace it.

When adding an overlay-aware config surface:

- route it through `SectionLoader`
- preserve typed deserialization and category filtering
- keep missing overlay files optional
- add merge-order and category tests

## Script Contract

Overlay scripts implement four modes:

| Invocation | Meaning |
|---|---|
| no arguments | apply desired state |
| `--check` | exit 0 when correct, 1 when apply is needed, other non-zero on failure |
| `--dryrun` | preview without mutation |
| `--remove` | undo managed state |

Shell scripts must be POSIX `sh`. PowerShell scripts must support non-interactive
execution. Every mode must return failures rather than printing an error and
exiting successfully.

Script paths are relative to the overlay root. Reject absolute paths and `..`
components before execution.

## Runtime Boundaries

- `cli/src/domains/overlay/config/scripts.rs` parses script entries.
- `cli/src/domains/overlay/resources/script.rs` owns check, preview, apply, and remove behavior.
- `ReportOverlayScriptSnapshot` reports scripts loaded into config during Sync.
- `OverlayScriptTask` provides one dynamic Provision task per entry.
- The command runner reloads the script handle with the rest of configuration,
  then builds and injects dynamic tasks after Sync so newly pulled definitions
  run in the same command.

Dynamic tasks are not registered as individual static catalog entries. Keep the
reporting task registered in Sync; dynamic task creation happens at the
Sync-to-Provision phase boundary.

All subprocesses go through the executor abstraction. Preserve interpreter
selection and non-interactive PowerShell behavior when changing command
construction.

## Change Checklist

1. Update the typed config contract.
2. Preserve path validation and overlay-root containment.
3. Keep all four script modes wired.
4. Update dynamic task creation and phase assumptions together.
5. Add focused tests for exit-code mapping, dry-run failures, path rejection,
   and overlay merging.

## Validation

- Use `resource-implementation` for `ScriptResource` behavior.
- Use `profile-system` and `toml-configuration` for category-aware merging.
- Use `cross-platform-verification` after Rust or interpreter changes.

## Rules

- Overlay config appends; it never silently replaces main config.
- Dry-run mode must not mutate.
- Check failures are distinct from “state missing.”
- PowerShell execution must remain non-interactive.
- Private overlay content must not be copied into this repository or its skills.
