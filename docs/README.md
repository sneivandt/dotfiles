# Documentation

This directory documents the Rust-based dotfiles manager, its declarative
configuration, and the workflows used to develop and operate it.

## Start here

| Guide | Purpose |
|---|---|
| [Usage](USAGE.md) | Bootstrap the CLI and use every command and global option |
| [Task reference](TASKS.md) | Understand every install, update, uninstall, validation, and overlay task |
| [Configuration](CONFIGURATION.md) | Edit the TOML desired-state files safely |
| [Profiles](PROFILES.md) | Control role and platform-specific configuration |
| [Troubleshooting](TROUBLESHOOTING.md) | Diagnose common bootstrap, configuration, and convergence failures |

## Design and development

| Guide | Purpose |
|---|---|
| [Architecture](ARCHITECTURE.md) | Learn the CLI layers, task engine, resource model, and execution flow |
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

- `conf\` contains declarative desired state.
- `cli\src\app\catalog.rs` contains the static install and uninstall task
  catalogs.
- `cli\src\app\commands\test.rs` contains the validation task list.
- `dotfiles.sh` and `dotfiles.ps1` only bootstrap a binary and forward CLI
  arguments.
- `.github\workflows\` is authoritative for CI and publishing behavior.

The documentation explains those sources; it does not replace them.
