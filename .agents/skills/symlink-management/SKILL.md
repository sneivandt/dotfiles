---
name: symlink-management
description: >
  Use for conf/symlinks.toml, SymlinkResource target computation,
  or install/remove/materialization behavior.
---

# Symlink Management

Sources are relative to `symlinks/`, have no leading dot, and cannot escape that
tree. Explicit targets are home-relative. Reject absolute paths, `..`, missing
sources, duplicate targets, and overlapping parent/child targets.

## Target rules

- A string source receives the normal leading-dot target mapping.
- Use `{ source, target }` for paths that must not receive a dot prefix.
- One canonical source may feed multiple explicit targets.
- Do not create links inside another managed directory link.

## Resource behavior

- The task uses strict resource processing.
- `SymlinkResource` owns platform-specific creation.
- Correct links are already converged; wrong links are repaired only when safe.
- Uninstall materializes managed links into real files/directories rather than
  deleting their content.
- Never overwrite an existing non-link target during removal.

## Add a symlink

1. Add the real source under `symlinks/`.
2. Add it to the correct `conf/symlinks.toml` section.
3. Add config-drift coverage where needed.

Use `windows-specific-patterns` for capability/junction behavior. Use the TOML
and dry-run coverage in [Testing](../../../docs/TESTING.md#choosing-coverage).
