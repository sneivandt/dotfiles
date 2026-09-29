# Documentation

Dotfiles manages selected machine state from a Git checkout. The
[project README](../README.md) is the short introduction; these guides explain
how to operate it, change desired state, and maintain the implementation.

## Start here

| Your goal | Read |
|---|---|
| Set up a machine or preview an installation | [Usage](USAGE.md), then [Profiles](PROFILES.md) |
| Find a task or limit what a command runs | [Task reference](TASKS.md) |
| Change packages, links, settings, or services | [Configuration](CONFIGURATION.md) |
| Add private machine configuration | [Overlays](CONFIGURATION.md#overlays) |
| Understand a failure or an unexpected skip | [Troubleshooting](TROUBLESHOOTING.md) |
| Configure Windows or distinguish it from WSL | [Windows](WINDOWS.md) |
| Add agent packages, plugins, or deployment targets | [APM and AI tooling](APM.md) |

For a first installation, read the bootstrap and dry-run boundaries before
applying. For an existing installation, remember that application configuration
may be symlinked to this checkout: editing a file can affect a running app
without invoking the CLI.

## Design and development

Start with [Contributing](CONTRIBUTING.md) for the change workflow.
[Architecture](ARCHITECTURE.md) explains which layer should own a change;
[Testing](TESTING.md) maps that change to focused validation and describes
what CI does and does not cover.

| Guide | Use it to |
|---|---|
| [Architecture](ARCHITECTURE.md) | Trace commands, configuration, task scheduling, resources, and effects |
| [Contributing](CONTRIBUTING.md) | Make a safe, scoped change and prepare it for review |
| [Testing](TESTING.md) | Choose exact local checks and understand platform coverage |
| [Hooks](HOOKS.md) | Understand staged-index checks and diagnose commit failures |
| [Security](SECURITY.md) | Understand trust boundaries, verification limits, and private-data handling |

Agent-specific procedures live in [repository skills](../.agents/skills/),
with repository-wide constraints in [AGENTS.md](../AGENTS.md). Human contributors
do not need to read every skill to work on the project.

## Platforms and integrations

- [Windows](WINDOWS.md): bootstrap, symlink privileges, registry, PATH, and WSL.
- [APM](APM.md): source fragments, generated state, deployment targets, and updates.
- [Desktop configuration](CONFIGURATION.md#desktop-application-configuration):
  Quickshell appearance, reload behavior, and Hyprland media keys.
- [Vim and Neovim](../symlinks/vim/README.md): shared configuration, plugin
  bootstrap, lockfile changes, and Tree-sitter setup.

## Source-of-truth boundaries

Keep an explanation in its owning guide and link to it elsewhere. A new
behavior should not require copying the same contract into several documents.

| Question | Owning source |
|---|---|
| What should this machine contain? | [`conf/`](../conf/), with application sources in [`symlinks/`](../symlinks/) and administrator-file fragments in [`system/`](../system/) |
| How do I operate the CLI? | [Usage](USAGE.md); argument definitions in [`app/cli.rs`](../cli/src/app/cli.rs) |
| Which tasks exist? | [Task reference](TASKS.md); install/uninstall catalog in [`app/catalog.rs`](../cli/src/app/catalog.rs), validation list in [`app/commands/check.rs`](../cli/src/app/commands/check.rs) |
| How should a human make a change? | [Contributing](CONTRIBUTING.md) |
| What are the runtime and layer contracts? | [Architecture](ARCHITECTURE.md) |
| Which checks should run, and what does CI cover? | [Testing](TESTING.md); executable scripts and jobs in [`.github/workflows/`](../.github/workflows/) |
| What must an agent preserve across all tasks? | [AGENTS.md](../AGENTS.md) |
| How should an agent work in one subsystem? | The narrowest relevant [skill](../.agents/skills/) |

The wrappers [`dotfiles.sh`](../dotfiles.sh) and
[`dotfiles.ps1`](../dotfiles.ps1) bootstrap and forward arguments; they do not
own application tasks.

When guidance disagrees with behavior, inspect the owning source and regression
tests. Correct stale documentation, but do not redefine an established safety
or behavior requirement merely because an implementation currently violates it.
