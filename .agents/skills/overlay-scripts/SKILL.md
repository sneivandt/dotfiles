---
name: overlay-scripts
description: >
  Use for private overlay path resolution, overlay script resources, or dynamic
  OverlayScriptTask discovery. Not for generic TOML section loading/merging
  (use toml-configuration), ordinary repository scripts, or public config alone.
---

# Overlay Scripts

User-facing setup and protocol live in
[Private overlays](../../../docs/content/guides/private-overlays.md);
implementation lives in [resolution.rs](../../../cli/src/domains/overlay/resolution.rs),
[script.rs](../../../cli/src/domains/overlay/resources/script.rs), and
[scripts.rs](../../../cli/src/domains/overlay/scripts.rs).

## Resolution and persistence

- Precedence: `--overlay > DOTFILES_OVERLAY > repository dotfiles.overlay`.
- An explicit linked worktree (`.git` is a file) requires interactive `[y/N]`
  confirmation before use or persistence; empty input and non-interactive runs
  decline it.
- `resolution::resolve_from_args` and `resolution::resolve_read_only` are both
  read-only; [app/commands/runner.rs](../../../cli/src/app/commands/runner.rs)
  owns persisting an explicit `--overlay` selection.

## Script execution and dynamic discovery

- Script paths are overlay-relative: reject absolute paths and `..`, and
  preserve canonical containment checks against symlink escapes.
- Scripts implement apply, `--check`, `--dryrun`, and `--remove`. `--check`
  exits `0` (correct), `1` (apply needed), or other non-zero (error); every
  mode propagates failures. Shell scripts use POSIX `sh`; PowerShell runs
  non-interactively.
- Dry-run is cooperative: `--check` and `--dryrun` execute overlay code and
  cannot sandbox a misbehaving script.
- Build one `OverlayScriptTask` per active entry from the immutable startup
  snapshot (not catalog entries), preserving config order and stable
  `script-<normalized-name>` selectors. Repository updates rebuild dynamic tasks
  only across the guarded restart boundary.
