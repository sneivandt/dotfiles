---
name: symlink-management
description: >
  Use for conf/symlinks.toml, source glob expansion, SymlinkResource target
  computation, or managed-link install/remove/materialization. Not for editing
  the contents of an already-managed application config without changing links.
---

# Symlink Management

Sources live in [symlinks.rs](../../../cli/src/domains/files/config/symlinks.rs),
[glob_expansion.rs](../../../cli/src/domains/files/config/symlinks/glob_expansion.rs),
[state.rs](../../../cli/src/domains/files/resources/symlink/state.rs), and
[materialize.rs](../../../cli/src/domains/files/resources/symlink/materialize.rs).

## Targets and globs

- Sources are relative to their declaring repo's `symlinks/` tree (resolved
  from the entry's recorded `origin` for overlays); explicit targets are
  home-relative.
- Bare string entries receive a leading dot on the first segment (`config/foo`
  -> `~/.config/foo`, not `~/.config/.foo`). Use `{ source, target }` to omit
  the leading dot; one source may feed multiple targets. Never link inside
  another managed directory link.
- Source globs support only a complete `*` path segment (single-segment capture,
  no general shell globbing or `**`). Expansion runs deterministically before
  conflict checks; malformed/unmatched globs fail, and explicit target `*`
  count must match source captures.

## Install and removal invariants

- `SymlinkResource` uses strict processing. Existing non-link files/dirs and
  unrelated symlinks are preserved as conflicts (`Invalid`). The **only**
  permitted occupied-target replacement is a recognized historical managed link
  (`legacy_link`), rechecked immediately before replacing.
- Check where existing target parents resolve: external destinations require
  `allow_external_target` for both install and removal.
- Uninstall materializes managed links into real files/directories rather than
  deleting content (stage copy before unlink, handle cross-filesystem errors
  without assuming atomicity). Missing targets stay absent unless
  `--restore-missing` is passed; never overwrite an existing non-link target.

Use `windows-specific-patterns` for Windows symlink capability and junction
fallback.
