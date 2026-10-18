# Dotfiles Agent Instructions

This repository manages Linux and Windows desired state through a Rust CLI.
Start with the files that own the requested behavior; application configuration
changes usually do not need engine changes.

## Working safely

- This checkout may be the live source of home-directory symlinks. Edit tracked
  sources, not installed copies, and account for applications that auto-reload.
- A code change is not permission to run `dotfiles update` or `dotfiles remove`
  against the host, update packages, deploy agent plugins, restart the desktop,
  or make privileged changes. Use isolated fixtures and scoped checks. Ask before
  machine changes outside the user's authorized scope.
- Never include private overlay content, credentials, or unsanitized machine logs
  in public code, fixtures, skills, or remote requests.
- Dry-run is not a sandbox: it can inspect the host, write logs and persist
  selection metadata; overlay preview scripts execute code. Use fixtures to
  verify that managed state is unchanged.

## Implementation boundaries

- Keep desired-state declarations in `conf/` and managed content in `symlinks/`
  or `system/`. Wrappers bootstrap and forward; Rust owns command semantics.
- Use `Resource` for independent items and `Operation` for a workflow that
  converges as one unit. Reuse their lifecycle helpers; keep discovery read-only,
  apply idempotent and preview non-mutating. Failed discovery is not absence.
- Tasks own metadata, applicability and dependencies. Register static
  `update`/`remove` tasks in [`app/catalog.rs`](cli/src/app/catalog.rs);
  command-specific tasks belong in that command's task list. Cross-domain
  dependencies belong in the app layer; list order does not schedule tasks.
- Use context capabilities and injected adapters for runtime platform decisions.
  Keep compile-time gates where platform-only APIs require them.
- Keep conditional symlinks aligned with profile categories. Removing a profile
  category does not authorize deleting previously managed state; `remove` is
  a separate capability, not a universal undo.