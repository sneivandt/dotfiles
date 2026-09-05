---
name: symlink-management
description: >
  Use for conf/symlinks.toml, source glob expansion, SymlinkResource target
  computation, or managed-link install/remove/materialization. Not for editing
  the contents of an already-managed application config without changing links.
---

# Symlink Management

Sources are relative to their owning repository's `symlinks/` tree and cannot
escape it. Explicit targets are home-relative. Validate empty/absolute paths,
`..`, source containment, source existence, duplicate targets, and overlapping
parent/child targets using the existing cross-platform helpers.

## Target rules

- A string source receives the normal leading-dot target mapping: `config/foo`
  becomes `~/.config/foo`, not `~/.config/.foo`.
- Use `{ source, target }` for paths that must not receive a dot prefix.
- One canonical source may feed multiple explicit targets.
- Do not create links inside another managed directory link.
- Resolve overlay entries from their recorded `origin`, not the public root.

## Source globs

Only a complete `*` path segment is supported; it captures one segment.
Expansion is deterministic and happens before active-target conflict checks.
Malformed or unmatched globs fail. Explicit target `*` captures must match
source captures in number; do not assume general shell glob syntax or recursive
`**` support. See [glob expansion](../../../cli/src/domains/files/config/symlinks/glob_expansion.rs).

## Resource behavior

- The task uses strict resource processing.
- `SymlinkResource` owns platform-specific creation.
- Correct links are already converged. Install replaces incorrect targets,
  including regular files/directories, with a warning and no backup. Preserve
  `pre_apply_warning()` and never run a real install as a casual verification
  step against the user's home.
- Uninstall materializes managed links into real files/directories rather than
  deleting their content. Missing targets also materialize via
  `remove_when_missing()`; incorrect or user-replaced targets are left alone by
  normal removal processing.
- Never overwrite an existing non-link target during removal.
- Stage copies before unlinking the target; preserve temporary cleanup and
  cross-filesystem handling. Cover copy/rename failures rather than assuming
  the whole materialization is atomic.

## Add a symlink

1. Add the real source under `symlinks/`.
2. Add it to the correct `conf/symlinks.toml` section.
3. Check the selected and excluded profiles and add config-drift coverage where
   needed; do not enable a platform-specific config in an incompatible profile.

Read [source and target validation](../../../cli/src/domains/files/config/symlinks.rs)
and [materialization](../../../cli/src/domains/files/resources/symlink/materialize.rs).
Use `windows-specific-patterns` for capability/junction behavior. Use the TOML
and dry-run coverage in [Testing](../../../docs/TESTING.md#choosing-coverage).
