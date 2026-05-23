---
name: ai-tooling-apm
description: >
  AI tooling and APM plugin workflow for this dotfiles repo. Use when changing
  Copilot/agent settings, APM dependencies, local APM plugins, or skill
  distribution through symlinks/apm.
---

# AI Tooling and APM

This repo provisions AI tooling as part of the dotfiles system. APM config is
managed under `symlinks/apm/`, linked into the home directory, then installed by
the Rust `InstallApmPackages` task.

## Moving Parts

| Path | Purpose |
|---|---|
| `symlinks/apm/config/base.yml` | APM manifest fragment linked to `~/.apm/config/base.yml` |
| `symlinks/apm/plugins/dot-code` | Local coding workflow skills |
| `symlinks/apm/plugins/dot-doc` | Local document-generation workflow skills |
| `symlinks/apm/plugins/dot-skill` | Local skill/plugin maintenance skills |
| `conf/symlinks.toml` | Links `apm/config/base.yml` and `apm/plugins/*` from this repo |
| `cli/src/phases/apply/apm.rs` | Merges APM fragments and runs global APM dependency updates |

## When to Change What

- Update `.github/skills/` for repository-specific coding patterns that should
  guide agents working on this dotfiles codebase.
- Update `symlinks/apm/plugins/dot-*` for personal reusable skills that should
  be installed into the user's global APM environment.
- Update `symlinks/apm/config/base.yml` when adding, removing, or re-grouping
  external APM plugin dependencies.
- Update docs when the user-facing install, Windows, or usage workflow changes.

## Local Plugin Rules

- Keep local plugin names short and `dot-*`: `dot-code`, `dot-doc`, `dot-skill`.
- Ensure each plugin folder has a matching `plugin.json` `name` field.
- Keep skills concise and composable; prefer updating an existing related skill
  over adding a near-duplicate.
- Do not put secrets, tokens, private URLs, or employer-confidential content in
  skills, plugin manifests, or APM config.

## Validation

After APM config or local plugin changes, run:

```sh
./dotfiles.sh install -d
```

For changes to `cli/src/phases/apply/apm.rs`, also run the Rust checks from the
`cross-platform-verification` skill.
