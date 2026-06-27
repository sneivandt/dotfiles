---
name: apm-dotfiles
description: Use when changing, reviewing, or designing Stuart's personal APM setup through dotfiles, including per-agent skills/plugins, APM config fragments, MCP/hooks/instructions delivered by APM, and private overlay APM content.
---

# APM Dotfiles

Use this skill for Stuart's personal agent customization system: public dotfiles
declare the reusable baseline, and optional private overlays can add private APM
fragments, local plugins, skills, hooks, MCP servers, and instructions.

## Mental Model

- Public dotfiles link `symlinks/apm/config/*.yml` and
  `symlinks/apm/plugins/*` into `~/.apm/` through `conf/symlinks.toml`.
- The Rust dotfiles engine installs APM during the AI tooling task:
  `install` runs `apm install`; `update` runs `apm outdated` and `apm update`
  only after a successful installed state exists.
- APM merges every `~/.apm/config/*.yml` fragment, so the baseline public
  fragment and private overlay fragments compose into one user-level agent
  environment.
- Local APM plugins are the preferred way to distribute reusable personal
  primitives. Keep plugin names short and scoped, such as `dot-code`,
  `dot-copilot`, and `dot-skill`.

## Per-Agent Organization

- Put coding, review, refactoring, and implementation preferences in
  `dot-code`.
- Put Copilot interaction behavior, status-update style, chat workflows, and
  user-experience preferences in `dot-copilot`.
- Put skill/plugin authoring, curation, maintenance, and APM workflow guidance
  in `dot-skill`.
- Prefer one small composable skill per recurring behavior. Do not bury
  project-specific rules in global APM skills when `.github/skills/` or
  repository instructions would be more precise.

## Private Overlay APM

Private overlay repos may add APM content without publishing it in the public
dotfiles repo:

- Add overlay symlink config that links private `apm/config/*.yml` fragments
  into `~/.apm/config/`.
- Add overlay symlink config that links private `apm/plugins/*` directories
  into `~/.apm/plugins/`.
- Keep work, employer, host-specific, or private skills in overlay plugins.
  Public dotfiles should only contain reusable, non-sensitive defaults.
- Overlay APM fragments can add private plugin dependencies, private MCP server
  declarations, hooks, and instructions. Use environment variable references
  for credentials; never hard-code secrets in fragments, hooks, skills, or
  plugin manifests.
- Prefer direct MCP declarations in the private fragment unless explicitly
  choosing to trust transitive MCP servers.

## Editing Rules

- Use APM native plugin layout: `apm.yml` at plugin root and primitives under
  `.apm/`, for example `.apm/skills/<skill>/SKILL.md`.
- Keep `includes: auto` unless a plugin needs a deliberate include allow-list.
- Reference local plugins with forward slashes, even for Windows-compatible
  config, because APM and dotfiles validation expect that form.
- When adding an external APM dependency, update the relevant
  `symlinks/apm/config/*.yml` fragment. Do not hard-code lock SHAs there; APM
  records exact refs in the user lockfile.
- When adding a local personal skill, choose the existing `dot-*` plugin that
  matches the audience before creating a new plugin.

## Validation

- After public APM config or local plugin changes, run `./dotfiles.sh install -d`
  to verify symlinks, manifest merging, and install planning.
- For Rust changes under `cli/src/tasks/ai/apm/`, also run the repo's Rust and
  cross-platform checks.
- If the change is private-overlay-only, validate from a checkout where the
  overlay is configured so the merged `~/.apm/config/*.yml` view is exercised.
