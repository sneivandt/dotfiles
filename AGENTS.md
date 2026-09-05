# Dotfiles Agent Instructions

Load the narrowest relevant skill from `.agents/skills/`. Load a companion only
when the change crosses into its subsystem; do not recurse through related
skills. If no skill fits, follow the owning code and the guides below rather
than loading an unrelated skill. Reviewing skill text does not require
activating every skill being reviewed.

## Working safely

- Check the working tree and current branch before editing; preserve unrelated
  changes. Commit and push only when requested, and stage only intended files.
- This checkout may be the live source of home-directory symlinks. Edit tracked
  sources, not installed copies, and account for applications that auto-reload.
- A code change is not permission to run a real install/uninstall, update packages,
  deploy agent plugins, request elevation, or restart the desktop. Use isolated
  fixtures and scoped checks; ask before unrequested machine changes.
- Never include private overlay content, credentials, or unsanitized machine logs
  in public code, fixtures, skills, or remote requests.

## Repository invariants

- Wrappers (`dotfiles.sh`, `dotfiles.ps1`) bootstrap and forward only.
- Declarative desired state lives in `conf/`.
- Independent config-backed state generally uses `Resource`.
- Whole-workflow convergence generally uses `Operation`.
- Tasks own metadata, policies, dependencies, and orchestration boundaries.
- Mutations must be idempotent and dry-run safe.
- Prefer capability methods over direct operating-system checks.
- Static install/uninstall tasks belong in `cli/src/app/catalog.rs`;
  command-specific tasks belong in that command's task list.
- Conditional symlink behavior must remain aligned with profile categories.

## Guidance ownership

- [Contributing](docs/CONTRIBUTING.md) owns the human change workflow.
- [Architecture](docs/ARCHITECTURE.md) describes layers and runtime contracts.
- [Testing](docs/TESTING.md) owns validation commands and CI coverage.
- Skills contain only task-specific procedures and subsystem gotchas.
- Follow executable source when guidance has drifted; correct the affected
  guidance rather than changing working code to match stale instructions.
