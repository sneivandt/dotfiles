# Dotfiles Agent Instructions

Load the narrowest relevant skill from `.agents/skills/`. Load a companion only
when the change crosses into its subsystem; do not recurse through related
skills.

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
- Conditional symlink behavior and `conf/manifest.toml` coverage must stay
  synchronized.

## Guidance ownership

- [Contributing](docs/CONTRIBUTING.md) owns the human change workflow.
- [Architecture](docs/ARCHITECTURE.md) describes layers and runtime contracts.
- [Testing](docs/TESTING.md) owns validation commands and CI coverage.
- Skills contain only task-specific procedures and subsystem gotchas.
