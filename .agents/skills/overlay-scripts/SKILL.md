---
name: overlay-scripts
description: >
  Use for private overlay path resolution, SectionLoader overlay merging,
  overlay script resources, or dynamic OverlayScriptTask discovery. Not for
  ordinary repository scripts or public config alone.
---

# Overlay Scripts

Never copy private overlay content into this repository or its skills.

## Overlay resolution

Resolve once at startup:

```text
--overlay > DOTFILES_OVERLAY > repository dotfiles.overlay
```

An explicit linked worktree (`.git` is a file) requires interactive `[y/N]`
confirmation before use or persistence. Empty input and non-interactive runs
decline it.

The normal resolver persists an explicit `--overlay` path; the read-only resolver
does not. Do not invoke the normal resolver against the real checkout merely to
test selection. Use temporary repositories and synthetic overlay fixtures.

## Config and scripts

- `SectionLoader` appends optional overlay sections to public desired state
  under the same category rules; overlays extend rather than replace.
- Script paths are overlay-relative. Reject absolute paths and `..`, and preserve
  canonical containment checks for symlink escapes.
- Scripts implement apply, `--check`, `--dryrun`, and `--remove`.
- Check exits 0 when correct, 1 when apply is needed, and another non-zero on
  failure. Every mode propagates failures.
- Shell scripts are POSIX `sh`; PowerShell runs non-interactively.
- All subprocesses use the executor.
- Dry-run is cooperative: `--check` and `--dryrun` execute overlay code, and the
  engine cannot stop a misbehaving script from mutating. Inspect trusted scripts
  or use fixtures before treating a preview as safe.

## Dynamic task discovery

Build one `OverlayScriptTask` per active entry from the immutable startup
configuration. Dynamic tasks are not catalog entries. The catalog-registered
reporting task is internal. Preserve config order and stable
`script-<normalized-name>` selectors.

When repository synchronization changes the checkout, the guarded child process
reloads configuration and rebuilds the entire task set. Do not add mid-run
configuration reload or late task discovery.

Test merge order, categories, path rejection, all exit mappings, dry-run
failure, startup discovery, and restart selection.

Use [resolution](../../../cli/src/domains/overlay/resolution.rs),
[script execution](../../../cli/src/domains/overlay/resources/script.rs), and
[dynamic tasks](../../../cli/src/domains/overlay/scripts.rs) as the source of
truth; keep private paths and script output out of public reports and fixtures.
