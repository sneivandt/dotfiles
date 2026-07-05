# APM Tooling

This repository uses [Microsoft APM](https://github.com/microsoft/apm) to
install AI-agent skills, prompts, MCP servers, hooks, and user-scope
instructions. Dotfiles owns the orchestration around APM so each machine
converges to the same declared AI tooling without advancing dependency versions
except during `dotfiles update`.

## What Lives Where

| Path | Purpose |
| --- | --- |
| `symlinks/apm/config/*.yml` | Manifest fragments linked into `~/.apm/config/`. |
| `symlinks/apm/plugins/dot-*` | Local APM plugins for reusable personal skills and agent workflows. |
| `conf/symlinks.toml` | Links APM config fragments and local plugins into `~/.apm/`. |
| `cli/src/tasks/ai/apm/` | Rust tasks that merge fragments, install/update APM deps, choose targets, and repair Copilot App workflows. |
| `cli/src/config/apm.rs` | Config validation for APM fragments, local plugin references, and direct MCP declarations. |
| `cli/src/tasks/validation/checks.rs` | `ValidateApmPlugins`, which runs APM's own package dry-run validator. |

The checked-in `base.yml` fragment declares the local `dot-*` plugins plus
external dependencies. Platform-specific fragments, such as `arch.yml`, add
extra dependencies only when that fragment is linked by the selected profile.
Private overlays can add more fragments under their own `symlinks/apm/config/`
tree; the merge task treats them the same way.

## Local Plugins

Local plugins live under `symlinks/apm/plugins/` and are referenced from
fragments as `~/.apm/plugins/dot-name`. Keep plugin names short and prefixed
with `dot-`.

Each plugin uses APM's native layout:

```text
symlinks/apm/plugins/dot-code/
├── apm.yml
└── .apm/
    └── skills/
        └── project-hygiene/
            └── SKILL.md
```

The plugin manifest should include package metadata and a dependency block so
`apm pack --dry-run --verbose` can validate it:

```yaml
name: dot-code
version: 1.0.0
description: Coding workflows and preferences
license: MIT
includes: auto
dependencies:
  apm: []
```

Use `.apm/<type>/` for every primitive, even when APM accepts root convention
directories for some package formats. That layout works consistently for both
`apm install` and `apm pack`.

## Manifest Fragments

Dotfiles does not hand-author `~/.apm/apm.yml`. Instead, it links one or more
fragment files into `~/.apm/config/` and generates the final manifest during
install/update.

Fragments can declare any APM-supported manifest fields, but dependencies are
the main use:

```yaml
name: dotfiles
version: 1.0.0
dependencies:
  apm:
    - ~/.apm/plugins/dot-agent
    - github/awesome-copilot/skills/agent-supply-chain#main
  mcp:
    - name: example-server
      command: example-mcp
```

The merge code:

1. Reads every `~/.apm/config/*.yml` and `*.yaml` file in sorted order.
2. Generates `~/.apm/apm.yml` with stable `name: dotfiles` and
   `version: 1.0.0`.
3. Concatenates dependency groups under both `dependencies` and
   `devDependencies`.
4. Deduplicates dependencies, using MCP `name` fields when available.
5. Recursively layers other manifest fields.

The generated manifest starts with a warning header because manual edits are
overwritten on the next dotfiles run.

## Install and Update Flow

APM work is split between convergence and version advancement.

| Command | Task | What it does |
| --- | --- | --- |
| `dotfiles install` | `Install APM packages` | Merges fragments, writes `~/.apm/apm.yml`, runs `apm install -g`, and records a success marker for the merged manifest. |
| `dotfiles update` | `Install APM packages` | Runs the same convergence step first. |
| `dotfiles update` | `Update APM packages` | Runs `apm outdated -g`; if dependencies are stale, runs `apm update -g --yes`. |
| `dotfiles test` | `Validate APM plugins` | Runs `apm pack --dry-run --verbose` for each local `dot-*` plugin when `apm` is installed. |

`install` never advances locked dependency refs. It converges to whatever is
already pinned in `~/.apm/apm.lock.yaml`, or creates that lockfile on first
install.

`update` is the only dotfiles command that advances refs. Before contacting APM
for updates, it checks that the current generated manifest has already installed
successfully: the lockfile must exist and the dotfiles success marker must match
the current merged manifest hash. This prevents a failed or partial install from
moving locked refs forward.

## Target Selection

Dotfiles installs APM packages globally for the supported AI runtimes:

```text
copilot,codex
```

When `~/.copilot/data.db` exists, dotfiles also adds the experimental
`copilot-app` target:

```text
copilot,codex,copilot-app
```

That database check avoids materializing Copilot App workflows before the app
has initialized its local state. When `copilot-app` is active, dotfiles
idempotently enables APM's `copilot-app` experimental flag before running the
install/update command.

## Copilot App Workflow Fixup

APM deploys Copilot App workflows secure-by-default: rows arrive disabled and in
interactive mode. Dotfiles-managed workflows are intended to run hands-off, so
after a successful `apm install` or `apm update`, dotfiles re-arms only the
workflows deployed by the current generated manifest.

The fixup is deliberately scoped:

1. Read workflow IDs from `~/.apm/apm.lock.yaml` entries such as
   `copilot-app-db://workflows/<id>`.
2. Update only those IDs in `~/.copilot/data.db`.
3. Set them to `mode = 'autopilot'` and `enabled = 1`.

Dotfiles never updates every `apm--*` workflow on the machine because unrelated
APM installs may also use that namespace. If the database is missing, locked, or
has an unexpected schema, the APM operation still succeeds and dotfiles prints a
warning with the manual recovery step.

## MCP Servers, Hooks, and Instructions

Prefer native APM primitives for AI-tooling content:

| Primitive | Where to declare it |
| --- | --- |
| MCP server | `dependencies.mcp` in a fragment or plugin manifest. |
| Agent runtime hook | `.apm/hooks/*.json` inside a local plugin. |
| Reusable user instruction | `.apm/instructions/*.instructions.md` inside a local plugin. |
| Triggerable workflow guidance | `.apm/skills/<name>/SKILL.md` inside a local plugin. |

Keep repository-specific instructions in `AGENTS.md`. Use APM instructions for
reusable user-scope preferences that should follow you across repositories.

For direct MCP declarations, keep self-defined servers direct in the fragment
unless you intentionally trust transitive MCP with APM's
`--trust-transitive-mcp`. The config validator checks that direct MCP mappings
have a non-empty `name` and either `command` or `url`.

## Validation

Use these checks when changing APM files:

```bash
# Validate config, symlink sources, scripts, and local APM plugin package shape.
./dotfiles.sh test

# Preview install convergence without applying changes.
./dotfiles.sh install -d

# Validate one local plugin directly.
cd symlinks/apm/plugins/dot-code
apm pack --dry-run --verbose
```

Rust changes under `cli/src/tasks/ai/apm/` also need the normal Rust checks:

```bash
cd cli
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test
```

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `apm not found in PATH` | APM is not installed. | Install with the platform package manager, then rerun dotfiles. |
| `apm install requires GitHub authentication` | A private or GitHub-hosted dependency needs credentials. | Run `gh auth login` or set `GH_TOKEN` / `GITHUB_TOKEN`. |
| `dotfiles update` skips APM advancement | The current manifest has not successfully installed yet. | Run `dotfiles install` first and resolve any install errors. |
| Copilot App workflows are not enabled | The app database was missing, locked, or schema-changed during fixup. | Open/close the Copilot App as suggested by the warning, then rerun `dotfiles install` or enable workflows manually. |
| `Validate APM plugins` fails | A local plugin manifest or primitive layout is invalid. | Run `apm pack --dry-run --verbose` in the named plugin directory and fix the reported package issue. |
