# Documentation

These guides cover the Rust CLI, the desired state in `conf/`, and the
workflows for maintaining a configured machine.

## Start here

| Guide | Purpose |
|---|---|
| [Usage](USAGE.md) | Bootstrap the CLI; use its commands and scoped options |
| [Task reference](TASKS.md) | Look up install, pin-update, uninstall, validation, and overlay tasks |
| [Configuration](CONFIGURATION.md) | Edit the TOML desired-state files |
| [Profiles](PROFILES.md) | Select role-specific and platform-specific configuration |
| [Troubleshooting](TROUBLESHOOTING.md) | Diagnose bootstrap, configuration, and convergence failures |

## Design and development

| Guide | Purpose |
|---|---|
| [Architecture](ARCHITECTURE.md) | See the CLI layers, task engine, resource model, and execution flow |
| [Contributing](CONTRIBUTING.md) | Build, test, and change the project |
| [Testing](TESTING.md) | Run local checks and understand CI coverage |
| [Hooks](HOOKS.md) | Understand installed Git hooks and sensitive-data checks |
| [Security](SECURITY.md) | Review trust boundaries, download verification, and secret handling |

## Platforms and integrations

| Guide | Purpose |
|---|---|
| [Windows](WINDOWS.md) | Windows bootstrap, Developer Mode, registry, PATH, and WSL behavior |
| [APM](APM.md) | Manage AI tooling packages, plugins, and generated configuration |
| [Docker](DOCKER.md) | Build and use the container image |

## Source-of-truth boundaries

- `AGENTS.md` contains repository-wide agent invariants and skill routing.
- `CONTRIBUTING.md` contains the human development and contribution workflow.
- `ARCHITECTURE.md` describes the system's layers and runtime contracts.
- `TESTING.md` is the canonical reference for validation commands and CI
  coverage.
- `.agents/skills/*/SKILL.md` contains narrow agent procedures and subsystem
  gotchas.
- `conf/` contains declarative desired state.
- `cli/src/app/catalog.rs` contains the static install and uninstall task
  catalogs.
- `cli/src/app/commands/test.rs` contains the validation task list.
- `dotfiles.sh` and `dotfiles.ps1` only bootstrap a binary and forward CLI
  arguments.
- `.github/workflows/` is the executable source for CI and publishing behavior;
  `TESTING.md` explains its coverage.

When a guide and one of these sources disagree, follow the source.
